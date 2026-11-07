//! JNI entry points for the Kotlin side (`dev.timlo.planner.Core`): the
//! foreground service, alarm and notification-button receivers call these.
//! They run in the app's process, beside the Tauri UI, on the same database.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use chrono::Local;
use jni::objects::{GlobalRef, JClass, JString};
use jni::sys::{jboolean, jstring, JNI_FALSE, JNI_TRUE};
use jni::{JNIEnv, JavaVM};
use planner_core::{paths, Status, Store};
use planner_sync::SyncEvent;

use crate::phone;

static STORE: Mutex<Option<Store>> = Mutex::new(None);
static SYNC: Mutex<Option<Sender<SyncEvent>>> = Mutex::new(None);
/// The JVM and the `Core` class, to call `Core.onNativeChange()` from any thread.
static CALLBACK: OnceLock<(JavaVM, GlobalRef)> = OnceLock::new();

fn log(msg: &str) {
    eprintln!("timlo: {msg}");
}

/// Point every file at the app's private directory. Must run before the
/// database or sync is used; repeated calls are harmless.
fn init(dir: &str) {
    let dir = PathBuf::from(dir);
    for (var, file) in [
        ("TIMLO_DB", "planner.db"),
        ("TIMLO_SYNC_CONFIG", "sync.toml"),
        ("TIMLO_SESSION", "session.json"),
        ("TIMLO_PLAN", "plan.toml"),
        ("TIMLO_SOCKET", "plannerd.sock"),
    ] {
        if std::env::var_os(var).is_none() {
            std::env::set_var(var, dir.join(file));
        }
    }
}

fn with_store<T>(f: impl FnOnce(&Store) -> anyhow::Result<T>) -> anyhow::Result<T> {
    let mut guard = STORE.lock().map_err(|_| anyhow::anyhow!("store lock poisoned"))?;
    if guard.is_none() {
        *guard = Some(Store::open(&paths::db_path())?);
    }
    f(guard.as_ref().unwrap())
}

/// Start the sync threads if logged in and not already running.
pub fn ensure_sync() {
    let Ok(mut guard) = SYNC.lock() else { return };
    if guard.as_ref().is_some_and(|tx| tx.send(SyncEvent::Local).is_ok()) {
        return;
    }
    *guard = None;
    match planner_sync::start(paths::db_path(), notify_kotlin) {
        Ok(tx) => *guard = tx,
        Err(e) => log(&format!("sync not started: {e:#}")),
    }
}

/// Something changed locally (app UI or a notification button): push it and
/// refresh the notification.
pub fn changed() {
    if let Ok(guard) = SYNC.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(SyncEvent::Local);
        }
    }
    notify_kotlin();
}

/// Ask the Kotlin side to redraw the notification (it posts to its main thread).
fn notify_kotlin() {
    let Some((vm, class)) = CALLBACK.get() else { return };
    let Ok(mut env) = vm.attach_current_thread() else { return };
    let class: &JClass = class.as_obj().into();
    if env.call_static_method(class, "onNativeChange", "()V", &[]).is_err() {
        let _ = env.exception_clear();
    }
}

fn jstr(env: &mut JNIEnv, s: &str) -> jstring {
    env.new_string(s).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

fn rstr(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(Into::into).unwrap_or_default()
}

#[no_mangle]
pub extern "system" fn Java_dev_timlo_planner_Core_init<'l>(mut env: JNIEnv<'l>, class: JClass<'l>, dir: JString<'l>) {
    let dir = rstr(&mut env, &dir);
    init(&dir);
    if CALLBACK.get().is_none() {
        if let (Ok(vm), Ok(global)) = (env.get_java_vm(), env.new_global_ref(&class)) {
            let _ = CALLBACK.set((vm, global));
        }
    }
}

/// JSON [`phone::Tick`], or `{"error": "..."}`.
#[no_mangle]
pub extern "system" fn Java_dev_timlo_planner_Core_tick<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>) -> jstring {
    let out = with_store(|s| Ok(serde_json::to_string(&phone::tick(s, Local::now().naive_local())?)?))
        .unwrap_or_else(|e| serde_json::json!({ "error": format!("{e:#}") }).to_string());
    jstr(&mut env, &out)
}

/// Record Done / Skip. An empty `block_id` means the "now" card's block.
/// Returns an error message, or "" on success.
#[no_mangle]
pub extern "system" fn Java_dev_timlo_planner_Core_answer<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    block_id: JString<'l>,
    status: JString<'l>,
) -> jstring {
    let id = rstr(&mut env, &block_id);
    let status = match rstr(&mut env, &status).as_str() {
        "done" => Status::Done,
        "skipped" => Status::Skipped,
        other => return jstr(&mut env, &format!("unknown status {other:?}")),
    };
    let id = Some(id.as_str()).filter(|s| !s.is_empty());
    let msg = match with_store(|s| phone::answer(s, id, status, Local::now().naive_local())) {
        Ok(_) => {
            changed();
            String::new()
        }
        Err(e) => format!("{e:#}"),
    };
    jstr(&mut env, &msg)
}

#[no_mangle]
pub extern "system" fn Java_dev_timlo_planner_Core_blockOpen<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    block_id: JString<'l>,
) -> jboolean {
    let id = rstr(&mut env, &block_id);
    match with_store(|s| phone::block_open(s, &id)) {
        Ok(true) => JNI_TRUE,
        _ => JNI_FALSE,
    }
}

#[no_mangle]
pub extern "system" fn Java_dev_timlo_planner_Core_startSync<'l>(_env: JNIEnv<'l>, _class: JClass<'l>) {
    ensure_sync();
}

/// The widget or notification asked the UI to open a screen ("focus", "add-task").
#[no_mangle]
pub extern "system" fn Java_dev_timlo_planner_Core_setLaunchAction<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    action: JString<'l>,
) {
    if let Ok(a) = env.get_string(&action) {
        crate::set_launch_action(&String::from(a));
    }
}

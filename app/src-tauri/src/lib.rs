//! Tauri backend: thin commands over `planner-core`. Every write tells the
//! background piece (`plannerd` on Linux, the service on Android) so the
//! bar, notification and reminders pick up the change at once.

#[cfg(target_os = "android")]
mod android;
pub mod phone;

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{Local, NaiveDate, NaiveDateTime};
use planner_core::store::weekday_num;
use planner_core::time::minute_of;
use planner_core::settings::{fmt_quiet_hours, parse_quiet_hours};
use planner_core::{paths, BlockInput, DayBlock, FocusSession, Settings, Status, Store, Task, Template, TemplateBlock};
use planner_sync::{LoginOutcome, SyncConfig};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

struct AppState(Mutex<Store>);

type CmdResult<T> = Result<T, String>;

fn now() -> NaiveDateTime {
    Local::now().naive_local()
}

fn parse_date(s: &str) -> CmdResult<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| format!("bad date {s:?}: {e}"))
}

/// Run `f` against the store, turning errors into messages for the UI.
fn with_store<T>(state: &State<AppState>, f: impl FnOnce(&Store) -> anyhow::Result<T>) -> CmdResult<T> {
    let store = state.0.lock().map_err(|_| "database lock poisoned".to_string())?;
    f(&store).map_err(|e| format!("{e:#}"))
}

/// Something changed in the database.
fn changed() {
    #[cfg(target_os = "android")]
    android::changed();
    #[cfg(not(target_os = "android"))]
    paths::poke_daemon();
}

/// Like `with_store`, then tell the background piece something changed.
fn write<T>(state: &State<AppState>, f: impl FnOnce(&Store) -> anyhow::Result<T>) -> CmdResult<T> {
    let out = with_store(state, f)?;
    changed();
    Ok(out)
}

#[derive(Deserialize)]
struct BlockDto {
    id: Option<String>,
    start_min: u16,
    end_min: u16,
    title: String,
    note: Option<String>,
    icon: Option<String>,
    colour: Option<String>,
}

impl From<BlockDto> for BlockInput {
    fn from(b: BlockDto) -> Self {
        let clean = |s: Option<String>| s.filter(|v| !v.trim().is_empty());
        BlockInput {
            id: b.id,
            start_min: b.start_min,
            end_min: b.end_min,
            title: b.title,
            note: clean(b.note),
            icon: clean(b.icon),
            colour: clean(b.colour),
        }
    }
}

#[derive(Serialize)]
struct DayDto {
    date: NaiveDate,
    template_id: Option<String>,
    template_name: Option<String>,
    default_template_id: Option<String>,
    customised: bool,
    overridden: bool,
    is_today: bool,
    is_past: bool,
    /// Minute of the day, only for today.
    now_min: Option<u16>,
    blocks: Vec<DayBlock>,
    /// Icon and colour of each block, from the template block it was copied
    /// from (keyed by day block id; blocks added by hand have none).
    looks: HashMap<String, Look>,
}

#[derive(Serialize)]
struct Look {
    icon: Option<String>,
    colour: Option<String>,
}

fn looks(s: &Store, blocks: &[DayBlock]) -> anyhow::Result<HashMap<String, Look>> {
    let mut by_template_block = HashMap::new();
    for t in s.templates()? {
        for tb in s.template_blocks(&t.id)? {
            by_template_block.insert(tb.id.clone(), tb);
        }
    }
    Ok(blocks
        .iter()
        .filter_map(|b| {
            let tb = by_template_block.get(b.template_block_id.as_deref()?)?;
            (tb.icon.is_some() || tb.colour.is_some())
                .then(|| (b.id.clone(), Look { icon: tb.icon.clone(), colour: tb.colour.clone() }))
        })
        .collect())
}

#[derive(Serialize)]
struct TemplateDto {
    #[serde(flatten)]
    template: Template,
    blocks: Vec<TemplateBlock>,
}

#[derive(Serialize)]
struct MonthDayDto {
    date: NaiveDate,
    template_name: Option<String>,
    /// The day has its own plan (false = it would follow the weekly default).
    exists: bool,
    overridden: bool,
    customised: bool,
    total: usize,
    done: usize,
    skipped: usize,
    missed: usize,
}

// ------------------------------------------------------------------ days

#[tauri::command]
fn get_day(state: State<AppState>, date: Option<String>) -> CmdResult<DayDto> {
    let now = now();
    let date = match date {
        Some(d) => parse_date(&d)?,
        None => now.date(),
    };
    with_store(&state, |s| {
        let default_template_id = s.week_assignment()?[(weekday_num(date) - 1) as usize].clone();
        // Looking at a past date that was never planned doesn't create it.
        let day = if date < now.date() { s.day(date)? } else { Some(s.ensure_day(date, now)?) };
        s.mark_missed(now)?;
        let blocks = if day.is_some() { s.day_blocks(date)? } else { Vec::new() };
        let template_id = day.as_ref().and_then(|d| d.template_id.clone());
        let template_name = match &template_id {
            Some(id) => s.template(id)?.map(|t| t.name),
            None => None,
        };
        Ok(DayDto {
            date,
            template_id,
            template_name,
            default_template_id,
            customised: day.as_ref().is_some_and(|d| d.customised),
            overridden: day.as_ref().is_some_and(|d| d.overridden),
            is_today: date == now.date(),
            is_past: date < now.date(),
            now_min: (date == now.date()).then(|| minute_of(&now)),
            looks: looks(s, &blocks)?,
            blocks,
        })
    })
}

#[tauri::command]
fn set_status(state: State<AppState>, id: String, status: Status) -> CmdResult<DayBlock> {
    write(&state, |s| s.set_status(&id, status))
}

#[tauri::command]
fn switch_day(state: State<AppState>, date: String, template_id: Option<String>) -> CmdResult<()> {
    let date = parse_date(&date)?;
    write(&state, |s| s.switch_day_template(date, template_id.as_deref(), now()))
}

#[tauri::command]
fn add_day_block(state: State<AppState>, date: String, block: BlockDto) -> CmdResult<DayBlock> {
    let date = parse_date(&date)?;
    write(&state, |s| s.add_day_block(date, block.into(), now()))
}

#[tauri::command]
fn update_day_block(
    state: State<AppState>,
    id: String,
    start_min: u16,
    end_min: u16,
    title: String,
    note: Option<String>,
) -> CmdResult<()> {
    write(&state, |s| {
        let mut b = s.day_block(&id)?.ok_or_else(|| anyhow::anyhow!("no such block"))?;
        b.start_min = start_min;
        b.end_min = end_min;
        b.title = title;
        b.note = note.filter(|n| !n.trim().is_empty());
        s.update_day_block(&b)
    })
}

#[tauri::command]
fn delete_day_block(state: State<AppState>, id: String) -> CmdResult<()> {
    write(&state, |s| s.delete_day_block(&id))
}

#[tauri::command]
fn get_month(state: State<AppState>, year: i32, month: u32) -> CmdResult<Vec<MonthDayDto>> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).ok_or("bad month")?;
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .ok_or("bad month")?;
    with_store(&state, |s| {
        let names: HashMap<String, String> = s.templates()?.into_iter().map(|t| (t.id, t.name)).collect();
        let week = s.week_assignment()?;
        let last = next.pred_opt().unwrap_or(next);
        let days: HashMap<NaiveDate, _> =
            s.days_in_range(first, last)?.into_iter().map(|d| (d.date, d)).collect();
        let mut out = Vec::new();
        for date in first.iter_days().take_while(|d| *d < next) {
            let name_of = |id: &Option<String>| id.as_ref().and_then(|i| names.get(i).cloned());
            let mut row = MonthDayDto {
                date,
                template_name: None,
                exists: false,
                overridden: false,
                customised: false,
                total: 0,
                done: 0,
                skipped: 0,
                missed: 0,
            };
            match days.get(&date) {
                Some(day) => {
                    let blocks = s.day_blocks(date)?;
                    let count = |st: Status| blocks.iter().filter(|b| b.status == st).count();
                    row.template_name = name_of(&day.template_id);
                    row.exists = true;
                    row.overridden = day.overridden;
                    row.customised = day.customised;
                    row.total = blocks.len();
                    row.done = count(Status::Done);
                    row.skipped = count(Status::Skipped);
                    row.missed = count(Status::Missed);
                }
                None => row.template_name = name_of(&week[(weekday_num(date) - 1) as usize]),
            }
            out.push(row);
        }
        Ok(out)
    })
}

#[derive(Serialize)]
struct ActivityDto {
    date: NaiveDate,
    total: usize,
    done: usize,
    skipped: usize,
    missed: usize,
    /// Minutes of Focus sessions started that day.
    focus_min: u32,
}

/// Block outcomes and focus time for every day in `[from, to]` that has
/// either, for the activity map and progress chart.
#[tauri::command]
fn get_activity(state: State<AppState>, from: String, to: String) -> CmdResult<Vec<ActivityDto>> {
    let (from, to) = (parse_date(&from)?, parse_date(&to)?);
    with_store(&state, |s| {
        let mut focus = s.focus_minutes(from, to)?;
        let mut out = Vec::new();
        for day in s.days_in_range(from, to)? {
            let blocks = s.day_blocks(day.date)?;
            let count = |st: Status| blocks.iter().filter(|b| b.status == st).count();
            out.push(ActivityDto {
                date: day.date,
                total: blocks.len(),
                done: count(Status::Done),
                skipped: count(Status::Skipped),
                missed: count(Status::Missed),
                focus_min: focus.remove(&day.date).unwrap_or(0),
            });
        }
        out.extend(focus.into_iter().map(|(date, focus_min)| ActivityDto {
            date,
            total: 0,
            done: 0,
            skipped: 0,
            missed: 0,
            focus_min,
        }));
        out.sort_by_key(|a| a.date);
        Ok(out)
    })
}

#[tauri::command]
fn week_summary(state: State<AppState>, date: String) -> CmdResult<planner_core::summary::WeekSummary> {
    let date = parse_date(&date)?;
    with_store(&state, |s| planner_core::summary::week(s, date))
}

#[tauri::command]
fn save_focus_session(state: State<AppState>, session: FocusSession) -> CmdResult<()> {
    write(&state, |s| s.save_focus_session(&session))
}

/// Put a (missed) block into the first free slot later today or on `to`.
#[tauri::command]
fn move_block(state: State<AppState>, id: String, to: String) -> CmdResult<DayBlock> {
    let to = parse_date(&to)?;
    write(&state, |s| s.move_block(&id, to, now()))
}

// ---- Launch actions --------------------------------------------------------
// `timlo --add-task` / `--focus` (Waybar right-click, the Android widget and
// notification) ask the UI to open a screen. Kept until the UI takes it.

static LAUNCH_ACTION: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn set_launch_action(action: &str) {
    if let Ok(mut a) = LAUNCH_ACTION.lock() {
        *a = Some(action.to_string());
    }
}

#[cfg(desktop)]
fn action_from_args(args: &[String]) -> Option<&'static str> {
    args.iter().find_map(|a| match a.as_str() {
        "--add-task" => Some("add-task"),
        "--focus" => Some("focus"),
        _ => None,
    })
}

#[tauri::command]
fn take_launch_action() -> Option<String> {
    LAUNCH_ACTION.lock().ok().and_then(|mut a| a.take())
}

// ------------------------------------------------------------------ focus

/// `local.` = this device only: each device has its own music folder.
const FOCUS_DIR_KEY: &str = "local.focus.dir";

fn default_focus_dir() -> String {
    if cfg!(target_os = "android") {
        "/storage/emulated/0/Music/Focus".into()
    } else {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/Music/focus")
    }
}

fn focus_dir(s: &Store) -> anyhow::Result<String> {
    Ok(s.setting(FOCUS_DIR_KEY)?.filter(|d| !d.trim().is_empty()).unwrap_or_else(default_focus_dir))
}

#[derive(Serialize)]
struct FocusTracks {
    dir: String,
    /// Absolute paths of the audio files found (recursively).
    tracks: Vec<String>,
    /// Why nothing could be listed (missing folder, no permission …).
    problem: Option<String>,
}

const AUDIO_EXT: &[&str] = &["mp3", "ogg", "oga", "opus", "flac", "wav", "m4a", "aac", "webm"];

fn find_audio(dir: &std::path::Path, out: &mut Vec<String>, depth: u8) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if depth < 4 {
                let _ = find_audio(&path, out, depth + 1);
            }
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| AUDIO_EXT.contains(&e.to_ascii_lowercase().as_str()))
        {
            out.push(path.to_string_lossy().into_owned());
        }
    }
    Ok(())
}

/// The audio files in the focus folder.
#[tauri::command]
fn focus_tracks(state: State<AppState>) -> CmdResult<FocusTracks> {
    let dir = with_store(&state, focus_dir)?;
    let mut tracks = Vec::new();
    let problem = match find_audio(std::path::Path::new(&dir), &mut tracks, 0) {
        Err(e) => Some(e.to_string()),
        Ok(()) if tracks.is_empty() => Some("no audio files in this folder".into()),
        Ok(()) => None,
    };
    tracks.sort();
    Ok(FocusTracks { dir, tracks, problem })
}

#[derive(Serialize)]
struct FocusChunk {
    /// Base64 of the bytes from `offset`.
    data: String,
    /// Size of the whole file.
    size: u64,
}

/// Up to `len` bytes of a focus track from `offset`, as base64. Raw bytes
/// would reach the Linux webview as a JSON array of numbers, which is huge
/// and slow; base64 is compact and decodes fast on every platform.
#[tauri::command]
fn focus_read(state: State<AppState>, path: String, offset: u64, len: u32) -> CmdResult<FocusChunk> {
    use base64::Engine;
    use std::io::{Read, Seek, SeekFrom};
    let dir = std::fs::canonicalize(with_store(&state, focus_dir)?).map_err(|e| e.to_string())?;
    let file = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
    if !file.starts_with(&dir) {
        return Err("not in the focus folder".into());
    }
    let read = || -> std::io::Result<FocusChunk> {
        let mut f = std::fs::File::open(&file)?;
        let size = f.metadata()?.len();
        f.seek(SeekFrom::Start(offset))?;
        let mut buf = Vec::with_capacity(len.min(16 << 20) as usize);
        f.take(len.min(16 << 20) as u64).read_to_end(&mut buf)?;
        Ok(FocusChunk { data: base64::engine::general_purpose::STANDARD.encode(&buf), size })
    };
    read().map_err(|e| format!("{path}: {e}"))
}

// ------------------------------------------------------------------ templates

#[tauri::command]
fn list_templates(state: State<AppState>) -> CmdResult<Vec<TemplateDto>> {
    with_store(&state, |s| {
        s.templates()?
            .into_iter()
            .map(|t| Ok(TemplateDto { blocks: s.template_blocks(&t.id)?, template: t }))
            .collect()
    })
}

#[tauri::command]
fn create_template(state: State<AppState>, name: String) -> CmdResult<Template> {
    write(&state, |s| s.create_template(&name, None))
}

#[tauri::command]
fn rename_template(state: State<AppState>, id: String, name: String) -> CmdResult<()> {
    write(&state, |s| s.rename_template(&id, &name))
}

#[tauri::command]
fn duplicate_template(state: State<AppState>, id: String, name: String) -> CmdResult<Template> {
    write(&state, |s| s.duplicate_template(&id, &name))
}

#[tauri::command]
fn delete_template(state: State<AppState>, id: String) -> CmdResult<()> {
    write(&state, |s| s.delete_template(&id))
}

#[tauri::command]
fn save_template_blocks(
    state: State<AppState>,
    id: String,
    blocks: Vec<BlockDto>,
) -> CmdResult<Vec<TemplateBlock>> {
    let blocks: Vec<BlockInput> = blocks.into_iter().map(Into::into).collect();
    write(&state, |s| s.set_template_blocks(&id, &blocks, now()))
}

// ------------------------------------------------------------------ week

#[tauri::command]
fn get_week(state: State<AppState>) -> CmdResult<Vec<Option<String>>> {
    with_store(&state, |s| Ok(s.week_assignment()?.to_vec()))
}

#[tauri::command]
fn set_week(state: State<AppState>, weekday: u8, template_id: Option<String>) -> CmdResult<()> {
    write(&state, |s| s.set_week_assignment(weekday, template_id.as_deref(), now()))
}

// ---- Tasks ---------------------------------------------------------------

#[derive(Serialize)]
struct TaskList {
    tasks: Vec<Task>,
    /// Done tasks (hidden unless shown).
    done_count: usize,
}

#[tauri::command]
fn list_tasks(state: State<AppState>, show_done: bool) -> CmdResult<TaskList> {
    with_store(&state, |s| Ok(TaskList { tasks: s.tasks(show_done)?, done_count: s.done_task_count()? }))
}

#[tauri::command]
fn add_task(state: State<AppState>, title: String, parent_id: Option<String>) -> CmdResult<Task> {
    write(&state, |s| s.add_task(&title, parent_id.as_deref()))
}

#[tauri::command]
fn update_task(state: State<AppState>, id: String, title: String, notes: Option<String>) -> CmdResult<()> {
    write(&state, |s| s.update_task(&id, &title, notes.as_deref()))
}

#[tauri::command]
fn set_task_done(state: State<AppState>, id: String, done: bool) -> CmdResult<()> {
    write(&state, |s| s.set_task_done(&id, done))
}

#[tauri::command]
fn delete_task(state: State<AppState>, id: String) -> CmdResult<()> {
    write(&state, |s| s.delete_task(&id))
}

// ---- Settings ------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct SettingsDto {
    remind_start: bool,
    remind_before_start: u16,
    remind_before_end: u16,
    remind_unfinished: bool,
    ending_soon: u16,
    /// `"23:00-07:00"`, or empty for none.
    quiet_hours: String,
    sound: bool,
    /// Desktop only, per device: hide the window when it loses focus.
    hide_on_blur: bool,
    /// Per device: folder the Focus tab plays music from.
    focus_dir: String,
}

/// `local.` = this device only, never synced.
const HIDE_ON_BLUR_KEY: &str = "local.desktop.hide_on_blur";

#[tauri::command]
fn get_settings(state: State<AppState>) -> CmdResult<SettingsDto> {
    with_store(&state, |s| {
        let v = Settings::load(s)?;
        Ok(SettingsDto {
            remind_start: v.remind_start,
            remind_before_start: v.remind_before_start,
            remind_before_end: v.remind_before_end,
            remind_unfinished: v.remind_unfinished,
            ending_soon: v.ending_soon,
            quiet_hours: fmt_quiet_hours(v.quiet_hours),
            sound: v.sound,
            hide_on_blur: s.setting(HIDE_ON_BLUR_KEY)?.as_deref() != Some("false"),
            focus_dir: focus_dir(s)?,
        })
    })
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: SettingsDto) -> CmdResult<()> {
    let quiet_hours = parse_quiet_hours(&settings.quiet_hours).map_err(|e| format!("{e:#}"))?;
    write(&state, |s| {
        Settings {
            remind_start: settings.remind_start,
            remind_before_start: settings.remind_before_start.min(240),
            remind_before_end: settings.remind_before_end.min(240),
            remind_unfinished: settings.remind_unfinished,
            ending_soon: settings.ending_soon.min(240),
            quiet_hours,
            sound: settings.sound,
        }
        .save(s)?;
        s.set_setting(HIDE_ON_BLUR_KEY, &settings.hide_on_blur.to_string())?;
        s.set_setting(FOCUS_DIR_KEY, settings.focus_dir.trim())
    })
}

// ---- Sync ----------------------------------------------------------------

#[derive(Serialize)]
struct SyncInfo {
    #[serde(flatten)]
    status: planner_sync::SyncStatus,
    /// Project to offer on the login form (saved one, else the build's default).
    default_url: String,
    default_key: String,
}

fn open_store() -> CmdResult<Store> {
    Store::open(&paths::db_path()).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn sync_status() -> CmdResult<SyncInfo> {
    let store = open_store()?;
    let status = planner_sync::login::status(&store).map_err(|e| format!("{e:#}"))?;
    let saved = SyncConfig::load().ok().flatten();
    Ok(SyncInfo {
        status,
        default_url: saved.as_ref().map_or(DEFAULT_SUPABASE_URL.into(), |c| c.url.clone()),
        default_key: saved.map_or(DEFAULT_SUPABASE_KEY.into(), |c| c.anon_key),
    })
}

/// Runs off the UI thread (network). Returns a message for the user.
#[tauri::command]
async fn sync_login(url: String, key: String, email: String, password: String, signup: bool) -> CmdResult<String> {
    let store = open_store()?;
    let cfg = SyncConfig::new(&url, &key);
    let out = planner_sync::login(&store, cfg, &email, &password, signup).map_err(|e| format!("{e:#}"))?;
    Ok(match out {
        LoginOutcome::ConfirmEmail => "Account created. Click the link in the confirmation email, then log in.".into(),
        LoginOutcome::LoggedIn { sent, received } => {
            #[cfg(target_os = "android")]
            android::ensure_sync();
            changed();
            format!("Logged in. Sent {sent} rows, received {received} changes.")
        }
    })
}

#[tauri::command]
async fn sync_logout() -> CmdResult<()> {
    planner_sync::logout().map_err(|e| format!("{e:#}"))?;
    changed();
    Ok(())
}

/// Defaults for the login form, from the repo's `.env` at build time.
const DEFAULT_SUPABASE_URL: &str = env!("TIMLO_DEFAULT_SUPABASE_URL");
const DEFAULT_SUPABASE_KEY: &str = env!("TIMLO_DEFAULT_SUPABASE_KEY");

/// On the desktop the window acts as a popup from Waybar: launching `timlo`
/// again toggles it, and it hides when it loses focus (unless turned off in
/// Settings). Hidden, not closed, so it comes back instantly.
#[cfg(desktop)]
mod popup {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Manager, Window, WindowEvent};

    use crate::AppState;

    /// Clicking the Waybar module can itself take focus away; a launch right
    /// after an auto-hide means "close", not "open again".
    static LAST_AUTO_HIDE: Mutex<Option<Instant>> = Mutex::new(None);
    /// Ignore focus flicker (popups, dialogs opening) shorter than this.
    const GRACE: Duration = Duration::from_millis(150);

    pub fn show(app: &AppHandle) {
        let Some(w) = app.get_webview_window("main") else { return };
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }

    pub fn toggle(app: &AppHandle) {
        let Some(w) = app.get_webview_window("main") else { return };
        let just_hidden = LAST_AUTO_HIDE.lock().ok().and_then(|t| *t).is_some_and(|t| t.elapsed() < Duration::from_millis(600));
        let visible = w.is_visible().unwrap_or(false);
        if just_hidden || (visible && w.is_focused().unwrap_or(false)) {
            let _ = w.hide();
        } else {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    }

    pub fn on_event(window: &Window, event: &WindowEvent) {
        if !matches!(event, WindowEvent::Focused(false)) || !hide_on_blur(window.app_handle()) {
            return;
        }
        let w = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(GRACE);
            if w.is_visible().unwrap_or(false) && !w.is_focused().unwrap_or(true) {
                let _ = w.hide();
                if let Ok(mut t) = LAST_AUTO_HIDE.lock() {
                    *t = Some(Instant::now());
                }
            }
        });
    }

    fn hide_on_blur(app: &AppHandle) -> bool {
        let state = app.state::<AppState>();
        let Ok(store) = state.0.lock() else { return true };
        store.setting(super::HIDE_ON_BLUR_KEY).ok().flatten().as_deref() != Some("false")
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();

    // A second launch (e.g. clicking the Waybar module again) focuses the
    // window that is already open.
    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| match action_from_args(&args) {
                Some(action) => {
                    set_launch_action(action);
                    popup::show(app);
                    let _ = tauri::Emitter::emit(app, "launch-action", ());
                }
                None => popup::toggle(app),
            }))
            .on_window_event(popup::on_event);
    }

    builder
        .setup(|app| {
            let store = Store::open(&paths::db_path())?;
            app.manage(AppState(Mutex::new(store)));
            #[cfg(desktop)]
            if let Some(action) = action_from_args(&std::env::args().collect::<Vec<_>>()) {
                set_launch_action(action);
            }
            #[cfg(target_os = "android")]
            android::ensure_sync();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_day,
            set_status,
            switch_day,
            add_day_block,
            update_day_block,
            delete_day_block,
            get_month,
            get_activity,
            week_summary,
            save_focus_session,
            move_block,
            take_launch_action,
            focus_tracks,
            focus_read,
            list_templates,
            create_template,
            rename_template,
            duplicate_template,
            delete_template,
            save_template_blocks,
            get_week,
            set_week,
            list_tasks,
            add_task,
            update_task,
            set_task_done,
            delete_task,
            get_settings,
            save_settings,
            sync_status,
            sync_login,
            sync_logout,
        ])
        .run(tauri::generate_context!())
        .expect("error while running timlo");
}

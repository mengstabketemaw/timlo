//! The background service: keeps today's day created, marks missed blocks,
//! fires reminders, streams Waybar updates and reloads the plan file.
//!
//! It wakes once a minute (at the minute boundary) and whenever a client
//! connects; everything else is idle.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime};
use std::{fs, thread};

use anyhow::{bail, Context, Result};
use chrono::{Local, NaiveDate, NaiveDateTime, Timelike};
use planner_core::time::minute_of;
use planner_core::{reminders, Settings, Status, Store};

use planner_core::paths;

use crate::{notify, waybar};

const PLAN_MTIME_KEY: &str = "plan_file_mtime";

pub enum Msg {
    Subscribe(UnixStream),
    Refresh,
    SetStatus(String, Status),
}

/// Bring today up to date and render the Waybar line.
pub fn current_output(store: &Store, now: NaiveDateTime) -> Result<waybar::Output> {
    store.ensure_day(now.date(), now)?;
    store.mark_missed(now)?;
    let blocks = store.day_blocks(now.date())?;
    let settings = Settings::load(store)?;
    Ok(waybar::render(&blocks, minute_of(&now), &settings))
}

pub fn run() -> Result<()> {
    let sock = paths::socket_path();
    let listener = bind(&sock)?;
    let (tx, rx) = mpsc::channel::<Msg>();
    {
        let tx = tx.clone();
        thread::spawn(move || accept_loop(listener, tx));
    }

    let db = paths::db_path();
    let plan = paths::plan_path();
    let store = Store::open(&db)?;
    eprintln!(
        "plannerd: database {} · plan file {} · socket {}",
        db.display(),
        plan.display(),
        sock.display()
    );

    let mut subs: Vec<UnixStream> = Vec::new();
    let mut last_line = String::new();
    let mut last_tick: Option<(NaiveDate, u16)> = None;

    loop {
        let now = Local::now().naive_local();
        reload_plan(&store, &plan, now);
        match tick(&store, now, &mut last_tick, &tx) {
            Ok(line) if line != last_line => {
                subs.retain_mut(|s| writeln!(s, "{line}").is_ok());
                last_line = line;
            }
            Ok(_) => {}
            Err(e) => eprintln!("plannerd: {e:#}"),
        }

        let into_minute = now.second() as u64 * 1000 + (now.nanosecond() / 1_000_000).min(999) as u64;
        let wait = Duration::from_millis(60_000 - into_minute + 20);
        match rx.recv_timeout(wait) {
            Ok(Msg::Subscribe(mut s)) => {
                if writeln!(s, "{last_line}").is_ok() {
                    subs.push(s);
                }
            }
            Ok(Msg::SetStatus(id, status)) => {
                if let Err(e) = store.set_status(&id, status) {
                    eprintln!("plannerd: {e:#}");
                }
            }
            Ok(Msg::Refresh) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => bail!("internal channel closed"),
        }
    }
}

/// Update today and fire reminders due since the last tick. Reminders are
/// only fired for minutes this process saw pass: after a start, a suspend or
/// a long gap, missed reminders are dropped rather than replayed at once.
fn tick(
    store: &Store,
    now: NaiveDateTime,
    last_tick: &mut Option<(NaiveDate, u16)>,
    tx: &Sender<Msg>,
) -> Result<String> {
    let out = current_output(store, now)?;
    let today = now.date();
    let m = minute_of(&now);
    let from = match *last_tick {
        Some((d, lm)) if d == today && m >= lm && m - lm <= 2 => lm + 1,
        Some(_) => m,
        None => m + 1,
    };
    *last_tick = Some((today, m));
    if from <= m {
        let settings = Settings::load(store)?;
        if !settings.is_quiet(m) {
            let blocks = store.day_blocks(today)?;
            for r in reminders::due(&blocks, from, m, &settings) {
                notify::reminder(&r, settings.sound, tx.clone());
            }
        }
    }
    Ok(serde_json::to_string(&out)?)
}

/// Import the plan file when it changed since the last import (tracked in
/// the database, so a restart doesn't overwrite edits made in the app).
fn reload_plan(store: &Store, path: &Path, now: NaiveDateTime) {
    let Ok(meta) = fs::metadata(path) else { return };
    let stamp = match meta.modified().ok().and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok()) {
        Some(d) => d.as_nanos().to_string(),
        None => return,
    };
    if store.setting(PLAN_MTIME_KEY).ok().flatten().as_deref() == Some(stamp.as_str()) {
        return;
    }
    match crate::import_file(store, path, now) {
        Ok(r) => eprintln!("plannerd: loaded {} ({} templates, {} blocks)", path.display(), r.templates, r.blocks),
        Err(e) => {
            eprintln!("plannerd: {}: {e:#}", path.display());
            notify::plain("Plan file not loaded", &format!("{e:#}"));
        }
    }
    // Recorded on failure too, so a broken file is reported once per save.
    if let Err(e) = store.set_setting(PLAN_MTIME_KEY, &stamp) {
        eprintln!("plannerd: {e:#}");
    }
}

fn bind(sock: &Path) -> Result<UnixListener> {
    if let Some(dir) = sock.parent() {
        fs::create_dir_all(dir)?;
    }
    if sock.exists() {
        if UnixStream::connect(sock).is_ok() {
            bail!("plannerd is already running ({})", sock.display());
        }
        fs::remove_file(sock)?;
    }
    UnixListener::bind(sock).with_context(|| format!("binding {}", sock.display()))
}

fn accept_loop(listener: UnixListener, tx: Sender<Msg>) {
    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let Ok(reader) = stream.try_clone() else { continue };
        let mut line = String::new();
        if BufReader::new(reader).read_line(&mut line).is_err() {
            continue;
        }
        let msg = match line.trim() {
            "subscribe" => {
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                Msg::Subscribe(stream)
            }
            "refresh" => Msg::Refresh,
            _ => continue,
        };
        if tx.send(msg).is_err() {
            break;
        }
    }
}

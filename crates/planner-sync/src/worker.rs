//! The background sync loop.
//!
//! - Local change → push (the daemon sends this after every write and once a
//!   minute, so changes made by other processes go out too).
//! - Realtime event → pull.
//! - Catch-up pull on start and every 15 minutes as a fallback.
//! - Offline: failures are retried every minute; local use is unaffected.
//! - Logged out (session file gone): both threads stop.

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use planner_core::Store;

use crate::client::Client;
use crate::config::session_path;
use crate::realtime;

const FALLBACK_PULL: Duration = Duration::from_secs(15 * 60);
const RETRY: Duration = Duration::from_secs(60);
/// Realtime events arrive in bursts; wait this long and pull once.
const DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncEvent {
    /// Something changed locally: push.
    Local,
    /// Something changed on the server: pull.
    Remote,
}

/// Start sync threads if logged in. `on_pulled` runs after a pull changed
/// local rows (the daemon uses it to refresh Waybar).
pub fn start(db_path: PathBuf, on_pulled: impl Fn() + Send + 'static) -> Result<Option<Sender<SyncEvent>>> {
    let Some(client) = Client::load()? else { return Ok(None) };
    eprintln!(
        "plannerd: syncing with {} as {}",
        client.cfg.url,
        client.session.email.as_deref().unwrap_or(&client.session.user_id)
    );
    let store = Store::open(&db_path)?;
    let client = Arc::new(Mutex::new(client));
    let (tx, rx) = mpsc::channel();

    {
        let client = client.clone();
        let tx = tx.clone();
        thread::spawn(move || realtime::run(client, move || {
            let _ = tx.send(SyncEvent::Remote);
        }));
    }

    thread::spawn(move || {
        let mut need_push = true;
        let mut need_pull = true;
        let mut last_pull: Option<Instant> = None;
        let mut last_err = String::new();
        loop {
            if !session_path().exists() {
                eprintln!("plannerd: logged out, sync stopped");
                return;
            }
            let mut failed = false;
            let mut report = |what: &str, r: Result<usize>| -> Option<usize> {
                match r {
                    Ok(n) => {
                        if !last_err.is_empty() {
                            eprintln!("plannerd: sync working again");
                            last_err.clear();
                        }
                        Some(n)
                    }
                    Err(e) => {
                        let msg = format!("{what}: {e:#}");
                        if msg != last_err {
                            eprintln!("plannerd: sync {msg}");
                            last_err = msg;
                        }
                        failed = true;
                        None
                    }
                }
            };

            if need_push {
                let r = client.lock().map_err(|_| anyhow::anyhow!("lock poisoned")).and_then(|mut c| c.push(&store));
                if report("push", r).is_some() {
                    need_push = false;
                }
            }
            if need_pull || last_pull.is_none_or(|t| t.elapsed() >= FALLBACK_PULL) {
                let r = client.lock().map_err(|_| anyhow::anyhow!("lock poisoned")).and_then(|mut c| c.pull(&store));
                if let Some(n) = report("pull", r) {
                    need_pull = false;
                    last_pull = Some(Instant::now());
                    if n > 0 {
                        on_pulled();
                    }
                }
            }

            let wait = if failed {
                RETRY
            } else {
                FALLBACK_PULL.saturating_sub(last_pull.map_or(Duration::ZERO, |t| t.elapsed()))
            };
            match rx.recv_timeout(wait) {
                Ok(ev) => {
                    thread::sleep(DEBOUNCE);
                    for ev in std::iter::once(ev).chain(rx.try_iter()) {
                        match ev {
                            SyncEvent::Local => need_push = true,
                            SyncEvent::Remote => need_pull = true,
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            // Only push when there is something to send.
            if need_push && store.sync_pending_count().unwrap_or(1) == 0 {
                need_push = false;
            }
            if failed {
                need_push |= store.sync_pending_count().unwrap_or(0) > 0;
            }
        }
    });

    Ok(Some(tx))
}

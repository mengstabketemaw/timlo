//! Freedesktop notifications (mako, dunst, swaync).

use std::sync::mpsc::Sender;
use std::thread;

use notify_rust::{Hint, Notification};
use planner_core::reminders::Reminder;
use planner_core::Status;

use crate::daemon::Msg;

const APP: &str = "timlo";

/// Show a reminder. "Did you finish …?" reminders get Done / Skip buttons;
/// the answer goes back to the daemon loop through `tx`.
pub fn reminder(r: &Reminder, sound: bool, tx: Sender<Msg>) {
    let mut n = Notification::new();
    n.appname(APP).summary(&r.summary).body(&r.body).icon("appointment-soon");
    if !sound {
        n.hint(Hint::SuppressSound(true));
    }
    if r.ask {
        n.action("done", "Done").action("skip", "Skip");
    }
    match n.show() {
        Ok(handle) if r.ask => {
            let id = r.block_id.clone();
            thread::spawn(move || {
                handle.wait_for_action(move |action| {
                    let status = match action {
                        "done" => Status::Done,
                        "skip" => Status::Skipped,
                        _ => return,
                    };
                    let _ = tx.send(Msg::SetStatus(id, status));
                })
            });
        }
        Ok(_) => {}
        Err(e) => eprintln!("plannerd: notification failed: {e}"),
    }
}

pub fn plain(summary: &str, body: &str) {
    if let Err(e) = Notification::new().appname(APP).summary(summary).body(body).icon("x-office-calendar").show() {
        eprintln!("plannerd: notification failed: {e}");
    }
}

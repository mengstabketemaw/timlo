//! Which reminders fall inside a window of minutes. Each device works this
//! out from its local copy, so reminders keep working offline.

use crate::model::{DayBlock, Status};
use crate::settings::Settings;
use crate::time::fmt_hm;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReminderKind {
    Start,
    BeforeStart,
    BeforeEnd,
    Unfinished,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reminder {
    pub kind: ReminderKind,
    pub at_min: u16,
    pub block_id: String,
    pub summary: String,
    pub body: String,
    /// Offer Done / Skip buttons.
    pub ask: bool,
}

/// Reminders whose time is in `from..=to` (minutes of the day), in time order.
/// Quiet hours are the caller's job: Waybar still updates while pop-ups stop.
pub fn due(blocks: &[DayBlock], from: u16, to: u16, s: &Settings) -> Vec<Reminder> {
    let mut sorted: Vec<&DayBlock> = blocks.iter().collect();
    sorted.sort_by_key(|b| b.start_min);
    let in_window = |m: u16| from <= m && m <= to;
    let mut out = Vec::new();

    for (i, b) in sorted.iter().enumerate() {
        let next = sorted.get(i + 1);
        let span = format!("{}–{}", fmt_hm(b.start_min), fmt_hm(b.end_min));
        let mut push = |kind, at_min, summary: String, body: String, ask| {
            out.push(Reminder { kind, at_min, block_id: b.id.clone(), summary, body, ask });
        };

        if s.remind_before_start > 0
            && b.status == Status::Pending
            && b.start_min >= s.remind_before_start
            && in_window(b.start_min - s.remind_before_start)
        {
            push(
                ReminderKind::BeforeStart,
                b.start_min - s.remind_before_start,
                format!("{} in {} min", b.title, s.remind_before_start),
                span.clone(),
                false,
            );
        }
        if s.remind_start && b.status == Status::Pending && in_window(b.start_min) {
            let body = match (&b.note, next) {
                (Some(n), _) if !n.trim().is_empty() => n.clone(),
                (_, Some(n)) => format!("Next: {} at {}", n.title, fmt_hm(n.start_min)),
                _ => String::new(),
            };
            push(ReminderKind::Start, b.start_min, format!("Now: {} ({span})", b.title), body, false);
        }
        let len = b.end_min - b.start_min;
        if s.remind_before_end > 0
            && s.remind_before_end < len
            && b.status == Status::Pending
            && in_window(b.end_min - s.remind_before_end)
        {
            let mut summary = format!("{} ends in {} min", b.title, s.remind_before_end);
            if let Some(n) = next {
                summary.push_str(&format!(" — next: {}", n.title));
            }
            push(ReminderKind::BeforeEnd, b.end_min - s.remind_before_end, summary, span.clone(), false);
        }
        if s.remind_unfinished && b.status.is_open() && in_window(b.end_min) {
            push(ReminderKind::Unfinished, b.end_min, format!("Did you finish {}?", b.title), span, true);
        }
    }
    out.sort_by_key(|r| r.at_min);
    out
}

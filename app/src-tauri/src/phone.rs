//! What the Android service shows and when it wakes. Worked out here, from
//! the shared core, so the Kotlin side stays a thin shell (and so it can be
//! tested on the desktop).

use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use planner_core::now::{action_target, now_view};
use planner_core::reminders::{self, ReminderKind};
use planner_core::summary;
use planner_core::time::{fmt_dur, fmt_hm, minute_of};
use planner_core::{DayBlock, Settings, Status, Store};
use serde::Serialize;

/// Last minute reminders were checked for (`local.` = never synced).
const LAST_TICK_KEY: &str = "local.phone.last_tick";
const LAST_TICK_FMT: &str = "%Y-%m-%d %H:%M";

/// The ongoing "now" notification. Shown only while a block is running
/// (`state == "active"`); in a gap the phone shows nothing unless a reminder
/// is up.
#[derive(Debug, Serialize)]
pub struct Card {
    /// "Study · 25m left" or "Free until 14:00".
    pub title: String,
    /// "Next: Gym at 14:00".
    pub text: String,
    /// "07:00–08:00", or a prompt for the block the buttons apply to.
    pub sub: String,
    /// Percent through the current block.
    pub progress: Option<u8>,
    /// Block Done / Skip apply to, when it still has no answer.
    pub target: Option<String>,
    /// `active`, `free`, `empty` or `day_over`.
    pub state: &'static str,
}

/// A one-off notification to post now.
#[derive(Debug, Serialize)]
pub struct Alert {
    /// Notification tag. `ask:<block id>` for "Did you finish …?" prompts,
    /// which are withdrawn once the block has an answer (on any device).
    pub tag: String,
    pub block_id: String,
    /// `start`, `before_start`, `before_end` or `unfinished`.
    pub kind: &'static str,
    pub title: String,
    pub body: String,
    /// Offer Done / Skip.
    pub ask: bool,
    /// Sound off in settings.
    pub silent: bool,
    /// Withdraw the alert after this long, once it is out of date.
    pub timeout_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct Tick {
    pub card: Card,
    pub alerts: Vec<Alert>,
    /// When to wake next (Unix ms): the next minute anything changes.
    pub next_wake_ms: i64,
}

/// Bring today up to date, list reminders due since the last call and say
/// when to call again. Reminders are fired for minutes since the previous
/// tick, at most a few minutes back, so a phone that was off doesn't replay
/// a morning's worth of alerts.
pub fn tick(store: &Store, now: NaiveDateTime) -> Result<Tick> {
    let today = now.date();
    store.ensure_day(today, now)?;
    store.mark_missed(now)?;
    let blocks = store.day_blocks(today)?;
    let settings = Settings::load(store)?;
    let m = minute_of(&now);

    let last = store
        .setting(LAST_TICK_KEY)?
        .and_then(|s| NaiveDateTime::parse_from_str(&s, LAST_TICK_FMT).ok());
    let from = match last {
        Some(l) if l.date() == today && minute_of(&l) <= m && m - minute_of(&l) <= 5 => minute_of(&l) + 1,
        _ => m,
    };
    store.set_setting(LAST_TICK_KEY, &now.format(LAST_TICK_FMT).to_string())?;

    let mut alerts = Vec::new();
    if from <= m && !settings.is_quiet(m) {
        for r in reminders::due(&blocks, from, m, &settings) {
            let block = blocks.iter().find(|b| b.id == r.block_id);
            // Out of date once the block starts / ends.
            let until = block.map(|b| match r.kind {
                ReminderKind::BeforeStart => b.start_min,
                ReminderKind::Start | ReminderKind::BeforeEnd => b.end_min,
                ReminderKind::Unfinished => 24 * 60,
            });
            let timeout_ms = until.filter(|&u| r.kind != ReminderKind::Unfinished && u > m).map(|u| {
                let secs = (u - m) as i64 * 60 - now.second() as i64;
                secs.max(1) * 1000
            });
            let tag = if r.ask { format!("ask:{}", r.block_id) } else { format!("rem:{}:{:?}", r.block_id, r.kind) };
            let kind = match r.kind {
                ReminderKind::Start => "start",
                ReminderKind::BeforeStart => "before_start",
                ReminderKind::BeforeEnd => "before_end",
                ReminderKind::Unfinished => "unfinished",
            };
            alerts.push(Alert {
                tag,
                block_id: r.block_id,
                kind,
                title: r.summary,
                body: r.body,
                ask: r.ask,
                silent: !settings.sound,
                timeout_ms,
            });
        }
    }

    // Sunday evening: how the week went.
    if from <= m
        && !settings.is_quiet(m)
        && today.weekday() == chrono::Weekday::Sun
        && (from..=m).contains(&summary::RECAP_MIN)
    {
        if let Some((title, body)) = summary::recap_text(&summary::week(store, today)?) {
            alerts.push(Alert {
                tag: format!("recap:{today}"),
                block_id: String::new(),
                kind: "recap",
                title,
                body,
                ask: false,
                silent: !settings.sound,
                timeout_ms: Some(30 * 60_000),
            });
        }
    }

    Ok(Tick { card: card(&blocks, m), alerts, next_wake_ms: next_wake(&blocks, &settings, now) })
}

fn card(blocks: &[DayBlock], m: u16) -> Card {
    let v = now_view(blocks, m);
    let target = action_target(blocks, m).filter(|b| b.status.is_open());
    let next = v.next.map(|n| format!("Next: {} at {}", n.title, fmt_hm(n.start_min))).unwrap_or_default();
    let ask = |b: &DayBlock| format!("Did you finish {}?", b.title);
    match v.current {
        Some(c) => {
            let mark = match c.status {
                Status::Done => "✓ ",
                Status::Skipped => "✗ ",
                _ => "",
            };
            Card {
                title: format!("{mark}{} · {} left", c.title, fmt_dur(c.end_min - m)),
                text: next,
                sub: match target {
                    Some(t) if t.id != c.id => ask(t),
                    _ => format!("{}–{}", fmt_hm(c.start_min), fmt_hm(c.end_min)),
                },
                progress: v.progress(),
                target: target.map(|t| t.id.clone()),
                state: "active",
            }
        }
        None => {
            let (title, text, state) = match v.next {
                Some(n) => (format!("Free until {}", fmt_hm(n.start_min)), format!("→ {}", n.title), "free"),
                None if blocks.is_empty() => ("No plan today".into(), String::new(), "empty"),
                None => ("Free".into(), "Nothing else planned today".into(), "day_over"),
            };
            Card {
                title,
                text,
                sub: target.map(ask).unwrap_or_default(),
                progress: None,
                target: target.map(|t| t.id.clone()),
                state,
            }
        }
    }
}

/// The next minute (after now) at which the card or a reminder changes,
/// or just after midnight for the next day.
fn next_wake(blocks: &[DayBlock], s: &Settings, now: NaiveDateTime) -> i64 {
    let m = minute_of(&now);
    let mut points: Vec<u16> = Vec::new();
    for b in blocks {
        points.extend([b.start_min, b.end_min]);
        points.extend(b.start_min.checked_sub(s.remind_before_start).filter(|_| s.remind_before_start > 0));
        points.extend(b.end_min.checked_sub(s.remind_before_end).filter(|_| s.remind_before_end > 0));
    }
    if now.date().weekday() == chrono::Weekday::Sun {
        points.push(summary::RECAP_MIN);
    }
    // Quiet hours ending don't need a wake: nothing fires late.
    let at = match points.into_iter().filter(|&p| p > m && p < 24 * 60).min() {
        Some(p) => now.date().and_hms_opt(0, 0, 0).unwrap() + Duration::minutes(p as i64),
        None => next_midnight(now.date()),
    };
    local_ms(at)
}

fn next_midnight(d: NaiveDate) -> NaiveDateTime {
    d.succ_opt().unwrap_or(d).and_hms_opt(0, 0, 30).unwrap()
}

/// Local wall-clock time → Unix ms (a time skipped by DST moves an hour on).
fn local_ms(t: NaiveDateTime) -> i64 {
    Local
        .from_local_datetime(&t)
        .earliest()
        .or_else(|| Local.from_local_datetime(&(t + Duration::hours(1))).earliest())
        .map_or_else(|| Local::now().timestamp_millis() + 60_000, |d| d.timestamp_millis())
}

/// Done / Skip from a notification. `id: None` = whatever the "now" card's
/// buttons apply to.
pub fn answer(store: &Store, id: Option<&str>, status: Status, now: NaiveDateTime) -> Result<Option<DayBlock>> {
    let id = match id {
        Some(id) => id.to_string(),
        None => {
            store.ensure_day(now.date(), now)?;
            let blocks = store.day_blocks(now.date())?;
            match action_target(&blocks, minute_of(&now)) {
                Some(b) => b.id.clone(),
                None => return Ok(None),
            }
        }
    };
    Ok(Some(store.set_status(&id, status)?))
}

/// Whether a block still waits for Done / Skip (unknown blocks don't).
pub fn block_open(store: &Store, id: &str) -> Result<bool> {
    Ok(store.day_block(id)?.is_some_and(|b| b.status.is_open()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use planner_core::BlockInput;

    fn at(h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 4).unwrap().and_hms_opt(h, mi, 0).unwrap()
    }

    fn blk(start: u16, end: u16, title: &str) -> BlockInput {
        BlockInput {
            id: None,
            start_min: start,
            end_min: end,
            title: title.into(),
            note: None,
            icon: None,
            colour: None,
        }
    }

    fn store() -> Store {
        let s = Store::open_in_memory().unwrap();
        let t = s.create_template("Day", None).unwrap();
        s.set_template_blocks(&t.id, &[blk(9 * 60, 10 * 60, "Study"), blk(11 * 60, 12 * 60, "Gym")], at(0, 0)).unwrap();
        for wd in 1..=7 {
            s.set_week_assignment(wd, Some(&t.id), at(0, 0)).unwrap();
        }
        s
    }

    #[test]
    fn card_and_wake() {
        let s = store();
        let t = tick(&s, at(9, 35)).unwrap();
        assert_eq!(t.card.title, "Study · 25m left");
        assert_eq!(t.card.text, "Next: Gym at 11:00");
        assert_eq!(t.card.progress, Some(58));
        assert!(t.card.target.is_some());
        assert_eq!(t.next_wake_ms, local_ms(at(10, 0)));

        let t = tick(&s, at(10, 30)).unwrap();
        assert_eq!(t.card.title, "Free until 11:00");
        assert_eq!(t.card.sub, "Did you finish Study?");
        assert_eq!(t.next_wake_ms, local_ms(at(10, 55)));

        let t = tick(&s, at(13, 0)).unwrap();
        assert_eq!(t.card.state, "day_over");
        assert_eq!(t.next_wake_ms, local_ms(at(0, 0) + Duration::days(1) + Duration::seconds(30)));
    }

    #[test]
    fn reminders_fire_once_without_replay() {
        let s = store();
        // First tick ever at 08:55: the "Study in 5 min" for this minute fires.
        let t = tick(&s, at(8, 55)).unwrap();
        assert_eq!(t.alerts.len(), 1);
        assert_eq!(t.alerts[0].title, "Study in 5 min");
        assert_eq!(t.alerts[0].timeout_ms, Some(5 * 60_000));
        // Same minute again: nothing new.
        assert!(tick(&s, at(8, 55)).unwrap().alerts.is_empty());
        // Woken at 09:00: the start alert.
        let t = tick(&s, at(9, 0)).unwrap();
        assert_eq!(t.alerts.len(), 1);
        assert!(t.alerts[0].title.starts_with("Now: Study"));
        // Phone off until 10:58: Study's end at 10:00 isn't replayed, but
        // Gym's 10:55 pre-alert is too far back as well.
        assert!(tick(&s, at(10, 58)).unwrap().alerts.is_empty());
    }

    #[test]
    fn answering_withdraws_the_prompt() {
        let s = store();
        tick(&s, at(9, 59)).unwrap();
        let t = tick(&s, at(10, 0)).unwrap();
        let ask = t.alerts.iter().find(|a| a.ask).expect("prompt");
        assert_eq!(ask.title, "Did you finish Study?");
        assert!(block_open(&s, &ask.block_id).unwrap());
        // "Done" on the card in the gap applies to Study.
        let b = answer(&s, None, Status::Done, at(10, 1)).unwrap().unwrap();
        assert_eq!(b.title, "Study");
        assert!(!block_open(&s, &ask.block_id).unwrap());
        assert!(tick(&s, at(10, 2)).unwrap().card.target.is_none());
    }
}

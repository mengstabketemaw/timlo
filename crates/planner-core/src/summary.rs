//! Weekly recap: how a week of plans went, for the Calendar card and the
//! Sunday-evening notification.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::Serialize;

use crate::model::Status;
use crate::store::Store;
use crate::time::fmt_dur;

/// When the recap notification fires: Sunday at this minute of the day.
pub const RECAP_MIN: u16 = 20 * 60;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TitleCount {
    pub title: String,
    pub count: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct WeekSummary {
    /// Monday of the week.
    pub from: NaiveDate,
    /// Sunday of the week.
    pub to: NaiveDate,
    pub planned: u32,
    pub done: u32,
    pub skipped: u32,
    pub missed: u32,
    pub focus_min: u32,
    /// Day with the most blocks done (ties: the higher share).
    pub best_day: Option<NaiveDate>,
    /// Block finished most often.
    pub top_done: Option<TitleCount>,
    /// Block skipped or missed most often (at least twice).
    pub most_dropped: Option<TitleCount>,
    /// Share done the week before, for comparison.
    pub prev_pct: Option<u32>,
}

impl WeekSummary {
    pub fn pct(&self) -> Option<u32> {
        (self.planned > 0).then(|| self.done * 100 / self.planned)
    }
}

/// Monday of the week `d` is in.
pub fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

struct Counts {
    planned: u32,
    done: u32,
    skipped: u32,
    missed: u32,
}

/// Totals, (date, done, planned) per day, and every block's (title, status).
type WeekCounts = (Counts, Vec<(NaiveDate, u32, u32)>, Vec<(String, Status)>);

fn counts(store: &Store, from: NaiveDate, to: NaiveDate) -> Result<WeekCounts> {
    let mut c = Counts { planned: 0, done: 0, skipped: 0, missed: 0 };
    let mut per_day = Vec::new();
    let mut blocks_all = Vec::new();
    for day in store.days_in_range(from, to)? {
        let blocks = store.day_blocks(day.date)?;
        let done = blocks.iter().filter(|b| b.status == Status::Done).count() as u32;
        c.planned += blocks.len() as u32;
        c.done += done;
        c.skipped += blocks.iter().filter(|b| b.status == Status::Skipped).count() as u32;
        c.missed += blocks.iter().filter(|b| b.status == Status::Missed).count() as u32;
        per_day.push((day.date, done, blocks.len() as u32));
        blocks_all.extend(blocks.into_iter().map(|b| (b.title, b.status)));
    }
    Ok((c, per_day, blocks_all))
}

fn top(map: HashMap<String, u32>, min: u32) -> Option<TitleCount> {
    map.into_iter()
        .filter(|(_, n)| *n >= min)
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(title, count)| TitleCount { title, count })
}

/// Summary of the Monday–Sunday week starting `monday`.
pub fn week(store: &Store, monday: NaiveDate) -> Result<WeekSummary> {
    let monday = week_start(monday);
    let sunday = monday + Duration::days(6);
    let (c, per_day, blocks) = counts(store, monday, sunday)?;

    let best_day = per_day
        .iter()
        .filter(|(_, done, _)| *done > 0)
        .max_by(|a, b| {
            a.1.cmp(&b.1).then_with(|| (a.1 as u64 * b.2 as u64).cmp(&(b.1 as u64 * a.2 as u64)))
        })
        .map(|(d, _, _)| *d);

    let mut done_by: HashMap<String, u32> = HashMap::new();
    let mut dropped_by: HashMap<String, u32> = HashMap::new();
    for (title, status) in blocks {
        match status {
            Status::Done => *done_by.entry(title).or_default() += 1,
            Status::Skipped | Status::Missed => *dropped_by.entry(title).or_default() += 1,
            Status::Pending => {}
        }
    }

    let (prev, _, _) = counts(store, monday - Duration::days(7), monday - Duration::days(1))?;
    let focus_min = store.focus_minutes(monday, sunday)?.values().sum();

    Ok(WeekSummary {
        from: monday,
        to: sunday,
        planned: c.planned,
        done: c.done,
        skipped: c.skipped,
        missed: c.missed,
        focus_min,
        best_day,
        top_done: top(done_by, 1),
        most_dropped: top(dropped_by, 2),
        prev_pct: (prev.planned > 0).then(|| prev.done * 100 / prev.planned),
    })
}

fn weekday_name(d: NaiveDate) -> &'static str {
    match d.weekday() {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

/// Notification title and body, or `None` for a week with nothing in it.
pub fn recap_text(s: &WeekSummary) -> Option<(String, String)> {
    if s.planned == 0 && s.focus_min == 0 {
        return None;
    }
    let mut title = format!("Your week: {}/{} blocks done", s.done, s.planned);
    if let Some(p) = s.pct() {
        title.push_str(&format!(" ({p}%)"));
    }
    let mut lines = Vec::new();
    if let (Some(p), Some(prev)) = (s.pct(), s.prev_pct) {
        let diff = p as i64 - prev as i64;
        lines.push(match diff {
            0 => "Same as last week".to_string(),
            d if d > 0 => format!("Up {d} points on last week"),
            d => format!("Down {} points on last week", -d),
        });
    }
    if s.focus_min > 0 {
        lines.push(format!("{} of focus", fmt_dur(s.focus_min.min(u16::MAX as u32) as u16)));
    }
    if let Some(d) = s.best_day {
        lines.push(format!("Best day: {}", weekday_name(d)));
    }
    if let Some(t) = &s.most_dropped {
        lines.push(format!("{} slipped {} times", t.title, t.count));
    }
    Some((title, lines.join(" · ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BlockInput;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn week_counts_and_text() {
        let s = Store::open_in_memory().unwrap();
        // Week of Mon 2026-11-02; "now" is the Sunday evening.
        let now = d("2026-11-08").and_hms_opt(20, 0, 0).unwrap();
        let block = |t: &str, start: u16| BlockInput {
            id: None,
            start_min: start,
            end_min: start + 60,
            title: t.into(),
            note: None,
            icon: None,
            colour: None,
        };
        for (day, statuses) in [("2026-11-03", [Status::Done, Status::Missed]), ("2026-11-05", [Status::Done, Status::Skipped])] {
            let a = s.add_day_block(d(day), block("Study", 600), now).unwrap();
            let b = s.add_day_block(d(day), block("Gym", 900), now).unwrap();
            s.set_status(&a.id, statuses[0]).unwrap();
            s.set_status(&b.id, statuses[1]).unwrap();
        }
        let w = week(&s, d("2026-11-04")).unwrap();
        assert_eq!((w.from, w.to), (d("2026-11-02"), d("2026-11-08")));
        assert_eq!((w.planned, w.done, w.skipped, w.missed), (4, 2, 1, 1));
        assert_eq!(w.top_done.as_ref().unwrap().title, "Study");
        assert_eq!(w.most_dropped.as_ref().unwrap().title, "Gym");
        let (title, body) = recap_text(&w).unwrap();
        assert_eq!(title, "Your week: 2/4 blocks done (50%)");
        assert!(body.contains("Gym slipped 2 times"), "{body}");
        assert!(recap_text(&week(&s, d("2026-12-07")).unwrap()).is_none());
    }
}

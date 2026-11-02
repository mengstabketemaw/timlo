//! Waybar custom-module output (`return-type: json`).

use planner_core::now::now_view;
use planner_core::time::{fmt_dur, fmt_hm};
use planner_core::{DayBlock, Settings, Status};
use serde::Serialize;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Output {
    pub text: String,
    pub tooltip: String,
    /// CSS classes: `active`, `ending-soon`, `free`, `day-over`, `done`,
    /// `skipped`, `empty`, `offline`.
    pub class: Vec<&'static str>,
    /// Progress through the current block.
    pub percentage: u8,
}

/// Waybar text and tooltips are Pango markup.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn offline() -> Output {
    Output {
        text: "planner offline".into(),
        tooltip: "plannerd isn't running.\nStart it with: systemctl --user start plannerd".into(),
        class: vec!["offline"],
        percentage: 0,
    }
}

pub fn render(blocks: &[DayBlock], now_min: u16, s: &Settings) -> Output {
    let v = now_view(blocks, now_min);
    let mut class = Vec::new();
    let text = if let Some(c) = v.current {
        let left = c.end_min - now_min;
        class.push("active");
        if left <= s.ending_soon {
            class.push("ending-soon");
        }
        let mut t = format!("{} · {} left", c.title, fmt_dur(left));
        if let Some(n) = v.next {
            t.push_str(&format!(" → {}", n.title));
        }
        match c.status {
            Status::Done => {
                class.push("done");
                t = format!("✓ {t}");
            }
            Status::Skipped => {
                class.push("skipped");
                t = format!("✗ {t}");
            }
            _ => {}
        }
        t
    } else if let Some(n) = v.next {
        class.push("free");
        format!("Free until {} → {}", fmt_hm(n.start_min), n.title)
    } else if blocks.is_empty() {
        class.push("empty");
        "No plan today".into()
    } else {
        class.push("free");
        class.push("day-over");
        "Free".into()
    };
    Output {
        text: escape(&text),
        tooltip: tooltip(blocks, now_min),
        class,
        percentage: v.progress().unwrap_or(0),
    }
}

/// The rest of today, one block per line.
fn tooltip(blocks: &[DayBlock], now_min: u16) -> String {
    let rest: Vec<&DayBlock> = blocks.iter().filter(|b| b.end_min > now_min).collect();
    if rest.is_empty() {
        return "Nothing else planned today".into();
    }
    let mut lines = vec!["Rest of today".to_string()];
    for b in rest {
        let mark = match b.status {
            Status::Done => "  ✓",
            Status::Skipped => "  ✗",
            _ => "",
        };
        let now = if b.start_min <= now_min { "  ◂ now" } else { "" };
        lines.push(format!(
            "{}–{}  {}{mark}{now}",
            fmt_hm(b.start_min),
            fmt_hm(b.end_min),
            escape(&b.title)
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn block(id: &str, s: u16, e: u16, title: &str, status: Status) -> DayBlock {
        DayBlock {
            id: id.into(),
            date: NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
            template_block_id: None,
            start_min: s,
            end_min: e,
            title: title.into(),
            note: None,
            status,
            status_at: None,
        }
    }

    fn day() -> Vec<DayBlock> {
        vec![
            block("a", 420, 480, "Study", Status::Done),
            block("b", 495, 540, "R&D", Status::Pending),
            block("c", 840, 900, "Gym", Status::Pending),
        ]
    }

    #[test]
    fn current_block() {
        let out = render(&day(), 455, &Settings::default());
        assert_eq!(out.text, "✓ Study · 25m left → R&amp;D");
        assert_eq!(out.class, ["active", "done"]);
        assert_eq!(out.percentage, 58);
        assert!(out.tooltip.contains("07:00–08:00  Study  ✓  ◂ now"));
    }

    #[test]
    fn ending_soon_and_gap() {
        let out = render(&day(), 537, &Settings::default());
        assert_eq!(out.class, ["active", "ending-soon"]);
        let out = render(&day(), 600, &Settings::default());
        assert_eq!(out.text, "Free until 14:00 → Gym");
        assert_eq!(out.class, ["free"]);
    }

    #[test]
    fn end_of_day_and_empty() {
        let out = render(&day(), 1000, &Settings::default());
        assert_eq!(out.text, "Free");
        assert_eq!(out.tooltip, "Nothing else planned today");
        assert_eq!(render(&[], 600, &Settings::default()).text, "No plan today");
    }
}

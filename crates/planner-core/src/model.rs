use anyhow::{bail, Result};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::time::{fmt_hm, MINUTES_PER_DAY};

/// Status of a block on a specific day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Done,
    Skipped,
    /// Left untouched after the block ended.
    Missed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Done => "done",
            Status::Skipped => "skipped",
            Status::Missed => "missed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => Status::Pending,
            "done" => Status::Done,
            "skipped" => Status::Skipped,
            "missed" => Status::Missed,
            _ => return None,
        })
    }

    /// Still waiting for a Done/Skip answer.
    pub fn is_open(self) -> bool {
        matches!(self, Status::Pending | Status::Missed)
    }
}

/// A named, reusable list of blocks covering a day.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub colour: Option<String>,
}

/// One activity in a template. Times are minutes from midnight.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TemplateBlock {
    pub id: String,
    pub template_id: String,
    pub start_min: u16,
    pub end_min: u16,
    pub title: String,
    pub note: Option<String>,
    pub icon: Option<String>,
    pub colour: Option<String>,
}

/// A block as written by a caller. `id: None` creates a new block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockInput {
    pub id: Option<String>,
    pub start_min: u16,
    pub end_min: u16,
    pub title: String,
    pub note: Option<String>,
    pub icon: Option<String>,
    pub colour: Option<String>,
}

impl BlockInput {
    pub fn new(start_min: u16, end_min: u16, title: impl Into<String>) -> Self {
        Self { start_min, end_min, title: title.into(), ..Default::default() }
    }
}

impl From<&TemplateBlock> for BlockInput {
    fn from(b: &TemplateBlock) -> Self {
        Self {
            id: Some(b.id.clone()),
            start_min: b.start_min,
            end_min: b.end_min,
            title: b.title.clone(),
            note: b.note.clone(),
            icon: b.icon.clone(),
            colour: b.colour.clone(),
        }
    }
}

/// A specific date and the template it runs from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Day {
    pub date: NaiveDate,
    pub template_id: Option<String>,
    /// Blocks were edited by hand; template edits no longer reach this day.
    pub customised: bool,
    /// Switched away from the weekly default (a day override).
    pub overridden: bool,
}

/// The actual plan for one block on one date.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DayBlock {
    pub id: String,
    pub date: NaiveDate,
    pub template_block_id: Option<String>,
    pub start_min: u16,
    pub end_min: u16,
    pub title: String,
    pub note: Option<String>,
    pub status: Status,
    /// When the status was last set (ms since the Unix epoch).
    pub status_at: Option<i64>,
}

/// Check a set of `(start, end, title)` spans: non-empty titles, start < end,
/// within one day, and no overlaps.
pub fn validate_blocks<'a>(spans: impl IntoIterator<Item = (u16, u16, &'a str)>) -> Result<()> {
    let mut v: Vec<_> = spans.into_iter().collect();
    for &(s, e, t) in &v {
        if t.trim().is_empty() {
            bail!("block title can't be empty");
        }
        if s >= e {
            bail!("{t}: start must be before end ({}–{})", fmt_hm(s), fmt_hm(e));
        }
        if e > MINUTES_PER_DAY {
            bail!("{t}: blocks must end by 24:00 (crossing midnight isn't supported yet)");
        }
    }
    v.sort_by_key(|x| x.0);
    for w in v.windows(2) {
        let ((s1, e1, t1), (s2, e2, t2)) = (w[0], w[1]);
        if s2 < e1 {
            bail!(
                "{t2} ({}–{}) overlaps {t1} ({}–{})",
                fmt_hm(s2),
                fmt_hm(e2),
                fmt_hm(s1),
                fmt_hm(e1)
            );
        }
    }
    Ok(())
}

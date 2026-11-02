//! The hand-edited plan file (TOML). Until the planner UI exists this is how
//! templates, the weekly assignment and settings are written.
//!
//! ```toml
//! [settings]
//! quiet_hours = "23:00-07:00"
//!
//! [week]
//! "mon-fri" = "Workday"
//! "sat,sun" = "Weekend"
//!
//! [[templates]]
//! name = "Workday"
//! blocks = [
//!   { start = "07:00", end = "08:00", title = "Study" },
//! ]
//! ```

use std::collections::{BTreeMap, HashSet};

use anyhow::{bail, Context, Result};
use chrono::NaiveDateTime;
use serde::Deserialize;

use crate::model::{validate_blocks, BlockInput};
use crate::settings::{parse_quiet_hours, Settings};
use crate::store::Store;
use crate::time::parse_hm;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanFile {
    #[serde(default)]
    pub settings: SettingsSection,
    /// Weekday key (`mon`, `mon-fri`, `sat,sun`) → template name.
    #[serde(default)]
    pub week: BTreeMap<String, String>,
    #[serde(default)]
    pub templates: Vec<TemplateSection>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsSection {
    pub remind_start: Option<bool>,
    pub remind_before_start: Option<u16>,
    pub remind_before_end: Option<u16>,
    pub remind_unfinished: Option<bool>,
    pub ending_soon: Option<u16>,
    pub quiet_hours: Option<String>,
    pub sound: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateSection {
    pub name: String,
    pub colour: Option<String>,
    #[serde(default)]
    pub blocks: Vec<BlockSection>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockSection {
    pub start: String,
    pub end: String,
    pub title: String,
    pub note: Option<String>,
    pub icon: Option<String>,
    pub colour: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub templates: usize,
    pub blocks: usize,
}

pub fn parse(text: &str) -> Result<PlanFile> {
    Ok(toml::from_str(text)?)
}

fn weekday(s: &str) -> Result<u8> {
    Ok(match s.trim().to_ascii_lowercase().as_str() {
        "mon" | "monday" => 1,
        "tue" | "tuesday" => 2,
        "wed" | "wednesday" => 3,
        "thu" | "thursday" => 4,
        "fri" | "friday" => 5,
        "sat" | "saturday" => 6,
        "sun" | "sunday" => 7,
        other => bail!("unknown weekday {other:?}"),
    })
}

/// `"mon-fri"` → `[1..=5]`, `"sat,sun"` → `[6, 7]`.
pub fn parse_weekdays(key: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for part in key.split(',') {
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (weekday(a)?, weekday(b)?);
                if a > b {
                    bail!("weekday range {part:?} runs backwards");
                }
                out.extend(a..=b);
            }
            None => out.push(weekday(part)?),
        }
    }
    Ok(out)
}

/// Write the plan file into the store in one transaction. Templates are
/// matched by name; blocks keep their ids when start, end and title match
/// (or, failing that, the title alone), so today's Done/Skip records survive
/// a re-import. Templates missing from the file are left alone.
pub fn import(store: &Store, plan: &PlanFile, now: NaiveDateTime) -> Result<ImportReport> {
    // Validate everything before writing anything.
    let mut names = HashSet::new();
    let mut parsed = Vec::new();
    for t in &plan.templates {
        if !names.insert(t.name.trim().to_lowercase()) {
            bail!("template {:?} is defined twice", t.name);
        }
        let blocks = t
            .blocks
            .iter()
            .map(|b| {
                Ok(BlockInput {
                    id: None,
                    start_min: parse_hm(&b.start)?,
                    end_min: parse_hm(&b.end)?,
                    title: b.title.trim().to_string(),
                    note: b.note.clone(),
                    icon: b.icon.clone(),
                    colour: b.colour.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("template {:?}", t.name))?;
        validate_blocks(blocks.iter().map(|b| (b.start_min, b.end_min, b.title.as_str())))
            .with_context(|| format!("template {:?}", t.name))?;
        parsed.push((t, blocks));
    }

    let mut week: BTreeMap<u8, &str> = BTreeMap::new();
    for (key, name) in &plan.week {
        for d in parse_weekdays(key).with_context(|| format!("[week] key {key:?}"))? {
            if week.insert(d, name.as_str()).is_some() {
                bail!("[week] assigns weekday {d} more than once");
            }
        }
    }

    let quiet = match &plan.settings.quiet_hours {
        Some(q) => Some(parse_quiet_hours(q).context("[settings] quiet_hours")?),
        None => None,
    };

    store.with_tx(|s| {
        let mut report = ImportReport::default();
        for (t, mut blocks) in parsed {
            let tpl = match s.template_by_name(&t.name)? {
                Some(x) => x,
                None => s.create_template(&t.name, t.colour.as_deref())?,
            };
            if tpl.colour != t.colour {
                s.set_template_colour(&tpl.id, t.colour.as_deref())?;
            }
            let existing = s.template_blocks(&tpl.id)?;
            let mut used = HashSet::new();
            for b in blocks.iter_mut() {
                if let Some(e) = existing.iter().find(|e| {
                    !used.contains(&e.id)
                        && e.start_min == b.start_min
                        && e.end_min == b.end_min
                        && e.title == b.title
                }) {
                    used.insert(e.id.clone());
                    b.id = Some(e.id.clone());
                }
            }
            for b in blocks.iter_mut().filter(|b| b.id.is_none()) {
                if let Some(e) = existing.iter().find(|e| !used.contains(&e.id) && e.title == b.title) {
                    used.insert(e.id.clone());
                    b.id = Some(e.id.clone());
                }
            }
            report.templates += 1;
            report.blocks += blocks.len();
            s.set_template_blocks_in(&tpl.id, &blocks, now)?;
        }

        if !plan.week.is_empty() {
            let current = s.week_assignment()?;
            for wd in 1..=7u8 {
                let tid = match week.get(&wd) {
                    Some(name) => Some(
                        s.template_by_name(name)?
                            .with_context(|| format!("[week] refers to unknown template {name:?}"))?
                            .id,
                    ),
                    None => None,
                };
                if current[(wd - 1) as usize] != tid {
                    s.set_week_assignment_in(wd, tid.as_deref(), now)?;
                }
            }
        }

        let mut settings = Settings::load(s)?;
        let sec = &plan.settings;
        if let Some(v) = sec.remind_start {
            settings.remind_start = v;
        }
        if let Some(v) = sec.remind_before_start {
            settings.remind_before_start = v;
        }
        if let Some(v) = sec.remind_before_end {
            settings.remind_before_end = v;
        }
        if let Some(v) = sec.remind_unfinished {
            settings.remind_unfinished = v;
        }
        if let Some(v) = sec.ending_soon {
            settings.ending_soon = v;
        }
        if let Some(q) = quiet {
            settings.quiet_hours = q;
        }
        if let Some(v) = sec.sound {
            settings.sound = v;
        }
        settings.save(s)?;
        Ok(report)
    })
}

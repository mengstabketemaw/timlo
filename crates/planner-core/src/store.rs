//! SQLite storage and the template → day rules.
//!
//! Rules (from the spec):
//! 1. A day's plan is a copy of its template; editing a template never
//!    rewrites past days.
//! 2. Editing a template updates today (from now on) and future days that
//!    haven't been edited by hand.
//! 3. Switching today's template mid-day keeps blocks that already ended
//!    (with their Done/Skip records) and replaces the rest of the day.
//! 4. A single day's blocks can be edited without touching the template;
//!    the day is then `customised`.
//! 5. Blocks within one template (or one day) may not overlap.
//!
//! Every row has `updated_at` (ms since epoch) and `deleted` (soft delete)
//! so it can be synced later with last-write-wins.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::model::{validate_blocks, BlockInput, Day, DayBlock, Status, Template, TemplateBlock};
use crate::time::minute_of;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS templates (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    colour      TEXT,
    updated_at  INTEGER NOT NULL,
    deleted     INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS template_blocks (
    id          TEXT PRIMARY KEY,
    template_id TEXT NOT NULL,
    start_min   INTEGER NOT NULL,
    end_min     INTEGER NOT NULL,
    title       TEXT NOT NULL,
    note        TEXT,
    icon        TEXT,
    colour      TEXT,
    updated_at  INTEGER NOT NULL,
    deleted     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS template_blocks_template ON template_blocks (template_id);
CREATE TABLE IF NOT EXISTS week_assignment (
    weekday     INTEGER PRIMARY KEY,   -- 1 = Monday … 7 = Sunday
    template_id TEXT,
    updated_at  INTEGER NOT NULL,
    deleted     INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS days (
    date        TEXT PRIMARY KEY,      -- YYYY-MM-DD
    template_id TEXT,
    customised  INTEGER NOT NULL DEFAULT 0,
    overridden  INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL,
    deleted     INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS day_blocks (
    id                TEXT PRIMARY KEY,  -- "<date>#<template_block_id>" when copied from a template
    date              TEXT NOT NULL,
    template_block_id TEXT,
    start_min         INTEGER NOT NULL,
    end_min           INTEGER NOT NULL,
    title             TEXT NOT NULL,
    note              TEXT,
    status            TEXT NOT NULL DEFAULT 'pending',
    status_at         INTEGER,
    updated_at        INTEGER NOT NULL,
    deleted           INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS day_blocks_date ON day_blocks (date);
CREATE TABLE IF NOT EXISTS settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  INTEGER NOT NULL,
    deleted     INTEGER NOT NULL DEFAULT 0
);
"#;

/// Copy a template block into a day. Re-copying keeps the Done/Skip status,
/// except that a revived (soft-deleted) or `missed` block starts over as
/// pending: it only gets here when it ends after the cutoff.
const UPSERT_DAY_BLOCK: &str = r#"
INSERT INTO day_blocks (id, date, template_block_id, start_min, end_min, title, note,
                        status, status_at, updated_at, deleted)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', NULL, ?8, 0)
ON CONFLICT(id) DO UPDATE SET
    status    = CASE WHEN day_blocks.deleted = 1 OR day_blocks.status = 'missed'
                     THEN 'pending' ELSE day_blocks.status END,
    status_at = CASE WHEN day_blocks.deleted = 1 OR day_blocks.status = 'missed'
                     THEN NULL ELSE day_blocks.status_at END,
    date = excluded.date,
    template_block_id = excluded.template_block_id,
    start_min = excluded.start_min,
    end_min = excluded.end_min,
    title = excluded.title,
    note = excluded.note,
    updated_at = excluded.updated_at,
    deleted = 0
"#;

const TB_COLS: &str = "id, template_id, start_min, end_min, title, note, icon, colour";
const DB_COLS: &str = "id, date, template_block_id, start_min, end_min, title, note, status, status_at";

pub struct Store {
    conn: Connection,
}

fn stamp() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn date_key(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn parse_date_key(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").with_context(|| format!("bad date {s:?} in database"))
}

/// Weekday as stored in `week_assignment`: 1 = Monday … 7 = Sunday.
pub fn weekday_num(d: NaiveDate) -> u8 {
    d.weekday().number_from_monday() as u8
}

/// Fixed id of a day block copied from a template block. Two devices that
/// create the same day offline produce identical rows instead of duplicates.
pub fn day_block_id(date: NaiveDate, template_block_id: &str) -> String {
    format!("{}#{}", date_key(date), template_block_id)
}

fn conv_err(msg: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(std::io::Error::other(msg)))
}

fn template_row(r: &Row) -> rusqlite::Result<Template> {
    Ok(Template { id: r.get(0)?, name: r.get(1)?, colour: r.get(2)? })
}

fn tblock_row(r: &Row) -> rusqlite::Result<TemplateBlock> {
    Ok(TemplateBlock {
        id: r.get(0)?,
        template_id: r.get(1)?,
        start_min: r.get(2)?,
        end_min: r.get(3)?,
        title: r.get(4)?,
        note: r.get(5)?,
        icon: r.get(6)?,
        colour: r.get(7)?,
    })
}

fn dblock_row(r: &Row) -> rusqlite::Result<DayBlock> {
    let date: String = r.get(1)?;
    let status: String = r.get(7)?;
    Ok(DayBlock {
        id: r.get(0)?,
        date: NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| conv_err(e.to_string()))?,
        template_block_id: r.get(2)?,
        start_min: r.get(3)?,
        end_min: r.get(4)?,
        title: r.get(5)?,
        note: r.get(6)?,
        status: Status::parse(&status).ok_or_else(|| conv_err(format!("bad status {status:?}")))?,
        status_at: r.get(8)?,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        // The daemon and CLI commands open the database at the same time.
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(SCHEMA).context("creating schema")?;
        Ok(Self { conn })
    }

    /// Run `f` in one transaction. Methods ending in `_in` and the plain
    /// read/insert helpers may be called inside; public methods that open
    /// their own transaction may not.
    pub fn with_tx<T>(&self, f: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        let tx = self.conn.unchecked_transaction()?;
        let out = f(self)?;
        tx.commit()?;
        Ok(out)
    }

    // ---------------------------------------------------------------- templates

    pub fn templates(&self) -> Result<Vec<Template>> {
        let mut st = self.conn.prepare(
            "SELECT id, name, colour FROM templates WHERE deleted = 0 ORDER BY name COLLATE NOCASE",
        )?;
        let v = st.query_map([], template_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn template(&self, id: &str) -> Result<Option<Template>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, colour FROM templates WHERE id = ?1 AND deleted = 0",
                [id],
                template_row,
            )
            .optional()?)
    }

    pub fn template_by_name(&self, name: &str) -> Result<Option<Template>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, colour FROM templates
                 WHERE name = ?1 COLLATE NOCASE AND deleted = 0",
                [name.trim()],
                template_row,
            )
            .optional()?)
    }

    pub fn create_template(&self, name: &str, colour: Option<&str>) -> Result<Template> {
        let name = name.trim();
        if name.is_empty() {
            bail!("template name can't be empty");
        }
        if self.template_by_name(name)?.is_some() {
            bail!("a template named {name:?} already exists");
        }
        let t = Template { id: new_id(), name: name.to_string(), colour: colour.map(str::to_string) };
        self.conn.execute(
            "INSERT INTO templates (id, name, colour, updated_at, deleted) VALUES (?1, ?2, ?3, ?4, 0)",
            params![t.id, t.name, t.colour, stamp()],
        )?;
        Ok(t)
    }

    pub fn rename_template(&self, id: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("template name can't be empty");
        }
        if let Some(other) = self.template_by_name(name)? {
            if other.id != id {
                bail!("a template named {name:?} already exists");
            }
        }
        let n = self.conn.execute(
            "UPDATE templates SET name = ?2, updated_at = ?3 WHERE id = ?1 AND deleted = 0",
            params![id, name, stamp()],
        )?;
        if n == 0 {
            bail!("no such template");
        }
        Ok(())
    }

    pub fn set_template_colour(&self, id: &str, colour: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE templates SET colour = ?2, updated_at = ?3 WHERE id = ?1 AND deleted = 0",
            params![id, colour, stamp()],
        )?;
        Ok(())
    }

    pub fn duplicate_template(&self, id: &str, new_name: &str) -> Result<Template> {
        self.with_tx(|s| {
            let src = s.template(id)?.context("no such template")?;
            let t = s.create_template(new_name, src.colour.as_deref())?;
            let ts = stamp();
            for b in s.template_blocks(id)? {
                s.conn.execute(
                    &format!(
                        "INSERT INTO template_blocks ({TB_COLS}, updated_at, deleted)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)"
                    ),
                    params![new_id(), t.id, b.start_min, b.end_min, b.title, b.note, b.icon, b.colour, ts],
                )?;
            }
            Ok(t)
        })
    }

    /// Soft-delete a template. Days already copied from it keep their plan;
    /// weekdays that used it get no default.
    pub fn delete_template(&self, id: &str) -> Result<()> {
        self.with_tx(|s| {
            let ts = stamp();
            let n = s.conn.execute(
                "UPDATE templates SET deleted = 1, updated_at = ?2 WHERE id = ?1 AND deleted = 0",
                params![id, ts],
            )?;
            if n == 0 {
                bail!("no such template");
            }
            s.conn.execute(
                "UPDATE template_blocks SET deleted = 1, updated_at = ?2 WHERE template_id = ?1 AND deleted = 0",
                params![id, ts],
            )?;
            s.conn.execute(
                "UPDATE week_assignment SET template_id = NULL, updated_at = ?2 WHERE template_id = ?1",
                params![id, ts],
            )?;
            Ok(())
        })
    }

    pub fn template_blocks(&self, template_id: &str) -> Result<Vec<TemplateBlock>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {TB_COLS} FROM template_blocks
             WHERE template_id = ?1 AND deleted = 0 ORDER BY start_min"
        ))?;
        let v = st.query_map([template_id], tblock_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// Replace a template's whole block list. Blocks with an `id` are updated
    /// in place (so today's Done/Skip records follow them); blocks left out
    /// are deleted. Then today (from `now`) and future days that follow this
    /// template and weren't edited by hand are updated.
    pub fn set_template_blocks(
        &self,
        template_id: &str,
        blocks: &[BlockInput],
        now: NaiveDateTime,
    ) -> Result<Vec<TemplateBlock>> {
        self.with_tx(|s| s.set_template_blocks_in(template_id, blocks, now))
    }

    pub(crate) fn set_template_blocks_in(
        &self,
        template_id: &str,
        blocks: &[BlockInput],
        now: NaiveDateTime,
    ) -> Result<Vec<TemplateBlock>> {
        if self.template(template_id)?.is_none() {
            bail!("no such template");
        }
        validate_blocks(blocks.iter().map(|b| (b.start_min, b.end_min, b.title.as_str())))?;
        let ts = stamp();
        let mut keep = HashSet::new();
        for b in blocks {
            let id = b.id.clone().unwrap_or_else(new_id);
            self.conn.execute(
                &format!(
                    "INSERT INTO template_blocks ({TB_COLS}, updated_at, deleted)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)
                     ON CONFLICT(id) DO UPDATE SET
                        template_id = excluded.template_id, start_min = excluded.start_min,
                        end_min = excluded.end_min, title = excluded.title, note = excluded.note,
                        icon = excluded.icon, colour = excluded.colour,
                        updated_at = excluded.updated_at, deleted = 0"
                ),
                params![id, template_id, b.start_min, b.end_min, b.title.trim(), b.note, b.icon, b.colour, ts],
            )?;
            keep.insert(id);
        }
        for old in self.template_blocks(template_id)? {
            if !keep.contains(&old.id) {
                self.conn.execute(
                    "UPDATE template_blocks SET deleted = 1, updated_at = ?2 WHERE id = ?1",
                    params![old.id, ts],
                )?;
            }
        }
        self.propagate_template(template_id, now)?;
        self.template_blocks(template_id)
    }

    pub fn add_template_block(
        &self,
        template_id: &str,
        block: BlockInput,
        now: NaiveDateTime,
    ) -> Result<TemplateBlock> {
        let mut all: Vec<BlockInput> =
            self.template_blocks(template_id)?.iter().map(BlockInput::from).collect();
        let id = new_id();
        all.push(BlockInput { id: Some(id.clone()), ..block });
        let out = self.set_template_blocks(template_id, &all, now)?;
        out.into_iter().find(|b| b.id == id).context("new block missing after save")
    }

    pub fn update_template_block(&self, block: &TemplateBlock, now: NaiveDateTime) -> Result<()> {
        let mut all: Vec<BlockInput> =
            self.template_blocks(&block.template_id)?.iter().map(BlockInput::from).collect();
        let slot = all
            .iter_mut()
            .find(|b| b.id.as_deref() == Some(block.id.as_str()))
            .context("no such block")?;
        *slot = BlockInput::from(block);
        self.set_template_blocks(&block.template_id, &all, now)?;
        Ok(())
    }

    pub fn delete_template_block(&self, id: &str, now: NaiveDateTime) -> Result<()> {
        let tid: String = self
            .conn
            .query_row(
                "SELECT template_id FROM template_blocks WHERE id = ?1 AND deleted = 0",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .context("no such block")?;
        let all: Vec<BlockInput> = self
            .template_blocks(&tid)?
            .iter()
            .filter(|b| b.id != id)
            .map(BlockInput::from)
            .collect();
        self.set_template_blocks(&tid, &all, now)?;
        Ok(())
    }

    /// Rule 2: re-copy a template into today (from now on) and future days
    /// that follow it and weren't edited by hand.
    fn propagate_template(&self, template_id: &str, now: NaiveDateTime) -> Result<()> {
        let today = now.date();
        let dates: Vec<String> = {
            let mut st = self.conn.prepare(
                "SELECT date FROM days
                 WHERE template_id = ?1 AND customised = 0 AND deleted = 0 AND date >= ?2",
            )?;
            let v = st
                .query_map(params![template_id, date_key(today)], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        for d in dates {
            let date = parse_date_key(&d)?;
            let cutoff = if date == today { minute_of(&now) } else { 0 };
            self.rebuild_day(date, Some(template_id), cutoff)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ week

    /// Default template id per weekday; index 0 = Monday.
    pub fn week_assignment(&self) -> Result<[Option<String>; 7]> {
        let mut out: [Option<String>; 7] = Default::default();
        let mut st = self
            .conn
            .prepare("SELECT weekday, template_id FROM week_assignment WHERE deleted = 0")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, u8>(0)?, r.get::<_, Option<String>>(1)?)))?;
        for row in rows {
            let (wd, tid) = row?;
            if (1..=7).contains(&wd) {
                out[(wd - 1) as usize] = tid;
            }
        }
        Ok(out)
    }

    /// Set the default template for a weekday (1 = Monday … 7 = Sunday).
    /// Future days of that weekday that still follow the default, and weren't
    /// edited by hand, switch to it. Today is left alone.
    pub fn set_week_assignment(
        &self,
        weekday: u8,
        template_id: Option<&str>,
        now: NaiveDateTime,
    ) -> Result<()> {
        self.with_tx(|s| s.set_week_assignment_in(weekday, template_id, now))
    }

    pub(crate) fn set_week_assignment_in(
        &self,
        weekday: u8,
        template_id: Option<&str>,
        now: NaiveDateTime,
    ) -> Result<()> {
        if !(1..=7).contains(&weekday) {
            bail!("weekday must be 1 (Monday) to 7 (Sunday)");
        }
        if let Some(t) = template_id {
            if self.template(t)?.is_none() {
                bail!("no such template");
            }
        }
        let ts = stamp();
        self.conn.execute(
            "INSERT INTO week_assignment (weekday, template_id, updated_at, deleted) VALUES (?1, ?2, ?3, 0)
             ON CONFLICT(weekday) DO UPDATE SET
                template_id = excluded.template_id, updated_at = excluded.updated_at, deleted = 0",
            params![weekday, template_id, ts],
        )?;
        let days: Vec<(String, Option<String>)> = {
            let mut st = self.conn.prepare(
                "SELECT date, template_id FROM days
                 WHERE deleted = 0 AND customised = 0 AND overridden = 0 AND date > ?1",
            )?;
            let v = st
                .query_map([date_key(now.date())], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        for (d, current) in days {
            let date = parse_date_key(&d)?;
            if weekday_num(date) == weekday && current.as_deref() != template_id {
                self.rebuild_day(date, template_id, 0)?;
                self.conn.execute(
                    "UPDATE days SET template_id = ?2, updated_at = ?3 WHERE date = ?1",
                    params![d, template_id, ts],
                )?;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ days

    pub fn day(&self, date: NaiveDate) -> Result<Option<Day>> {
        Ok(self
            .conn
            .query_row(
                "SELECT template_id, customised, overridden FROM days WHERE date = ?1 AND deleted = 0",
                [date_key(date)],
                |r| {
                    Ok(Day {
                        date,
                        template_id: r.get(0)?,
                        customised: r.get(1)?,
                        overridden: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// Days that exist in `from..=to`, oldest first.
    pub fn days_in_range(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<Day>> {
        let mut st = self.conn.prepare(
            "SELECT date, template_id, customised, overridden FROM days
             WHERE deleted = 0 AND date >= ?1 AND date <= ?2 ORDER BY date",
        )?;
        let rows = st
            .query_map(params![date_key(from), date_key(to)], |r| {
                Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(d, template_id, customised, overridden)| {
                Ok(Day { date: parse_date_key(&d)?, template_id, customised, overridden })
            })
            .collect()
    }

    /// Get a day, creating it from the weekly default the first time a date
    /// is opened or reached. Past dates that were never created get an empty
    /// plan, so they don't show up as a day of missed blocks.
    pub fn ensure_day(&self, date: NaiveDate, now: NaiveDateTime) -> Result<Day> {
        if let Some(d) = self.day(date)? {
            return Ok(d);
        }
        self.with_tx(|s| s.ensure_day_in(date, now))
    }

    pub(crate) fn ensure_day_in(&self, date: NaiveDate, now: NaiveDateTime) -> Result<Day> {
        if let Some(d) = self.day(date)? {
            return Ok(d);
        }
        let tid = self.week_assignment()?[(weekday_num(date) - 1) as usize].clone();
        self.conn.execute(
            "INSERT INTO days (date, template_id, customised, overridden, updated_at, deleted)
             VALUES (?1, ?2, 0, 0, ?3, 0)
             ON CONFLICT(date) DO UPDATE SET template_id = excluded.template_id, customised = 0,
                overridden = 0, updated_at = excluded.updated_at, deleted = 0",
            params![date_key(date), tid, stamp()],
        )?;
        if date >= now.date() {
            self.rebuild_day(date, tid.as_deref(), 0)?;
        }
        Ok(Day { date, template_id: tid, customised: false, overridden: false })
    }

    pub fn day_blocks(&self, date: NaiveDate) -> Result<Vec<DayBlock>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {DB_COLS} FROM day_blocks WHERE date = ?1 AND deleted = 0 ORDER BY start_min"
        ))?;
        let v = st.query_map([date_key(date)], dblock_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn day_block(&self, id: &str) -> Result<Option<DayBlock>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {DB_COLS} FROM day_blocks WHERE id = ?1 AND deleted = 0"),
                [id],
                dblock_row,
            )
            .optional()?)
    }

    /// Copy `template_id` into `date`. Blocks that ended at or before
    /// `cutoff` (minutes) are kept as they are; everything after is replaced.
    /// A new block that starts before the cutoff is clipped so it never
    /// overlaps a kept block.
    fn rebuild_day(&self, date: NaiveDate, template_id: Option<&str>, cutoff: u16) -> Result<()> {
        let existing = self.day_blocks(date)?;
        let kept: Vec<&DayBlock> = existing.iter().filter(|b| b.end_min <= cutoff).collect();
        let kept_ids: HashSet<&str> = kept.iter().map(|b| b.id.as_str()).collect();
        let tblocks = match template_id {
            Some(t) => self.template_blocks(t)?,
            None => Vec::new(),
        };
        let ts = stamp();
        let dk = date_key(date);
        let mut written = HashSet::new();
        for tb in &tblocks {
            if tb.end_min <= cutoff {
                continue;
            }
            let id = day_block_id(date, &tb.id);
            if kept_ids.contains(id.as_str()) {
                continue;
            }
            let mut start = tb.start_min;
            for k in &kept {
                if k.start_min < tb.end_min && k.end_min > start {
                    start = k.end_min;
                }
            }
            if start >= tb.end_min {
                continue;
            }
            self.conn.execute(
                UPSERT_DAY_BLOCK,
                params![id, dk, tb.id, start, tb.end_min, tb.title, tb.note, ts],
            )?;
            written.insert(id);
        }
        for b in &existing {
            if b.end_min > cutoff && !written.contains(&b.id) {
                self.conn.execute(
                    "UPDATE day_blocks SET deleted = 1, updated_at = ?2 WHERE id = ?1",
                    params![b.id, ts],
                )?;
            }
        }
        Ok(())
    }

    /// Day override (rule 3). For today, blocks that already ended keep their
    /// records and the rest of the day comes from the new template; future
    /// days are replaced whole. Hand edits on that day are dropped.
    pub fn switch_day_template(
        &self,
        date: NaiveDate,
        template_id: Option<&str>,
        now: NaiveDateTime,
    ) -> Result<()> {
        self.with_tx(|s| {
            let today = now.date();
            if date < today {
                bail!("can't change the template of a past day");
            }
            if let Some(t) = template_id {
                if s.template(t)?.is_none() {
                    bail!("no such template");
                }
            }
            s.ensure_day_in(date, now)?;
            let cutoff = if date == today { minute_of(&now) } else { 0 };
            s.rebuild_day(date, template_id, cutoff)?;
            let default = s.week_assignment()?[(weekday_num(date) - 1) as usize].clone();
            let overridden = default.as_deref() != template_id;
            s.conn.execute(
                "UPDATE days SET template_id = ?2, customised = 0, overridden = ?3, updated_at = ?4
                 WHERE date = ?1",
                params![date_key(date), template_id, overridden, stamp()],
            )?;
            Ok(())
        })
    }

    // ------------------------------------------------------------ day edits (rule 4)

    fn mark_customised(&self, date: NaiveDate) -> Result<()> {
        self.conn.execute(
            "UPDATE days SET customised = 1, updated_at = ?2 WHERE date = ?1",
            params![date_key(date), stamp()],
        )?;
        Ok(())
    }

    pub fn add_day_block(&self, date: NaiveDate, block: BlockInput, now: NaiveDateTime) -> Result<DayBlock> {
        self.with_tx(|s| {
            s.ensure_day_in(date, now)?;
            let existing = s.day_blocks(date)?;
            validate_blocks(
                existing
                    .iter()
                    .map(|b| (b.start_min, b.end_min, b.title.as_str()))
                    .chain(std::iter::once((block.start_min, block.end_min, block.title.as_str()))),
            )?;
            let id = new_id();
            s.conn.execute(
                "INSERT INTO day_blocks (id, date, template_block_id, start_min, end_min, title, note,
                                         status, status_at, updated_at, deleted)
                 VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, 'pending', NULL, ?7, 0)",
                params![id, date_key(date), block.start_min, block.end_min, block.title.trim(), block.note, stamp()],
            )?;
            s.mark_customised(date)?;
            s.day_block(&id)?.context("new block missing after save")
        })
    }

    /// Move, retime, rename or re-note one block on one day.
    pub fn update_day_block(&self, block: &DayBlock) -> Result<()> {
        self.with_tx(|s| {
            let others = s.day_blocks(block.date)?;
            validate_blocks(
                others
                    .iter()
                    .filter(|b| b.id != block.id)
                    .map(|b| (b.start_min, b.end_min, b.title.as_str()))
                    .chain(std::iter::once((block.start_min, block.end_min, block.title.as_str()))),
            )?;
            let n = s.conn.execute(
                "UPDATE day_blocks SET start_min = ?2, end_min = ?3, title = ?4, note = ?5, updated_at = ?6
                 WHERE id = ?1 AND deleted = 0",
                params![block.id, block.start_min, block.end_min, block.title.trim(), block.note, stamp()],
            )?;
            if n == 0 {
                bail!("no such block");
            }
            s.mark_customised(block.date)
        })
    }

    pub fn delete_day_block(&self, id: &str) -> Result<()> {
        self.with_tx(|s| {
            let b = s.day_block(id)?.context("no such block")?;
            s.conn.execute(
                "UPDATE day_blocks SET deleted = 1, updated_at = ?2 WHERE id = ?1",
                params![id, stamp()],
            )?;
            s.mark_customised(b.date)
        })
    }

    // ------------------------------------------------------------ status

    pub fn set_status(&self, id: &str, status: Status) -> Result<DayBlock> {
        let ts = stamp();
        let at = if status == Status::Pending { None } else { Some(ts) };
        let n = self.conn.execute(
            "UPDATE day_blocks SET status = ?2, status_at = ?3, updated_at = ?4 WHERE id = ?1 AND deleted = 0",
            params![id, status.as_str(), at, ts],
        )?;
        if n == 0 {
            bail!("no such block");
        }
        self.day_block(id)?.context("block missing after save")
    }

    /// Turn pending blocks that have ended into Missed. Returns how many.
    pub fn mark_missed(&self, now: NaiveDateTime) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE day_blocks SET status = 'missed', updated_at = ?3
             WHERE deleted = 0 AND status = 'pending'
               AND (date < ?1 OR (date = ?1 AND end_min <= ?2))",
            params![date_key(now.date()), minute_of(&now), stamp()],
        )?)
    }

    // ------------------------------------------------------------ settings

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1 AND deleted = 0", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value, updated_at, deleted) VALUES (?1, ?2, ?3, 0)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at, deleted = 0
             WHERE settings.value IS NOT excluded.value OR settings.deleted = 1",
            params![key, value, stamp()],
        )?;
        Ok(())
    }
}

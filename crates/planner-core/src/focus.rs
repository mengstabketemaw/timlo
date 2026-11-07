//! Focus sessions: time spent with the Focus player running. Logged by the
//! app as a session goes on and synced, so every device's history adds up.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use chrono::NaiveDate;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::store::{stamp, Store};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusSession {
    pub id: String,
    /// Local date the session started on.
    pub date: NaiveDate,
    /// Unix ms.
    pub started_at: i64,
    /// Minutes actually spent playing.
    pub minutes: u32,
    /// The plan block it ran during, if any.
    pub block_title: Option<String>,
}

impl Store {
    /// Create or update a session. A session only grows, so a late or
    /// repeated save never shrinks it.
    pub fn save_focus_session(&self, s: &FocusSession) -> Result<()> {
        if s.id.trim().is_empty() {
            bail!("a focus session needs an id");
        }
        self.conn.execute(
            "INSERT INTO focus_sessions (id, date, started_at, minutes, block_title, updated_at, deleted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)
             ON CONFLICT(id) DO UPDATE SET
                minutes = MAX(focus_sessions.minutes, excluded.minutes),
                block_title = COALESCE(excluded.block_title, focus_sessions.block_title),
                updated_at = excluded.updated_at
             WHERE excluded.minutes > focus_sessions.minutes",
            params![
                s.id,
                s.date.format("%Y-%m-%d").to_string(),
                s.started_at,
                s.minutes.min(24 * 60),
                s.block_title,
                stamp()
            ],
        )?;
        Ok(())
    }

    /// Focus minutes per day in `[from, to]`; days without any are left out.
    pub fn focus_minutes(&self, from: NaiveDate, to: NaiveDate) -> Result<BTreeMap<NaiveDate, u32>> {
        let mut st = self.conn.prepare(
            "SELECT date, SUM(minutes) FROM focus_sessions
             WHERE deleted = 0 AND date BETWEEN ?1 AND ?2 GROUP BY date",
        )?;
        let rows = st.query_map(
            params![from.format("%Y-%m-%d").to_string(), to.format("%Y-%m-%d").to_string()],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (d, m) = row?;
            if let Ok(d) = NaiveDate::parse_from_str(&d, "%Y-%m-%d") {
                out.insert(d, m.max(0) as u32);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn sessions_only_grow_and_sum_per_day() {
        let s = Store::open_in_memory().unwrap();
        let mut a = FocusSession { id: "a".into(), date: d("2026-11-02"), started_at: 1, minutes: 10, block_title: None };
        s.save_focus_session(&a).unwrap();
        a.minutes = 25;
        s.save_focus_session(&a).unwrap();
        a.minutes = 5; // stale save
        s.save_focus_session(&a).unwrap();
        let b = FocusSession { id: "b".into(), date: d("2026-11-02"), started_at: 2, minutes: 15, block_title: None };
        s.save_focus_session(&b).unwrap();
        let m = s.focus_minutes(d("2026-11-01"), d("2026-11-03")).unwrap();
        assert_eq!(m.get(&d("2026-11-02")), Some(&40));
        assert_eq!(m.len(), 1);
    }
}

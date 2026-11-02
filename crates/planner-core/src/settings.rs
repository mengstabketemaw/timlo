//! Reminder timing, quiet hours and display thresholds, stored in the
//! `settings` table.

use anyhow::{Context, Result};
use serde::Serialize;

use crate::store::Store;
use crate::time::{fmt_hm, parse_hm};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Settings {
    /// "Now: Study (07:00–08:00)" when a block starts.
    pub remind_start: bool,
    /// "Gym in 5 min"; minutes before start, 0 = off.
    pub remind_before_start: u16,
    /// "Study ends in 5 min — next: Gym"; minutes before end, 0 = off.
    pub remind_before_end: u16,
    /// "Did you finish Study?" with Done / Skip when a block ends unanswered.
    pub remind_unfinished: bool,
    /// Minutes left at which the Waybar module gets the `ending-soon` class.
    pub ending_soon: u16,
    /// No sound or pop-ups between these minutes (may wrap past midnight).
    pub quiet_hours: Option<(u16, u16)>,
    /// Play the notification sound.
    pub sound: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            remind_start: true,
            remind_before_start: 5,
            remind_before_end: 0,
            remind_unfinished: true,
            ending_soon: 5,
            quiet_hours: None,
            sound: true,
        }
    }
}

/// `"23:00-07:00"` → `Some((1380, 420))`; `""` or `"off"` → `None`.
pub fn parse_quiet_hours(s: &str) -> Result<Option<(u16, u16)>> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("off") {
        return Ok(None);
    }
    let (a, b) = s.split_once('-').context("quiet hours must look like 23:00-07:00")?;
    Ok(Some((parse_hm(a)?, parse_hm(b)?)))
}

pub fn fmt_quiet_hours(q: Option<(u16, u16)>) -> String {
    q.map_or_else(String::new, |(a, b)| format!("{}-{}", fmt_hm(a), fmt_hm(b)))
}

impl Settings {
    pub fn load(store: &Store) -> Result<Self> {
        let mut s = Self::default();
        let get = |k: &str| store.setting(k);
        if let Some(v) = get("remind_start")? {
            s.remind_start = v == "true";
        }
        if let Some(v) = get("remind_before_start")? {
            s.remind_before_start = v.parse().unwrap_or(s.remind_before_start);
        }
        if let Some(v) = get("remind_before_end")? {
            s.remind_before_end = v.parse().unwrap_or(s.remind_before_end);
        }
        if let Some(v) = get("remind_unfinished")? {
            s.remind_unfinished = v == "true";
        }
        if let Some(v) = get("ending_soon")? {
            s.ending_soon = v.parse().unwrap_or(s.ending_soon);
        }
        if let Some(v) = get("quiet_hours")? {
            s.quiet_hours = parse_quiet_hours(&v).unwrap_or(None);
        }
        if let Some(v) = get("sound")? {
            s.sound = v == "true";
        }
        Ok(s)
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.set_setting("remind_start", &self.remind_start.to_string())?;
        store.set_setting("remind_before_start", &self.remind_before_start.to_string())?;
        store.set_setting("remind_before_end", &self.remind_before_end.to_string())?;
        store.set_setting("remind_unfinished", &self.remind_unfinished.to_string())?;
        store.set_setting("ending_soon", &self.ending_soon.to_string())?;
        store.set_setting("quiet_hours", &fmt_quiet_hours(self.quiet_hours))?;
        store.set_setting("sound", &self.sound.to_string())?;
        Ok(())
    }

    pub fn is_quiet(&self, minute: u16) -> bool {
        match self.quiet_hours {
            None => false,
            Some((a, b)) if a <= b => a <= minute && minute < b,
            Some((a, b)) => minute >= a || minute < b,
        }
    }
}

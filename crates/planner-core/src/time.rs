//! Minutes-from-midnight helpers. All times are device-local.

use anyhow::{anyhow, bail, Result};
use chrono::{NaiveDateTime, Timelike};

pub const MINUTES_PER_DAY: u16 = 24 * 60;

/// Parse `HH:MM` (00:00–24:00) into minutes from midnight.
pub fn parse_hm(s: &str) -> Result<u16> {
    let (h, m) = s
        .trim()
        .split_once(':')
        .ok_or_else(|| anyhow!("invalid time {s:?}, expected HH:MM"))?;
    let h: u16 = h.parse().map_err(|_| anyhow!("invalid hour in {s:?}"))?;
    let m: u16 = m.parse().map_err(|_| anyhow!("invalid minute in {s:?}"))?;
    if m >= 60 || h > 24 || (h == 24 && m != 0) {
        bail!("time {s:?} is out of range");
    }
    Ok(h * 60 + m)
}

/// `420` → `"07:00"`.
pub fn fmt_hm(min: u16) -> String {
    format!("{:02}:{:02}", min / 60, min % 60)
}

/// `25` → `"25m"`, `90` → `"1h 30m"`.
pub fn fmt_dur(min: u16) -> String {
    match (min / 60, min % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Minute of the day for a local timestamp.
pub fn minute_of(t: &NaiveDateTime) -> u16 {
    (t.hour() * 60 + t.minute()) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_format() {
        assert_eq!(parse_hm("07:00").unwrap(), 420);
        assert_eq!(parse_hm("24:00").unwrap(), 1440);
        assert!(parse_hm("24:01").is_err());
        assert!(parse_hm("7").is_err());
        assert_eq!(fmt_hm(845), "14:05");
        assert_eq!(fmt_dur(25), "25m");
        assert_eq!(fmt_dur(120), "2h");
        assert_eq!(fmt_dur(95), "1h 35m");
    }
}

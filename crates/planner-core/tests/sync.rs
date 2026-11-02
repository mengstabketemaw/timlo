//! Two in-memory "devices" exchanging rows the way the server relays them:
//! one device's pending rows are applied on the other.

use chrono::{NaiveDate, NaiveDateTime};
use planner_core::time::parse_hm;
use planner_core::{BlockInput, Status, Store};

const MON: &str = "2026-11-02";

fn date(d: &str) -> NaiveDate {
    NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()
}

fn at(d: &str, hm: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(&format!("{d} {hm}"), "%Y-%m-%d %H:%M").unwrap()
}

fn b(s: &str, e: &str, t: &str) -> BlockInput {
    BlockInput::new(parse_hm(s).unwrap(), parse_hm(e).unwrap(), t)
}

/// Push everything pending on `from` into `to`; returns rows changed on `to`.
fn relay(from: &Store, to: &Store) -> usize {
    let pending = from.sync_pending(10_000).unwrap();
    let mut changed = 0;
    for p in &pending {
        changed += to.sync_apply(p.table, std::slice::from_ref(&p.row)).unwrap();
    }
    from.sync_ack(&pending.iter().map(|p| p.seq).collect::<Vec<_>>()).unwrap();
    changed
}

fn device_with_plan(now: NaiveDateTime) -> Store {
    let s = Store::open_in_memory().unwrap();
    let t = s.create_template("Workday", None).unwrap();
    s.set_template_blocks(&t.id, &[b("07:00", "08:00", "Study"), b("09:00", "12:00", "Work")], now).unwrap();
    for wd in 1..=7 {
        s.set_week_assignment(wd, Some(&t.id), now).unwrap();
    }
    s
}

#[test]
fn local_writes_are_queued_and_acked() {
    let s = device_with_plan(at(MON, "06:00"));
    let pending = s.sync_pending(100).unwrap();
    let tables: Vec<_> = pending.iter().map(|p| p.table).collect();
    assert!(tables.contains(&"templates"));
    assert!(tables.contains(&"template_blocks"));
    assert!(tables.contains(&"week_assignment"));
    s.sync_ack(&pending.iter().map(|p| p.seq).collect::<Vec<_>>()).unwrap();
    assert_eq!(s.sync_pending_count().unwrap(), 0);

    // A row changed after being read keeps its place in the queue.
    let t = s.templates().unwrap().remove(0);
    s.rename_template(&t.id, "Weekday").unwrap();
    let first = s.sync_pending(100).unwrap();
    s.rename_template(&t.id, "Workday").unwrap();
    s.sync_ack(&first.iter().map(|p| p.seq).collect::<Vec<_>>()).unwrap();
    let again = s.sync_pending(100).unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].row["name"], "Workday");
}

#[test]
fn applied_rows_are_not_queued_again() {
    let a = device_with_plan(at(MON, "06:00"));
    let b = Store::open_in_memory().unwrap();
    assert!(relay(&a, &b) > 0);
    assert_eq!(b.sync_pending_count().unwrap(), 0);
    assert_eq!(b.templates().unwrap()[0].name, "Workday");
    // Same rows again change nothing.
    a.sync_enqueue_all().unwrap();
    assert_eq!(relay(&a, &b), 0);
}

#[test]
fn newer_local_row_wins_over_older_remote_row() {
    let a = device_with_plan(at(MON, "06:00"));
    let b = Store::open_in_memory().unwrap();
    relay(&a, &b);
    let t = a.templates().unwrap().remove(0);

    a.rename_template(&t.id, "Old name").unwrap();
    let stale = a.sync_pending(100).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    b.rename_template(&t.id, "New name").unwrap();

    for p in &stale {
        b.sync_apply(p.table, std::slice::from_ref(&p.row)).unwrap();
    }
    assert_eq!(b.template(&t.id).unwrap().unwrap().name, "New name");
}

#[test]
fn both_devices_create_the_same_day_and_done_survives() {
    let now = at(MON, "07:30");
    let a = device_with_plan(at(MON, "06:00"));
    let b = Store::open_in_memory().unwrap();
    relay(&a, &b);

    // Phone marks Study done; the PC opens the same day before syncing.
    a.ensure_day(date(MON), now).unwrap();
    let study = a.day_blocks(date(MON)).unwrap()[0].clone();
    a.set_status(&study.id, Status::Done).unwrap();
    b.ensure_day(date(MON), now).unwrap();
    assert_eq!(b.day_blocks(date(MON)).unwrap()[0].id, study.id, "same fixed ids");

    // The PC's fresh copy must not beat the phone's Done, in either order.
    relay(&b, &a);
    relay(&a, &b);
    assert_eq!(a.day_block(&study.id).unwrap().unwrap().status, Status::Done);
    assert_eq!(b.day_block(&study.id).unwrap().unwrap().status, Status::Done);
}

#[test]
fn missed_on_one_device_does_not_beat_done_on_another() {
    let a = device_with_plan(at(MON, "06:00"));
    let b = Store::open_in_memory().unwrap();
    relay(&a, &b);
    a.ensure_day(date(MON), at(MON, "06:00")).unwrap();
    relay(&a, &b);

    let study_id = a.day_blocks(date(MON)).unwrap()[0].id.clone();
    a.set_status(&study_id, Status::Done).unwrap(); // phone, offline
    b.mark_missed(at(MON, "08:30")).unwrap(); // PC decides it was missed

    relay(&b, &a);
    relay(&a, &b);
    assert_eq!(b.day_block(&study_id).unwrap().unwrap().status, Status::Done);
    assert_eq!(a.day_block(&study_id).unwrap().unwrap().status, Status::Done);
}

#[test]
fn local_settings_never_sync() {
    let a = Store::open_in_memory().unwrap();
    a.set_setting("local.plan_file_mtime", "123").unwrap();
    a.set_setting("quiet_hours", "23:00-07:00").unwrap();
    let pending = a.sync_pending(100).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].row["key"], "quiet_hours");

    a.sync_enqueue_all().unwrap();
    assert_eq!(a.sync_pending_count().unwrap(), 1);
}

#[test]
fn sync_state_round_trip_and_reset() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.sync_state("pull.days").unwrap(), None);
    s.set_sync_state("pull.days", "2026-11-02T08:00:00Z").unwrap();
    assert_eq!(s.sync_state("pull.days").unwrap().as_deref(), Some("2026-11-02T08:00:00Z"));
    s.set_setting("sound", "true").unwrap();
    s.sync_reset().unwrap();
    assert_eq!(s.sync_state("pull.days").unwrap(), None);
    assert_eq!(s.sync_pending_count().unwrap(), 0);
}

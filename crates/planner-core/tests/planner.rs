use chrono::{NaiveDate, NaiveDateTime};
use planner_core::now::{action_target, now_view};
use planner_core::reminders::{due, ReminderKind};
use planner_core::store::day_block_id;
use planner_core::time::parse_hm;
use planner_core::{plan_file, BlockInput, Settings, Status, Store, Template};

// 2026-11-02 is a Monday.
const MON: &str = "2026-11-02";
const TUE: &str = "2026-11-03";
const WED: &str = "2026-11-04";
const FRI_BEFORE: &str = "2026-10-30";

fn date(d: &str) -> NaiveDate {
    NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()
}

fn at(d: &str, hm: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(&format!("{d} {hm}"), "%Y-%m-%d %H:%M").unwrap()
}

fn m(hm: &str) -> u16 {
    parse_hm(hm).unwrap()
}

fn b(s: &str, e: &str, t: &str) -> BlockInput {
    BlockInput::new(m(s), m(e), t)
}

fn titles(store: &Store, d: &str) -> Vec<String> {
    store.day_blocks(date(d)).unwrap().into_iter().map(|b| b.title).collect()
}

/// Workday Mon–Fri, Rest day Sat–Sun.
fn setup(now: NaiveDateTime) -> (Store, Template, Template) {
    let s = Store::open_in_memory().unwrap();
    let work = s.create_template("Workday", None).unwrap();
    s.set_template_blocks(
        &work.id,
        &[b("07:00", "08:00", "Study"), b("09:00", "12:00", "Work"), b("18:00", "19:00", "Gym")],
        now,
    )
    .unwrap();
    let rest = s.create_template("Rest day", None).unwrap();
    s.set_template_blocks(&rest.id, &[b("10:00", "11:00", "Walk"), b("15:00", "16:00", "Read")], now)
        .unwrap();
    for wd in 1..=5 {
        s.set_week_assignment(wd, Some(&work.id), now).unwrap();
    }
    for wd in 6..=7 {
        s.set_week_assignment(wd, Some(&rest.id), now).unwrap();
    }
    (s, work, rest)
}

#[test]
fn overlapping_blocks_are_rejected() {
    let now = at(MON, "06:00");
    let (s, work, _) = setup(now);
    let err = s
        .set_template_blocks(&work.id, &[b("07:00", "08:00", "A"), b("07:30", "09:00", "B")], now)
        .unwrap_err();
    assert!(err.to_string().contains("overlaps"), "{err}");
    assert!(s.set_template_blocks(&work.id, &[b("08:00", "07:00", "Backwards")], now).is_err());
    // Touching blocks are fine.
    s.set_template_blocks(&work.id, &[b("07:00", "08:00", "A"), b("08:00", "09:00", "B")], now)
        .unwrap();
}

#[test]
fn day_is_copied_from_weekly_template() {
    let now = at(MON, "06:00");
    let (s, work, _) = setup(now);
    let day = s.ensure_day(date(MON), now).unwrap();
    assert_eq!(day.template_id.as_deref(), Some(work.id.as_str()));
    assert_eq!(titles(&s, MON), ["Study", "Work", "Gym"]);
    let first = &s.day_blocks(date(MON)).unwrap()[0];
    assert_eq!(first.status, Status::Pending);
    assert_eq!(first.id, day_block_id(date(MON), first.template_block_id.as_ref().unwrap()));

    s.ensure_day(date("2026-11-08"), now).unwrap(); // Sunday
    assert_eq!(titles(&s, "2026-11-08"), ["Walk", "Read"]);
}

#[test]
fn past_days_never_seen_get_an_empty_plan() {
    let now = at(MON, "06:00");
    let (s, _, _) = setup(now);
    s.ensure_day(date(FRI_BEFORE), now).unwrap();
    assert!(titles(&s, FRI_BEFORE).is_empty());
}

#[test]
fn template_edits_reach_today_and_future_but_not_past_or_customised() {
    let (s, work, _) = setup(at(FRI_BEFORE, "06:00"));
    s.ensure_day(date(FRI_BEFORE), at(FRI_BEFORE, "06:00")).unwrap();

    let now = at(MON, "10:00");
    s.ensure_day(date(MON), now).unwrap();
    s.ensure_day(date(TUE), now).unwrap();
    s.add_day_block(date(WED), b("20:00", "21:00", "Dinner out"), now).unwrap();
    assert!(s.day(date(WED)).unwrap().unwrap().customised);

    // Rename Gym → Swim and move Study (already over today).
    let mut blocks = s.template_blocks(&work.id).unwrap();
    for tb in &mut blocks {
        match tb.title.as_str() {
            "Gym" => tb.title = "Swim".into(),
            "Study" => tb.start_min = m("06:30"),
            _ => {}
        }
        s.update_template_block(tb, now).unwrap();
    }

    assert_eq!(titles(&s, FRI_BEFORE), ["Study", "Work", "Gym"], "past day untouched");
    assert_eq!(titles(&s, TUE), ["Study", "Work", "Swim"], "future day updated");
    assert_eq!(titles(&s, WED), ["Study", "Work", "Gym", "Dinner out"], "customised day untouched");

    let today = s.day_blocks(date(MON)).unwrap();
    assert_eq!(today[0].start_min, m("07:00"), "today's ended block kept as it was");
    assert_eq!(today[2].title, "Swim", "today's upcoming block updated");
}

#[test]
fn status_survives_template_edits() {
    let now = at(MON, "09:30");
    let (s, work, _) = setup(now);
    s.ensure_day(date(MON), now).unwrap();
    let current = s.day_blocks(date(MON)).unwrap()[1].clone();
    assert_eq!(current.title, "Work");
    s.set_status(&current.id, Status::Done).unwrap();

    let mut tb = s.template_blocks(&work.id).unwrap()[1].clone();
    tb.title = "Deep work".into();
    s.update_template_block(&tb, now).unwrap();

    let after = s.day_block(&current.id).unwrap().unwrap();
    assert_eq!(after.title, "Deep work");
    assert_eq!(after.status, Status::Done);
}

#[test]
fn switching_mid_day_keeps_past_records_and_replaces_the_rest() {
    let now = at(MON, "10:30");
    let (s, _, rest) = setup(now);
    s.ensure_day(date(MON), now).unwrap();
    let study = s.day_blocks(date(MON)).unwrap()[0].clone();
    s.set_status(&study.id, Status::Done).unwrap();

    s.switch_day_template(date(MON), Some(&rest.id), now).unwrap();
    let blocks = s.day_blocks(date(MON)).unwrap();
    let names: Vec<_> = blocks.iter().map(|b| (b.title.as_str(), b.status)).collect();
    assert_eq!(
        names,
        [("Study", Status::Done), ("Walk", Status::Pending), ("Read", Status::Pending)]
    );
    let day = s.day(date(MON)).unwrap().unwrap();
    assert!(day.overridden);
    assert_eq!(day.template_id.as_deref(), Some(rest.id.as_str()));

    assert!(s.switch_day_template(date(FRI_BEFORE), Some(&rest.id), now).is_err());
}

#[test]
fn switching_clips_new_blocks_that_overlap_kept_ones() {
    let now = at(MON, "08:30");
    let (s, work, _) = setup(now);
    let early = s.create_template("Early", None).unwrap();
    s.set_template_blocks(&early.id, &[b("07:30", "09:00", "Run")], now).unwrap();
    s.ensure_day(date(MON), now).unwrap();
    s.switch_day_template(date(MON), Some(&early.id), now).unwrap();
    let blocks = s.day_blocks(date(MON)).unwrap();
    assert_eq!(blocks[0].title, "Study");
    assert_eq!((blocks[1].title.as_str(), blocks[1].start_min), ("Run", m("08:00")));
    let _ = work;
}

#[test]
fn week_assignment_change_moves_future_default_days_only() {
    let now = at(MON, "06:00");
    let (s, work, rest) = setup(now);
    let next_mon = date("2026-11-09");
    let next_mon_after = date("2026-11-16");
    s.ensure_day(next_mon, now).unwrap();
    s.ensure_day(next_mon_after, now).unwrap();
    s.switch_day_template(next_mon_after, Some(&work.id), now).unwrap(); // same as default: not an override
    let gym = s.create_template("Gym day", None).unwrap();
    s.set_template_blocks(&gym.id, &[b("08:00", "09:00", "Lift")], now).unwrap();
    s.switch_day_template(next_mon, Some(&rest.id), now).unwrap(); // explicit override

    s.set_week_assignment(1, Some(&gym.id), now).unwrap();
    assert_eq!(titles(&s, "2026-11-09"), ["Walk", "Read"], "override kept");
    assert_eq!(titles(&s, "2026-11-16"), ["Lift"], "default day follows new assignment");
}

#[test]
fn ended_pending_blocks_become_missed() {
    let now = at(MON, "06:00");
    let (s, _, _) = setup(now);
    s.ensure_day(date(MON), now).unwrap();
    let study = s.day_blocks(date(MON)).unwrap()[0].clone();
    s.set_status(&study.id, Status::Skipped).unwrap();

    assert_eq!(s.mark_missed(at(MON, "12:30")).unwrap(), 1);
    let st: Vec<_> = s.day_blocks(date(MON)).unwrap().into_iter().map(|b| b.status).collect();
    assert_eq!(st, [Status::Skipped, Status::Missed, Status::Pending]);

    // A missed block can still be answered later.
    let work = s.day_blocks(date(MON)).unwrap()[1].clone();
    assert_eq!(s.set_status(&work.id, Status::Done).unwrap().status, Status::Done);
}

#[test]
fn now_and_next() {
    let now = at(MON, "06:00");
    let (s, _, _) = setup(now);
    s.ensure_day(date(MON), now).unwrap();
    let blocks = s.day_blocks(date(MON)).unwrap();

    let v = now_view(&blocks, m("10:00"));
    assert_eq!(v.current.unwrap().title, "Work");
    assert_eq!(v.next.unwrap().title, "Gym");
    assert_eq!(v.remaining(), Some(120));
    assert_eq!(v.progress(), Some(33));

    let v = now_view(&blocks, m("12:30"));
    assert!(v.current.is_none());
    assert_eq!(v.next.unwrap().title, "Gym");

    let v = now_view(&blocks, m("08:00"));
    assert!(v.current.is_none(), "end is exclusive");
    assert_eq!(v.next.unwrap().title, "Work");

    let v = now_view(&blocks, m("20:00"));
    assert!(v.current.is_none() && v.next.is_none());
}

#[test]
fn done_skip_target_falls_back_to_last_unanswered_block() {
    let now = at(MON, "06:00");
    let (s, _, _) = setup(now);
    s.ensure_day(date(MON), now).unwrap();
    let blocks = s.day_blocks(date(MON)).unwrap();
    assert_eq!(action_target(&blocks, m("10:00")).unwrap().title, "Work");
    assert_eq!(action_target(&blocks, m("13:00")).unwrap().title, "Work");
    s.set_status(&blocks[1].id, Status::Done).unwrap();
    let blocks = s.day_blocks(date(MON)).unwrap();
    assert_eq!(action_target(&blocks, m("13:00")).unwrap().title, "Study");
    assert!(action_target(&blocks, m("06:00")).is_none());
}

#[test]
fn reminders_fire_in_their_minute() {
    let now = at(MON, "06:00");
    let (s, _, _) = setup(now);
    s.ensure_day(date(MON), now).unwrap();
    let blocks = s.day_blocks(date(MON)).unwrap();
    let settings = Settings { remind_before_end: 5, ..Settings::default() };

    let r = due(&blocks, m("06:55"), m("06:55"), &settings);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].summary, "Study in 5 min");

    let r = due(&blocks, m("07:00"), m("07:00"), &settings);
    assert_eq!(r[0].kind, ReminderKind::Start);
    assert_eq!(r[0].summary, "Now: Study (07:00–08:00)");

    let r = due(&blocks, m("07:55"), m("07:55"), &settings);
    assert_eq!(r[0].summary, "Study ends in 5 min — next: Work");

    let r = due(&blocks, m("08:00"), m("08:00"), &settings);
    assert_eq!(r[0].summary, "Did you finish Study?");
    assert!(r[0].ask);

    assert!(due(&blocks, m("13:00"), m("13:00"), &settings).is_empty());
}

#[test]
fn quiet_hours_wrap_midnight() {
    let s = Settings { quiet_hours: Some((m("23:00"), m("07:00"))), ..Settings::default() };
    assert!(s.is_quiet(m("23:30")));
    assert!(s.is_quiet(m("03:00")));
    assert!(!s.is_quiet(m("07:00")));
    assert!(!s.is_quiet(m("12:00")));
}

#[test]
fn settings_round_trip() {
    let s = Store::open_in_memory().unwrap();
    let set = Settings { quiet_hours: Some((m("22:00"), m("06:30"))), sound: false, ..Settings::default() };
    set.save(&s).unwrap();
    assert_eq!(Settings::load(&s).unwrap(), set);
}

const PLAN: &str = r#"
[settings]
quiet_hours = "23:00-07:00"
remind_before_start = 10

[week]
"mon-fri" = "Workday"
"sat,sun" = "Weekend"

[[templates]]
name = "Workday"
blocks = [
  { start = "07:00", end = "08:00", title = "Study" },
  { start = "09:00", end = "12:00", title = "Work" },
]

[[templates]]
name = "Weekend"
blocks = [{ start = "10:00", end = "11:00", title = "Walk" }]
"#;

#[test]
fn plan_file_import_is_idempotent_and_keeps_statuses() {
    let s = Store::open_in_memory().unwrap();
    let now = at(MON, "07:30");
    let plan = plan_file::parse(PLAN).unwrap();
    let report = plan_file::import(&s, &plan, now).unwrap();
    assert_eq!(report.templates, 2);
    assert_eq!(report.blocks, 3);

    s.ensure_day(date(MON), now).unwrap();
    let study = s.day_blocks(date(MON)).unwrap()[0].clone();
    s.set_status(&study.id, Status::Done).unwrap();

    // Same file again, then a version with Work moved later.
    plan_file::import(&s, &plan, now).unwrap();
    let moved = PLAN.replace(r#"start = "09:00""#, r#"start = "09:30""#);
    plan_file::import(&s, &plan_file::parse(&moved).unwrap(), now).unwrap();

    assert_eq!(s.templates().unwrap().len(), 2);
    let blocks = s.day_blocks(date(MON)).unwrap();
    assert_eq!(blocks[0].status, Status::Done);
    assert_eq!(blocks[1].start_min, m("09:30"));

    let settings = Settings::load(&s).unwrap();
    assert_eq!(settings.remind_before_start, 10);
    assert!(settings.is_quiet(m("23:30")));
    assert_eq!(s.week_assignment().unwrap().iter().filter(|t| t.is_some()).count(), 7);
}

#[test]
fn plan_file_errors_are_reported() {
    let s = Store::open_in_memory().unwrap();
    let now = at(MON, "07:30");
    let bad_week = PLAN.replace(r#""sat,sun" = "Weekend""#, r#""sat,sun" = "Nope""#);
    let err = plan_file::import(&s, &plan_file::parse(&bad_week).unwrap(), now).unwrap_err();
    assert!(format!("{err:#}").contains("unknown template"), "{err:#}");
    assert!(s.templates().unwrap().is_empty(), "nothing written on error");

    let overlap = PLAN.replace(r#"start = "09:00""#, r#"start = "07:30""#);
    assert!(plan_file::import(&s, &plan_file::parse(&overlap).unwrap(), now).is_err());
}

#[test]
fn example_plan_file_is_valid() {
    let s = Store::open_in_memory().unwrap();
    let text = include_str!("../../plannerd/plan.example.toml");
    plan_file::import(&s, &plan_file::parse(text).unwrap(), at(MON, "06:00")).unwrap();
    assert_eq!(s.templates().unwrap().len(), 3);
}

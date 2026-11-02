/// `println!` that doesn't panic when stdout is closed (e.g. `plannerd today | head`).
macro_rules! outln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

mod client;
mod daemon;
mod notify;
mod paths;
mod waybar;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{Local, NaiveDate, NaiveDateTime};
use clap::{Parser, Subcommand};
use planner_core::now::action_target;
use planner_core::plan_file::{self, ImportReport};
use planner_core::store::weekday_num;
use planner_core::time::{fmt_hm, minute_of};
use planner_core::{Status, Store};

const EXAMPLE_PLAN: &str = include_str!("../plan.example.toml");

#[derive(Parser)]
#[command(name = "plannerd", version, about = "Day planner: background service, Waybar module and CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the background service (reminders, Waybar updates, plan file reload)
    Daemon,
    /// Stream Waybar JSON; use as the custom module's `exec`
    Waybar,
    /// Mark the current block (or the last unanswered one) Done
    Done,
    /// Mark the current block (or the last unanswered one) Skipped
    Skip,
    /// Print the current Waybar JSON once
    Status,
    /// Print a day's plan (default today)
    Today {
        #[arg(long)]
        date: Option<NaiveDate>,
    },
    /// Show today's plan as a desktop notification
    Show,
    /// List templates and the weekly assignment
    Templates,
    /// Switch a day (default today) to another template
    Switch {
        template: String,
        #[arg(long)]
        date: Option<NaiveDate>,
    },
    /// Import a plan file (default ~/.config/timlo/plan.toml)
    Import { file: Option<PathBuf> },
    /// Write an example plan file to ~/.config/timlo/plan.toml and import it
    Init {
        /// Overwrite an existing plan file
        #[arg(long)]
        force: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Daemon => daemon::run(),
        Cmd::Waybar => client::waybar_stream(),
        Cmd::Done => mark(Status::Done),
        Cmd::Skip => mark(Status::Skipped),
        Cmd::Status => {
            let out = daemon::current_output(&open()?, now())?;
            outln!("{}", serde_json::to_string(&out)?);
            Ok(())
        }
        Cmd::Today { date } => today(date),
        Cmd::Show => show(),
        Cmd::Templates => templates(),
        Cmd::Switch { template, date } => switch(&template, date),
        Cmd::Import { file } => {
            let path = file.unwrap_or_else(paths::plan_path);
            let r = import_file(&open()?, &path, now())?;
            client::poke();
            outln!("Imported {} ({} templates, {} blocks)", path.display(), r.templates, r.blocks);
            Ok(())
        }
        Cmd::Init { force } => init(force),
    }
}

fn now() -> NaiveDateTime {
    Local::now().naive_local()
}

fn open() -> Result<Store> {
    Store::open(&paths::db_path())
}

pub(crate) fn import_file(store: &Store, path: &Path, now: NaiveDateTime) -> Result<ImportReport> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let plan = plan_file::parse(&text).with_context(|| format!("parsing {}", path.display()))?;
    plan_file::import(store, &plan, now)
}

fn mark(status: Status) -> Result<()> {
    let store = open()?;
    let now = now();
    store.ensure_day(now.date(), now)?;
    store.mark_missed(now)?;
    let blocks = store.day_blocks(now.date())?;
    let Some(target) = action_target(&blocks, minute_of(&now)) else {
        bail!("nothing to mark right now");
    };
    let b = store.set_status(&target.id, status)?;
    client::poke();
    outln!(
        "{}: {} ({}–{})",
        if status == Status::Done { "Done" } else { "Skipped" },
        b.title,
        fmt_hm(b.start_min),
        fmt_hm(b.end_min)
    );
    Ok(())
}

fn day_lines(store: &Store, date: NaiveDate, now: NaiveDateTime) -> Result<Vec<String>> {
    store.ensure_day(date, now)?;
    store.mark_missed(now)?;
    Ok(store
        .day_blocks(date)?
        .iter()
        .map(|b| {
            format!("{}–{}  {:<24} {}", fmt_hm(b.start_min), fmt_hm(b.end_min), b.title, b.status.as_str())
        })
        .collect())
}

fn today(date: Option<NaiveDate>) -> Result<()> {
    let store = open()?;
    let now = now();
    let date = date.unwrap_or(now.date());
    let day = store.ensure_day(date, now)?;
    let tname = match &day.template_id {
        Some(id) => store.template(id)?.map_or("(deleted template)".into(), |t| t.name),
        None => "(no template)".into(),
    };
    let mut flags = Vec::new();
    if day.overridden {
        flags.push("override");
    }
    if day.customised {
        flags.push("edited");
    }
    let flags = if flags.is_empty() { String::new() } else { format!(" [{}]", flags.join(", ")) };
    outln!("{} {} — {tname}{flags}", date.format("%a"), date);
    let lines = day_lines(&store, date, now)?;
    if lines.is_empty() {
        outln!("  nothing planned");
    }
    for l in lines {
        outln!("  {l}");
    }
    Ok(())
}

fn show() -> Result<()> {
    let store = open()?;
    let now = now();
    let lines = day_lines(&store, now.date(), now)?;
    let body = if lines.is_empty() { "Nothing planned".to_string() } else { lines.join("\n") };
    notify::plain(&format!("Today · {}", now.date().format("%a %d %b")), &body);
    Ok(())
}

fn templates() -> Result<()> {
    let store = open()?;
    let week = store.week_assignment()?;
    let names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    for t in store.templates()? {
        let days: Vec<&str> = (0..7).filter(|&i| week[i].as_deref() == Some(t.id.as_str())).map(|i| names[i]).collect();
        let days = if days.is_empty() { String::new() } else { format!("  ({})", days.join(", ")) };
        outln!("{}{days}", t.name);
        for b in store.template_blocks(&t.id)? {
            outln!("  {}–{}  {}", fmt_hm(b.start_min), fmt_hm(b.end_min), b.title);
        }
    }
    Ok(())
}

fn switch(name: &str, date: Option<NaiveDate>) -> Result<()> {
    let store = open()?;
    let now = now();
    let date = date.unwrap_or(now.date());
    let t = store
        .template_by_name(name)?
        .with_context(|| format!("no template named {name:?} (see `plannerd templates`)"))?;
    store.switch_day_template(date, Some(&t.id), now)?;
    client::poke();
    let default = store.week_assignment()?[(weekday_num(date) - 1) as usize].clone();
    let note = if default.as_deref() == Some(t.id.as_str()) { " (its weekly default)" } else { "" };
    outln!("{date} now runs from {}{note}", t.name);
    Ok(())
}

fn init(force: bool) -> Result<()> {
    let path = paths::plan_path();
    if path.exists() && !force {
        bail!("{} already exists (use --force to overwrite)", path.display());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, EXAMPLE_PLAN)?;
    let r = import_file(&open()?, &path, now())?;
    client::poke();
    outln!("Wrote {} and imported {} templates. Edit it and save; plannerd reloads it.", path.display(), r.templates);
    Ok(())
}

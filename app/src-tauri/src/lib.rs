//! Tauri backend: thin commands over `planner-core`. Every write pokes
//! `plannerd` so Waybar and reminders pick up the change at once.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{Local, NaiveDate, NaiveDateTime};
use planner_core::store::weekday_num;
use planner_core::time::minute_of;
use planner_core::{paths, BlockInput, DayBlock, Status, Store, Template, TemplateBlock};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

struct AppState(Mutex<Store>);

type CmdResult<T> = Result<T, String>;

fn now() -> NaiveDateTime {
    Local::now().naive_local()
}

fn parse_date(s: &str) -> CmdResult<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| format!("bad date {s:?}: {e}"))
}

/// Run `f` against the store, turning errors into messages for the UI.
fn with_store<T>(state: &State<AppState>, f: impl FnOnce(&Store) -> anyhow::Result<T>) -> CmdResult<T> {
    let store = state.0.lock().map_err(|_| "database lock poisoned".to_string())?;
    f(&store).map_err(|e| format!("{e:#}"))
}

/// Like `with_store`, then tell the daemon something changed.
fn write<T>(state: &State<AppState>, f: impl FnOnce(&Store) -> anyhow::Result<T>) -> CmdResult<T> {
    let out = with_store(state, f)?;
    paths::poke_daemon();
    Ok(out)
}

#[derive(Deserialize)]
struct BlockDto {
    id: Option<String>,
    start_min: u16,
    end_min: u16,
    title: String,
    note: Option<String>,
    icon: Option<String>,
    colour: Option<String>,
}

impl From<BlockDto> for BlockInput {
    fn from(b: BlockDto) -> Self {
        let clean = |s: Option<String>| s.filter(|v| !v.trim().is_empty());
        BlockInput {
            id: b.id,
            start_min: b.start_min,
            end_min: b.end_min,
            title: b.title,
            note: clean(b.note),
            icon: clean(b.icon),
            colour: clean(b.colour),
        }
    }
}

#[derive(Serialize)]
struct DayDto {
    date: NaiveDate,
    template_id: Option<String>,
    template_name: Option<String>,
    default_template_id: Option<String>,
    customised: bool,
    overridden: bool,
    is_today: bool,
    is_past: bool,
    /// Minute of the day, only for today.
    now_min: Option<u16>,
    blocks: Vec<DayBlock>,
}

#[derive(Serialize)]
struct TemplateDto {
    #[serde(flatten)]
    template: Template,
    blocks: Vec<TemplateBlock>,
}

#[derive(Serialize)]
struct MonthDayDto {
    date: NaiveDate,
    template_name: Option<String>,
    /// The day has its own plan (false = it would follow the weekly default).
    exists: bool,
    overridden: bool,
    customised: bool,
    total: usize,
    done: usize,
    skipped: usize,
    missed: usize,
}

// ------------------------------------------------------------------ days

#[tauri::command]
fn get_day(state: State<AppState>, date: Option<String>) -> CmdResult<DayDto> {
    let now = now();
    let date = match date {
        Some(d) => parse_date(&d)?,
        None => now.date(),
    };
    with_store(&state, |s| {
        let default_template_id = s.week_assignment()?[(weekday_num(date) - 1) as usize].clone();
        // Looking at a past date that was never planned doesn't create it.
        let day = if date < now.date() { s.day(date)? } else { Some(s.ensure_day(date, now)?) };
        s.mark_missed(now)?;
        let blocks = if day.is_some() { s.day_blocks(date)? } else { Vec::new() };
        let template_id = day.as_ref().and_then(|d| d.template_id.clone());
        let template_name = match &template_id {
            Some(id) => s.template(id)?.map(|t| t.name),
            None => None,
        };
        Ok(DayDto {
            date,
            template_id,
            template_name,
            default_template_id,
            customised: day.as_ref().is_some_and(|d| d.customised),
            overridden: day.as_ref().is_some_and(|d| d.overridden),
            is_today: date == now.date(),
            is_past: date < now.date(),
            now_min: (date == now.date()).then(|| minute_of(&now)),
            blocks,
        })
    })
}

#[tauri::command]
fn set_status(state: State<AppState>, id: String, status: Status) -> CmdResult<DayBlock> {
    write(&state, |s| s.set_status(&id, status))
}

#[tauri::command]
fn switch_day(state: State<AppState>, date: String, template_id: Option<String>) -> CmdResult<()> {
    let date = parse_date(&date)?;
    write(&state, |s| s.switch_day_template(date, template_id.as_deref(), now()))
}

#[tauri::command]
fn add_day_block(state: State<AppState>, date: String, block: BlockDto) -> CmdResult<DayBlock> {
    let date = parse_date(&date)?;
    write(&state, |s| s.add_day_block(date, block.into(), now()))
}

#[tauri::command]
fn update_day_block(
    state: State<AppState>,
    id: String,
    start_min: u16,
    end_min: u16,
    title: String,
    note: Option<String>,
) -> CmdResult<()> {
    write(&state, |s| {
        let mut b = s.day_block(&id)?.ok_or_else(|| anyhow::anyhow!("no such block"))?;
        b.start_min = start_min;
        b.end_min = end_min;
        b.title = title;
        b.note = note.filter(|n| !n.trim().is_empty());
        s.update_day_block(&b)
    })
}

#[tauri::command]
fn delete_day_block(state: State<AppState>, id: String) -> CmdResult<()> {
    write(&state, |s| s.delete_day_block(&id))
}

#[tauri::command]
fn get_month(state: State<AppState>, year: i32, month: u32) -> CmdResult<Vec<MonthDayDto>> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).ok_or("bad month")?;
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .ok_or("bad month")?;
    with_store(&state, |s| {
        let names: HashMap<String, String> = s.templates()?.into_iter().map(|t| (t.id, t.name)).collect();
        let week = s.week_assignment()?;
        let last = next.pred_opt().unwrap_or(next);
        let days: HashMap<NaiveDate, _> =
            s.days_in_range(first, last)?.into_iter().map(|d| (d.date, d)).collect();
        let mut out = Vec::new();
        for date in first.iter_days().take_while(|d| *d < next) {
            let name_of = |id: &Option<String>| id.as_ref().and_then(|i| names.get(i).cloned());
            let mut row = MonthDayDto {
                date,
                template_name: None,
                exists: false,
                overridden: false,
                customised: false,
                total: 0,
                done: 0,
                skipped: 0,
                missed: 0,
            };
            match days.get(&date) {
                Some(day) => {
                    let blocks = s.day_blocks(date)?;
                    let count = |st: Status| blocks.iter().filter(|b| b.status == st).count();
                    row.template_name = name_of(&day.template_id);
                    row.exists = true;
                    row.overridden = day.overridden;
                    row.customised = day.customised;
                    row.total = blocks.len();
                    row.done = count(Status::Done);
                    row.skipped = count(Status::Skipped);
                    row.missed = count(Status::Missed);
                }
                None => row.template_name = name_of(&week[(weekday_num(date) - 1) as usize]),
            }
            out.push(row);
        }
        Ok(out)
    })
}

// ------------------------------------------------------------------ templates

#[tauri::command]
fn list_templates(state: State<AppState>) -> CmdResult<Vec<TemplateDto>> {
    with_store(&state, |s| {
        s.templates()?
            .into_iter()
            .map(|t| Ok(TemplateDto { blocks: s.template_blocks(&t.id)?, template: t }))
            .collect()
    })
}

#[tauri::command]
fn create_template(state: State<AppState>, name: String) -> CmdResult<Template> {
    write(&state, |s| s.create_template(&name, None))
}

#[tauri::command]
fn rename_template(state: State<AppState>, id: String, name: String) -> CmdResult<()> {
    write(&state, |s| s.rename_template(&id, &name))
}

#[tauri::command]
fn duplicate_template(state: State<AppState>, id: String, name: String) -> CmdResult<Template> {
    write(&state, |s| s.duplicate_template(&id, &name))
}

#[tauri::command]
fn delete_template(state: State<AppState>, id: String) -> CmdResult<()> {
    write(&state, |s| s.delete_template(&id))
}

#[tauri::command]
fn save_template_blocks(
    state: State<AppState>,
    id: String,
    blocks: Vec<BlockDto>,
) -> CmdResult<Vec<TemplateBlock>> {
    let blocks: Vec<BlockInput> = blocks.into_iter().map(Into::into).collect();
    write(&state, |s| s.set_template_blocks(&id, &blocks, now()))
}

// ------------------------------------------------------------------ week

#[tauri::command]
fn get_week(state: State<AppState>) -> CmdResult<Vec<Option<String>>> {
    with_store(&state, |s| Ok(s.week_assignment()?.to_vec()))
}

#[tauri::command]
fn set_week(state: State<AppState>, weekday: u8, template_id: Option<String>) -> CmdResult<()> {
    write(&state, |s| s.set_week_assignment(weekday, template_id.as_deref(), now()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();

    // A second launch (e.g. clicking the Waybar module again) focuses the
    // window that is already open.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }));
    }

    builder
        .setup(|app| {
            let store = Store::open(&paths::db_path())?;
            app.manage(AppState(Mutex::new(store)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_day,
            set_status,
            switch_day,
            add_day_block,
            update_day_block,
            delete_day_block,
            get_month,
            list_templates,
            create_template,
            rename_template,
            duplicate_template,
            delete_template,
            save_template_blocks,
            get_week,
            set_week,
        ])
        .run(tauri::generate_context!())
        .expect("error while running timlo");
}

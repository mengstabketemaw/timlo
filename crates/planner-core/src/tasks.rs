//! Tasks: a scratch book beside the day plan. A task has a title, free-form
//! notes and a checklist of subtasks (tasks with a `parent_id`). Done tasks
//! are hidden unless asked for. Synced like every other table.

use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;

use crate::store::{new_id, stamp, Store};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Task {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub notes: Option<String>,
    pub done: bool,
    /// Unix ms.
    pub done_at: Option<i64>,
    pub created_at: i64,
    /// Checklist, oldest first (empty for subtasks).
    pub subtasks: Vec<Task>,
}

const COLS: &str = "id, parent_id, title, notes, done, done_at, created_at";

fn row(r: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        title: r.get(2)?,
        notes: r.get(3)?,
        done: r.get::<_, i64>(4)? != 0,
        done_at: r.get(5)?,
        created_at: r.get(6)?,
        subtasks: Vec::new(),
    })
}

fn clean_title(title: &str) -> Result<String> {
    let t = title.trim();
    if t.is_empty() {
        bail!("a task needs a title");
    }
    Ok(t.to_string())
}

impl Store {
    /// Top-level tasks with their subtasks: open ones newest first, then (if
    /// `include_done`) done ones, most recently finished first.
    pub fn tasks(&self, include_done: bool) -> Result<Vec<Task>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {COLS} FROM tasks WHERE deleted = 0 AND parent_id IS NULL AND (?1 OR done = 0)
             ORDER BY done, CASE WHEN done = 1 THEN -done_at ELSE -created_at END"
        ))?;
        let mut tasks: Vec<Task> = st.query_map([include_done], row)?.collect::<rusqlite::Result<_>>()?;
        let mut sub = self.conn.prepare(&format!(
            "SELECT {COLS} FROM tasks WHERE deleted = 0 AND parent_id = ?1 ORDER BY created_at, id"
        ))?;
        for t in &mut tasks {
            t.subtasks = sub.query_map([&t.id], row)?.collect::<rusqlite::Result<_>>()?;
        }
        Ok(tasks)
    }

    /// How many top-level tasks are done (and so hidden by default).
    pub fn done_task_count(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE deleted = 0 AND parent_id IS NULL AND done = 1",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    pub fn task(&self, id: &str) -> Result<Option<Task>> {
        Ok(self
            .conn
            .query_row(&format!("SELECT {COLS} FROM tasks WHERE id = ?1 AND deleted = 0"), [id], row)
            .optional()?)
    }

    /// A new task, or a subtask of `parent_id` (one level deep).
    pub fn add_task(&self, title: &str, parent_id: Option<&str>) -> Result<Task> {
        let title = clean_title(title)?;
        if let Some(p) = parent_id {
            let parent = self.task(p)?.context("that task no longer exists")?;
            if parent.parent_id.is_some() {
                bail!("subtasks can't have subtasks");
            }
        }
        let now = stamp();
        let id = new_id();
        self.conn.execute(
            "INSERT INTO tasks (id, parent_id, title, notes, done, done_at, created_at, updated_at, deleted)
             VALUES (?1, ?2, ?3, NULL, 0, NULL, ?4, ?4, 0)",
            params![id, parent_id, title, now],
        )?;
        self.task(&id)?.context("task vanished")
    }

    /// Change the title and notes (empty notes are cleared).
    pub fn update_task(&self, id: &str, title: &str, notes: Option<&str>) -> Result<()> {
        let title = clean_title(title)?;
        let notes = notes.map(str::trim_end).filter(|n| !n.trim().is_empty());
        let n = self.conn.execute(
            "UPDATE tasks SET title = ?2, notes = ?3, updated_at = ?4
             WHERE id = ?1 AND deleted = 0 AND (title IS NOT ?2 OR notes IS NOT ?3)",
            params![id, title, notes, stamp()],
        )?;
        if n == 0 && self.task(id)?.is_none() {
            bail!("that task no longer exists");
        }
        Ok(())
    }

    pub fn set_task_done(&self, id: &str, done: bool) -> Result<()> {
        let now = stamp();
        let n = self.conn.execute(
            "UPDATE tasks SET done = ?2, done_at = CASE WHEN ?2 THEN ?3 ELSE NULL END, updated_at = ?3
             WHERE id = ?1 AND deleted = 0",
            params![id, done, now],
        )?;
        if n == 0 {
            bail!("that task no longer exists");
        }
        Ok(())
    }

    /// Delete a task and its subtasks.
    pub fn delete_task(&self, id: &str) -> Result<()> {
        self.with_tx(|s| {
            let now = stamp();
            s.conn.execute(
                "UPDATE tasks SET deleted = 1, updated_at = ?2 WHERE (id = ?1 OR parent_id = ?1) AND deleted = 0",
                params![id, now],
            )?;
            Ok(())
        })
    }
}

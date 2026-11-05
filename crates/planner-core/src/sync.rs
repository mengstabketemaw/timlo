//! Local side of sync: an outbox of changed rows to push, and applying rows
//! pulled from the server with last-write-wins by `updated_at`.
//!
//! SQLite triggers record every local insert/update in `sync_outbox`. While
//! pulled rows are being applied, `sync_ctl.applying` is 1 so the triggers
//! stay quiet and pulled rows aren't pushed straight back.
//!
//! Settings whose key starts with `local.` belong to one device and are
//! never synced.

use anyhow::{bail, Result};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{params, params_from_iter};
use serde_json::{Map, Number, Value as Json};

use crate::store::Store;

/// One synced table.
pub struct TableSpec {
    pub name: &'static str,
    pub pk: &'static str,
    pub cols: &'static [&'static str],
}

pub const TABLES: &[TableSpec] = &[
    TableSpec { name: "templates", pk: "id", cols: &["id", "name", "colour", "updated_at", "deleted"] },
    TableSpec {
        name: "template_blocks",
        pk: "id",
        cols: &["id", "template_id", "start_min", "end_min", "title", "note", "icon", "colour", "updated_at", "deleted"],
    },
    TableSpec { name: "week_assignment", pk: "weekday", cols: &["weekday", "template_id", "updated_at", "deleted"] },
    TableSpec {
        name: "days",
        pk: "date",
        cols: &["date", "template_id", "customised", "overridden", "updated_at", "deleted"],
    },
    TableSpec {
        name: "day_blocks",
        pk: "id",
        cols: &[
            "id",
            "date",
            "template_block_id",
            "start_min",
            "end_min",
            "title",
            "note",
            "status",
            "status_at",
            "updated_at",
            "deleted",
        ],
    },
    TableSpec { name: "settings", pk: "key", cols: &["key", "value", "updated_at", "deleted"] },
    TableSpec {
        name: "tasks",
        pk: "id",
        cols: &["id", "parent_id", "title", "notes", "done", "done_at", "created_at", "updated_at", "deleted"],
    },
];

pub fn table(name: &str) -> Option<&'static TableSpec> {
    TABLES.iter().find(|t| t.name == name)
}

/// Extra trigger condition so device-local settings never enter the outbox.
fn local_filter(t: &TableSpec) -> &'static str {
    if t.name == "settings" {
        " AND NEW.key NOT LIKE 'local.%'"
    } else {
        ""
    }
}

/// Outbox, control and state tables plus one insert and one update trigger
/// per synced table. Safe to run on every open.
pub(crate) fn schema_sql() -> String {
    let mut sql = String::from(
        "CREATE TABLE IF NOT EXISTS sync_outbox (
             seq INTEGER PRIMARY KEY AUTOINCREMENT,
             tbl TEXT NOT NULL,
             pk  TEXT NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS sync_outbox_row ON sync_outbox (tbl, pk);
         CREATE TABLE IF NOT EXISTS sync_ctl (id INTEGER PRIMARY KEY CHECK (id = 1), applying INTEGER NOT NULL);
         INSERT OR IGNORE INTO sync_ctl (id, applying) VALUES (1, 0);
         UPDATE sync_ctl SET applying = 0;
         CREATE TABLE IF NOT EXISTS sync_state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        ",
    );
    for t in TABLES {
        for (op, suffix) in [("INSERT", "ins"), ("UPDATE", "upd")] {
            sql.push_str(&format!(
                "CREATE TRIGGER IF NOT EXISTS {name}_outbox_{suffix} AFTER {op} ON {name}
                 WHEN (SELECT applying FROM sync_ctl) = 0{filter}
                 BEGIN
                     DELETE FROM sync_outbox WHERE tbl = '{name}' AND pk = CAST(NEW.{pk} AS TEXT);
                     INSERT INTO sync_outbox (tbl, pk) VALUES ('{name}', CAST(NEW.{pk} AS TEXT));
                 END;\n",
                name = t.name,
                pk = t.pk,
                filter = local_filter(t),
            ));
        }
    }
    sql
}

/// A local change waiting to be pushed.
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub seq: i64,
    pub table: &'static str,
    pub row: Map<String, Json>,
}

fn to_json(v: ValueRef<'_>) -> Json {
    match v {
        ValueRef::Null => Json::Null,
        ValueRef::Integer(i) => Json::from(i),
        ValueRef::Real(f) => Number::from_f64(f).map_or(Json::Null, Json::Number),
        ValueRef::Text(t) => Json::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(_) => Json::Null,
    }
}

fn from_json(v: Option<&Json>) -> Value {
    match v {
        None | Some(Json::Null) => Value::Null,
        Some(Json::Bool(b)) => Value::Integer(*b as i64),
        Some(Json::Number(n)) => n
            .as_i64()
            .map(Value::Integer)
            .or_else(|| n.as_f64().map(Value::Real))
            .unwrap_or(Value::Null),
        Some(Json::String(s)) => Value::Text(s.clone()),
        Some(other) => Value::Text(other.to_string()),
    }
}

impl Store {
    /// Oldest local changes first, at most `limit`.
    pub fn sync_pending(&self, limit: usize) -> Result<Vec<Pending>> {
        let entries: Vec<(i64, String, String)> = {
            let mut st = self.conn.prepare("SELECT seq, tbl, pk FROM sync_outbox ORDER BY seq LIMIT ?1")?;
            let v = st
                .query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        let mut out = Vec::with_capacity(entries.len());
        for (seq, tbl, pk) in entries {
            let Some(spec) = table(&tbl) else {
                self.sync_ack(&[seq])?;
                continue;
            };
            let sql = format!("SELECT {} FROM {} WHERE {} = ?1", spec.cols.join(", "), spec.name, spec.pk);
            let mut st = self.conn.prepare(&sql)?;
            let mut rows = st.query([&pk])?;
            match rows.next()? {
                Some(r) => {
                    let mut row = Map::new();
                    for (i, c) in spec.cols.iter().enumerate() {
                        row.insert(c.to_string(), to_json(r.get_ref(i)?));
                    }
                    out.push(Pending { seq, table: spec.name, row });
                }
                // Rows are soft-deleted, so this only happens if the outbox
                // outlived its row; nothing to send.
                None => {
                    drop(rows);
                    self.sync_ack(&[seq])?;
                }
            }
        }
        Ok(out)
    }

    /// Drop pushed changes. A row changed again since it was read has a new
    /// `seq`, so it stays queued.
    pub fn sync_ack(&self, seqs: &[i64]) -> Result<()> {
        for seq in seqs {
            self.conn.execute("DELETE FROM sync_outbox WHERE seq = ?1", [seq])?;
        }
        Ok(())
    }

    pub fn sync_pending_count(&self) -> Result<usize> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0))? as usize)
    }

    /// Queue every synced row, e.g. on first login so local plans reach the
    /// server.
    pub fn sync_enqueue_all(&self) -> Result<()> {
        self.with_tx(|s| {
            for t in TABLES {
                let filter = if t.name == "settings" { " WHERE key NOT LIKE 'local.%'" } else { "" };
                s.conn.execute(
                    &format!(
                        "INSERT OR REPLACE INTO sync_outbox (tbl, pk)
                         SELECT '{name}', CAST({pk} AS TEXT) FROM {name}{filter}",
                        name = t.name,
                        pk = t.pk,
                    ),
                    [],
                )?;
            }
            Ok(())
        })
    }

    /// Apply rows pulled from the server. A row is written only if it is new
    /// here or its `updated_at` is newer than the local copy. Unknown keys
    /// (`user_id`, `synced_at`) are ignored. Returns how many rows changed.
    pub fn sync_apply(&self, table_name: &str, rows: &[Map<String, Json>]) -> Result<usize> {
        let Some(spec) = table(table_name) else { bail!("unknown table {table_name:?}") };
        let cols = spec.cols.join(", ");
        let marks = (1..=spec.cols.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
        let sets = spec
            .cols
            .iter()
            .filter(|c| **c != spec.pk)
            .map(|c| format!("{c} = excluded.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "INSERT INTO {name} ({cols}) VALUES ({marks})
             ON CONFLICT({pk}) DO UPDATE SET {sets}
             WHERE excluded.updated_at > {name}.updated_at",
            name = spec.name,
            pk = spec.pk,
        );
        self.with_tx(|s| {
            s.conn.execute("UPDATE sync_ctl SET applying = 1", [])?;
            let mut changed = 0;
            for row in rows {
                if spec.name == "settings"
                    && row.get("key").and_then(Json::as_str).is_some_and(|k| k.starts_with("local."))
                {
                    continue;
                }
                let values: Vec<Value> = spec.cols.iter().map(|c| from_json(row.get(*c))).collect();
                changed += s.conn.execute(&sql, params_from_iter(values))?;
            }
            s.conn.execute("UPDATE sync_ctl SET applying = 0", [])?;
            Ok(changed)
        })
    }

    pub fn sync_state(&self, key: &str) -> Result<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row("SELECT value FROM sync_state WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_sync_state(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Forget sync progress (e.g. on logout), so the next login starts over.
    pub fn sync_reset(&self) -> Result<()> {
        self.conn.execute_batch("DELETE FROM sync_state; DELETE FROM sync_outbox;")?;
        Ok(())
    }
}

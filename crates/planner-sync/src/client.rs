//! Push and pull over Supabase's REST API (PostgREST).

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat};
use planner_core::sync::{self, Pending};
use planner_core::Store;
use serde_json::{Map, Value};

use crate::auth::{self, check};
use crate::config::{Session, SyncConfig};

const PUSH_BATCH: usize = 500;
const PULL_PAGE: usize = 1000;
/// Each pull re-reads this much before the last seen `synced_at`, so a row
/// committed late by a slower transaction isn't skipped. Re-applying is a no-op.
const PULL_OVERLAP_SECS: i64 = 30;

pub struct Client {
    pub cfg: SyncConfig,
    pub session: Session,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(cfg: SyncConfig, session: Session) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build();
        Self { cfg, session, agent }
    }

    /// The saved config and session, if logged in.
    pub fn load() -> Result<Option<Self>> {
        match (SyncConfig::load()?, Session::load()?) {
            (Some(cfg), Some(session)) => Ok(Some(Self::new(cfg, session))),
            _ => Ok(None),
        }
    }

    /// An access token valid for at least `min_secs` more, refreshing (and
    /// saving) the session if needed.
    pub fn token_valid_for(&mut self, min_secs: i64) -> Result<String> {
        if chrono::Utc::now().timestamp() + min_secs >= self.session.expires_at {
            self.session = auth::refresh(&self.cfg, &self.session)?;
            self.session.save()?;
        }
        Ok(self.session.access_token.clone())
    }

    fn rest_url(&self, table: &str) -> String {
        format!("{}/rest/v1/{table}", self.cfg.url)
    }

    fn authed(&mut self, req: ureq::Request) -> Result<ureq::Request> {
        let token = self.token_valid_for(60)?;
        Ok(req.set("apikey", &self.cfg.anon_key).set("Authorization", &format!("Bearer {token}")))
    }

    /// A 401 means the token went bad early; refresh it next time.
    fn note_auth_error(&mut self, e: &anyhow::Error) {
        if format!("{e:#}").contains("HTTP 401") {
            self.session.expires_at = 0;
        }
    }

    /// Send queued local changes. Returns how many rows were sent.
    pub fn push(&mut self, store: &Store) -> Result<usize> {
        let mut total = 0;
        // Tables the server doesn't have yet (schema not updated): their
        // rows wait in the outbox, everything else still syncs.
        let mut missing: Vec<&str> = Vec::new();
        loop {
            let batch = store.sync_pending_except(PUSH_BATCH, &missing)?;
            if batch.is_empty() {
                return Ok(total);
            }
            let mut by_table: BTreeMap<&str, Vec<&Pending>> = BTreeMap::new();
            for p in &batch {
                by_table.entry(p.table).or_default().push(p);
            }
            for (table, items) in by_table {
                let spec = sync::table(table).context("unknown table in outbox")?;
                let rows: Vec<Value> = items
                    .iter()
                    .map(|p| {
                        let mut row = p.row.clone();
                        row.insert("user_id".into(), Value::String(self.session.user_id.clone()));
                        Value::Object(row)
                    })
                    .collect();
                let url = format!("{}?on_conflict=user_id,{}", self.rest_url(table), spec.pk);
                let req = self.agent.post(&url).set("Prefer", "resolution=merge-duplicates,return=minimal");
                let req = self.authed(req)?;
                if let Err(e) = check(req.send_json(Value::Array(rows))).with_context(|| format!("pushing {table}")) {
                    if table_missing(&e) {
                        warn_missing(table);
                        missing.push(spec.name);
                        continue;
                    }
                    self.note_auth_error(&e);
                    return Err(e);
                }
                store.sync_ack(&items.iter().map(|p| p.seq).collect::<Vec<_>>())?;
                total += items.len();
            }
        }
    }

    /// Fetch rows changed on the server since the last pull and merge them.
    /// Returns how many local rows changed.
    pub fn pull(&mut self, store: &Store) -> Result<usize> {
        let mut changed = 0;
        for spec in sync::TABLES {
            let key = format!("pull.{}", spec.name);
            let mut since = match store.sync_state(&key)? {
                Some(ts) => overlap(&ts),
                None => "1970-01-01T00:00:00Z".to_string(),
            };
            loop {
                let req = self
                    .agent
                    .get(&self.rest_url(spec.name))
                    .query("select", "*")
                    .query("synced_at", &format!("gt.{since}"))
                    .query("order", "synced_at.asc")
                    .query("limit", &PULL_PAGE.to_string());
                let req = self.authed(req)?;
                let resp = match check(req.call()).with_context(|| format!("pulling {}", spec.name)) {
                    Ok(r) => r,
                    Err(e) if table_missing(&e) => {
                        warn_missing(spec.name);
                        break;
                    }
                    Err(e) => {
                        self.note_auth_error(&e);
                        return Err(e);
                    }
                };
                let rows: Vec<Map<String, Value>> = resp.into_json()?;
                let Some(last) = rows.last().and_then(|r| r.get("synced_at")).and_then(Value::as_str) else {
                    break;
                };
                let last = last.to_string();
                changed += store.sync_apply(spec.name, &rows)?;
                store.set_sync_state(&key, &last)?;
                if rows.len() < PULL_PAGE {
                    break;
                }
                since = last;
            }
        }
        Ok(changed)
    }
}

/// PostgREST answers 404 for a table that isn't in the database.
fn table_missing(e: &anyhow::Error) -> bool {
    format!("{e:#}").contains("(HTTP 404)")
}

/// Said once per run, not on every sync.
fn warn_missing(table: &str) {
    static WARNED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    if let Ok(mut w) = WARNED.lock() {
        if !w.iter().any(|t| t == table) {
            w.push(table.to_string());
            eprintln!("sync: the server has no {table:?} table yet; run supabase/schema.sql again. Other tables still sync.");
        }
    }
}

/// The pull watermark moved back by `PULL_OVERLAP_SECS`.
fn overlap(ts: &str) -> String {
    match DateTime::parse_from_rfc3339(ts) {
        Ok(t) => (t - chrono::Duration::seconds(PULL_OVERLAP_SECS)).to_rfc3339_opts(SecondsFormat::Micros, true),
        Err(_) => ts.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::overlap;

    #[test]
    fn watermark_overlap() {
        assert_eq!(overlap("2026-11-02T09:00:10.123456+00:00"), "2026-11-02T08:59:40.123456Z");
        assert_eq!(overlap("garbage"), "garbage");
    }
}

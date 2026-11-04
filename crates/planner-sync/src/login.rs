//! Signing in from any front end (the `plannerd login` CLI, the app's
//! Settings screen).

use anyhow::{bail, Result};
use planner_core::sync::TABLES;
use planner_core::Store;
use serde::Serialize;

use crate::config::{config_path, session_path};
use crate::{auth, Client, Session, SyncConfig};

pub enum LoginOutcome {
    /// Signed in and synced once.
    LoggedIn { sent: usize, received: usize },
    /// Sign-up needs the confirmation email clicked first.
    ConfirmEmail,
}

/// Sign in (or sign up), save the session, and do a first push and pull.
pub fn login(store: &Store, cfg: SyncConfig, email: &str, password: &str, signup: bool) -> Result<LoginOutcome> {
    if !cfg.url.starts_with("https://") && !cfg.url.starts_with("http://") {
        bail!("the project URL should look like https://<project>.supabase.co");
    }
    if email.trim().is_empty() {
        bail!("no email given");
    }
    let session = if signup {
        match auth::sign_up(&cfg, email.trim(), password)? {
            Some(s) => s,
            None => {
                cfg.save()?;
                return Ok(LoginOutcome::ConfirmEmail);
            }
        }
    } else {
        auth::sign_in(&cfg, email.trim(), password)?
    };
    cfg.save()?;
    session.save()?;

    // First login on this device (or a different account): send everything
    // local and pull everything remote.
    if store.sync_state("account")?.as_deref() != Some(session.user_id.as_str()) {
        store.sync_reset()?;
        store.sync_enqueue_all()?;
        store.set_sync_state("account", &session.user_id)?;
    }
    let mut c = Client::new(cfg, session);
    let sent = c.push(store)?;
    let received = c.pull(store)?;
    Ok(LoginOutcome::LoggedIn { sent, received })
}

/// Forget the session. Plans stay on the device.
pub fn logout() -> Result<()> {
    Session::delete()
}

#[derive(Clone, Debug, Serialize)]
pub struct SyncStatus {
    /// Project URL, if sync was ever set up.
    pub url: Option<String>,
    /// Signed-in account, if any.
    pub account: Option<String>,
    /// Local changes not sent yet.
    pub pending: usize,
    /// When the newest change from the server arrived (server time).
    pub last_received: Option<String>,
    pub files: String,
}

pub fn status(store: &Store) -> Result<SyncStatus> {
    let url = SyncConfig::load()?.map(|c| c.url);
    let account = Session::load()?.map(|s| s.email.unwrap_or(s.user_id));
    let last_received =
        TABLES.iter().filter_map(|t| store.sync_state(&format!("pull.{}", t.name)).ok().flatten()).max();
    Ok(SyncStatus {
        url,
        account,
        pending: store.sync_pending_count()?,
        last_received,
        files: format!("{} · {}", config_path().display(), session_path().display()),
    })
}

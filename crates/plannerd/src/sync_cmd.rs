//! `plannerd login | logout | sync | sync-status`.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{bail, Context, Result};
use planner_core::sync::TABLES;
use planner_core::Store;
use planner_sync::config::{config_path, session_path};
use planner_sync::{auth, Client, Session, SyncConfig};

use crate::client;

fn read_line(prompt: &str) -> Result<String> {
    if std::io::stdin().is_terminal() {
        eprint!("{prompt}");
        std::io::stderr().flush()?;
    }
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn read_password() -> Result<String> {
    if std::io::stdin().is_terminal() {
        Ok(rpassword::prompt_password("Password: ")?)
    } else {
        // Piped in, e.g. from a password manager.
        read_line("")
    }
}

pub fn login(
    store: &Store,
    url: Option<String>,
    key: Option<String>,
    email: Option<String>,
    signup: bool,
) -> Result<()> {
    let saved = SyncConfig::load()?;
    let cfg = match (url, key) {
        (Some(u), Some(k)) => SyncConfig::new(&u, &k),
        (None, None) => saved.context(
            "first login needs the project's --url and --key (Supabase → Project Settings → API: Project URL and anon public key)",
        )?,
        (u, k) => {
            let saved = saved.context("give both --url and --key")?;
            SyncConfig::new(&u.unwrap_or(saved.url), &k.unwrap_or(saved.anon_key))
        }
    };
    if !cfg.url.starts_with("https://") && !cfg.url.starts_with("http://") {
        bail!("--url should look like https://<project>.supabase.co");
    }
    let email = match email {
        Some(e) => e,
        None => read_line("Email: ")?,
    };
    if email.is_empty() {
        bail!("no email given");
    }
    let password = read_password()?;

    let session = if signup {
        match auth::sign_up(&cfg, &email, &password)? {
            Some(s) => s,
            None => {
                cfg.save()?;
                outln!("Account created. Click the link in the confirmation email, then run `plannerd login`.");
                return Ok(());
            }
        }
    } else {
        auth::sign_in(&cfg, &email, &password)?
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
    client::poke();
    outln!("Logged in as {email}. Sent {sent} rows, received {received} changes.");
    outln!("A running plannerd starts syncing within a minute.");
    Ok(())
}

pub fn logout() -> Result<()> {
    Session::delete()?;
    outln!("Logged out. Plans stay on this device; restart plannerd to stop syncing:");
    outln!("  systemctl --user restart plannerd");
    Ok(())
}

pub fn sync_now(store: &Store) -> Result<()> {
    let mut c = Client::load()?.context("not logged in (run `plannerd login`)")?;
    let sent = c.push(store)?;
    let received = c.pull(store)?;
    client::poke();
    outln!("Sent {sent} rows, received {received} changes.");
    Ok(())
}

pub fn status(store: &Store) -> Result<()> {
    match SyncConfig::load()? {
        Some(cfg) => outln!("Project:  {}", cfg.url),
        None => {
            outln!("Sync isn't set up. Run: plannerd login --url <project URL> --key <anon key>");
            return Ok(());
        }
    }
    match Session::load()? {
        Some(s) => outln!("Account:  {}", s.email.as_deref().unwrap_or(&s.user_id)),
        None => outln!("Account:  logged out"),
    }
    outln!("Waiting:  {} local changes to send", store.sync_pending_count()?);
    let last = TABLES
        .iter()
        .filter_map(|t| store.sync_state(&format!("pull.{}", t.name)).ok().flatten())
        .max();
    outln!("Server:   last change received {}", last.as_deref().unwrap_or("never"));
    outln!("Files:    {} · {}", config_path().display(), session_path().display());
    Ok(())
}

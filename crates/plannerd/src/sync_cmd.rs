//! `plannerd login | logout | sync | sync-status`.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{Context, Result};
use planner_core::Store;
use planner_sync::{Client, LoginOutcome, SyncConfig};

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
    let email = match email {
        Some(e) => e,
        None => read_line("Email: ")?,
    };
    let password = read_password()?;

    match planner_sync::login(store, cfg, &email, &password, signup)? {
        LoginOutcome::ConfirmEmail => {
            outln!("Account created. Click the link in the confirmation email, then run `plannerd login`.");
        }
        LoginOutcome::LoggedIn { sent, received } => {
            client::poke();
            outln!("Logged in as {email}. Sent {sent} rows, received {received} changes.");
            outln!("A running plannerd starts syncing within a minute.");
        }
    }
    Ok(())
}

pub fn logout() -> Result<()> {
    planner_sync::logout()?;
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
    let st = planner_sync::login::status(store)?;
    let Some(url) = st.url else {
        outln!("Sync isn't set up. Run: plannerd login --url <project URL> --key <anon key>");
        return Ok(());
    };
    outln!("Project:  {url}");
    outln!("Account:  {}", st.account.as_deref().unwrap_or("logged out"));
    outln!("Waiting:  {} local changes to send", st.pending);
    outln!("Server:   last change received {}", st.last_received.as_deref().unwrap_or("never"));
    outln!("Files:    {}", st.files);
    Ok(())
}

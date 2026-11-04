//! Supabase sync (milestone 4).
//!
//! - [`config`]: project URL/key (`~/.config/timlo/sync.toml`) and the login
//!   session (`~/.local/share/timlo/session.json`, mode 0600).
//! - [`auth`]: email + password sign-in, sign-up and token refresh.
//! - [`client`]: push the local outbox and pull rows newer than the last pull.
//! - [`realtime`]: Supabase Realtime listener; any change triggers a pull.
//! - [`login`]: sign-in, sign-out and status for the CLI and the app.
//! - [`worker`]: the background loop the daemon (or the Android service) runs.
//!
//! Each device keeps working from its local SQLite copy; sync only keeps the
//! copies matched, last write wins per row (see `planner_core::sync`).

pub mod auth;
pub mod client;
pub mod config;
pub mod login;
pub mod realtime;
pub mod worker;

pub use client::Client;
pub use config::{Session, SyncConfig};
pub use login::{login, logout, LoginOutcome, SyncStatus};
pub use worker::{start, SyncEvent};

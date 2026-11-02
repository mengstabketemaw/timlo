//! Talking to a running daemon over its Unix socket.
//!
//! Protocol: one line per connection — `subscribe` (the daemon then streams a
//! Waybar JSON line on every change) or `refresh` (re-read the database now).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::Duration;

use anyhow::Result;

use planner_core::paths;

use crate::waybar;

pub use planner_core::paths::poke_daemon as poke;

fn emit(line: &str) -> bool {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}").and_then(|_| out.flush()).is_ok()
}

/// Waybar `exec` target: relay the daemon's stream to stdout, reconnecting
/// if it restarts. Returns when Waybar closes the pipe.
pub fn waybar_stream() -> Result<()> {
    let offline = serde_json::to_string(&waybar::offline())?;
    loop {
        if let Ok(mut s) = UnixStream::connect(paths::socket_path()) {
            if s.write_all(b"subscribe\n").is_ok() {
                for line in BufReader::new(s).lines() {
                    let Ok(line) = line else { break };
                    if !emit(&line) {
                        return Ok(());
                    }
                }
            }
        }
        if !emit(&offline) {
            return Ok(());
        }
        thread::sleep(Duration::from_secs(5));
    }
}

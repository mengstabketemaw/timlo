//! File locations. Each can be overridden with an environment variable.

use std::path::PathBuf;

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// `$TIMLO_DB`, else `~/.local/share/timlo/planner.db`.
pub fn db_path() -> PathBuf {
    std::env::var_os("TIMLO_DB").map(PathBuf::from).unwrap_or_else(|| {
        dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("timlo/planner.db")
    })
}

/// `$TIMLO_PLAN`, else `~/.config/timlo/plan.toml`.
pub fn plan_path() -> PathBuf {
    std::env::var_os("TIMLO_PLAN").map(PathBuf::from).unwrap_or_else(|| {
        dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("timlo/plan.toml")
    })
}

/// `$TIMLO_SOCKET`, else `$XDG_RUNTIME_DIR/timlo-plannerd.sock`.
pub fn socket_path() -> PathBuf {
    std::env::var_os("TIMLO_SOCKET").map(PathBuf::from).unwrap_or_else(|| {
        dirs::runtime_dir().unwrap_or_else(std::env::temp_dir).join("timlo-plannerd.sock")
    })
}

/// Ask a running `plannerd` to re-read the database (so Waybar updates at
/// once after a change). Does nothing if it isn't running.
pub fn poke_daemon() {
    #[cfg(unix)]
    {
        use std::io::Write;
        if let Ok(mut s) = std::os::unix::net::UnixStream::connect(socket_path()) {
            let _ = s.write_all(b"refresh\n");
        }
    }
}

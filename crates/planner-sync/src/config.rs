use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// `$TIMLO_SYNC_CONFIG`, else `~/.config/timlo/sync.toml`.
pub fn config_path() -> PathBuf {
    std::env::var_os("TIMLO_SYNC_CONFIG").map(PathBuf::from).unwrap_or_else(|| {
        dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("timlo/sync.toml")
    })
}

/// `$TIMLO_SESSION`, else `~/.local/share/timlo/session.json`.
pub fn session_path() -> PathBuf {
    std::env::var_os("TIMLO_SESSION").map(PathBuf::from).unwrap_or_else(|| {
        dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("timlo/session.json")
    })
}

fn write_private(path: &PathBuf, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path).with_context(|| format!("writing {}", path.display()))?;
    f.write_all(text.as_bytes())?;
    Ok(())
}

/// Which Supabase project to sync with. The anon key is public by design;
/// row-level security is what protects the data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncConfig {
    /// e.g. `https://abcd1234.supabase.co`
    pub url: String,
    pub anon_key: String,
}

impl SyncConfig {
    pub fn new(url: &str, anon_key: &str) -> Self {
        Self { url: url.trim().trim_end_matches('/').to_string(), anon_key: anon_key.trim().to_string() }
    }

    pub fn load() -> Result<Option<Self>> {
        let path = config_path();
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let cfg: Self = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(Some(Self::new(&cfg.url, &cfg.anon_key)))
    }

    pub fn save(&self) -> Result<()> {
        write_private(&config_path(), &toml::to_string(self)?)
    }
}

/// A signed-in Supabase session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    pub user_id: String,
    pub email: Option<String>,
}

impl Session {
    pub fn load() -> Result<Option<Self>> {
        let path = session_path();
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        Ok(Some(serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?))
    }

    pub fn save(&self) -> Result<()> {
        write_private(&session_path(), &serde_json::to_string_pretty(self)?)
    }

    pub fn delete() -> Result<()> {
        let path = session_path();
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }
}

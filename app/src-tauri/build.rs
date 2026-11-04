//! Bakes the Supabase project from the repo's `.env` (PUBLIC_SUPABASE_URL,
//! PUBLIC_SUPABASE_PUBLISHABLE_KEY) into the login form's defaults. Both are
//! public values; nothing else is read from the file.

use std::path::Path;

fn env_file_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.trim().strip_prefix("export ").unwrap_or(line.trim()).split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
    })
}

fn main() {
    let env_path = Path::new("../../.env");
    println!("cargo:rerun-if-changed=../../.env");
    let text = std::fs::read_to_string(env_path).unwrap_or_default();
    for (out, var) in [
        ("TIMLO_DEFAULT_SUPABASE_URL", "PUBLIC_SUPABASE_URL"),
        ("TIMLO_DEFAULT_SUPABASE_KEY", "PUBLIC_SUPABASE_PUBLISHABLE_KEY"),
    ] {
        println!("cargo:rerun-if-env-changed={var}");
        let v = std::env::var(var).ok().or_else(|| env_file_value(&text, var)).unwrap_or_default();
        println!("cargo:rustc-env={out}={v}");
    }
    tauri_build::build()
}

//! Supabase Auth (GoTrue) over REST: email + password.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::config::{Session, SyncConfig};

/// Pull a readable message out of a Supabase error body.
pub(crate) fn api_message(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(body) else { return body.trim().to_string() };
    for key in ["error_description", "msg", "message", "error"] {
        if let Some(s) = v.get(key).and_then(Value::as_str) {
            return s.to_string();
        }
    }
    body.trim().to_string()
}

/// Turn a ureq result into the response, or an error with Supabase's message.
pub(crate) fn check(r: Result<ureq::Response, ureq::Error>) -> Result<ureq::Response> {
    match r {
        Ok(resp) => Ok(resp),
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            bail!("{} (HTTP {code})", api_message(&body))
        }
        Err(e) => Err(anyhow!(e).context("can't reach Supabase")),
    }
}

fn post(cfg: &SyncConfig, path: &str, body: Value) -> Result<Value> {
    let url = format!("{}{path}", cfg.url);
    let resp = check(ureq::post(&url).set("apikey", &cfg.anon_key).send_json(body))?;
    Ok(resp.into_json()?)
}

fn session_from(v: &Value) -> Result<Session> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let expires_at = match v.get("expires_at").and_then(Value::as_i64) {
        Some(t) => t,
        None => chrono::Utc::now().timestamp() + v.get("expires_in").and_then(Value::as_i64).unwrap_or(3600),
    };
    let user = v.get("user").context("no user in auth response")?;
    Ok(Session {
        access_token: s("access_token").context("no access_token in auth response")?,
        refresh_token: s("refresh_token").context("no refresh_token in auth response")?,
        expires_at,
        user_id: user.get("id").and_then(Value::as_str).context("no user id in auth response")?.to_string(),
        email: user.get("email").and_then(Value::as_str).map(str::to_string),
    })
}

pub fn sign_in(cfg: &SyncConfig, email: &str, password: &str) -> Result<Session> {
    let v = post(cfg, "/auth/v1/token?grant_type=password", json!({ "email": email, "password": password }))?;
    session_from(&v)
}

/// Create the account. Returns `None` when Supabase wants the email
/// confirmed first (then sign in after clicking the link).
pub fn sign_up(cfg: &SyncConfig, email: &str, password: &str) -> Result<Option<Session>> {
    let v = post(cfg, "/auth/v1/signup", json!({ "email": email, "password": password }))?;
    if v.get("access_token").is_some() {
        Ok(Some(session_from(&v)?))
    } else {
        Ok(None)
    }
}

pub fn refresh(cfg: &SyncConfig, session: &Session) -> Result<Session> {
    let v = post(cfg, "/auth/v1/token?grant_type=refresh_token", json!({ "refresh_token": session.refresh_token }))
        .context("refreshing login (run `plannerd login` if this keeps failing)")?;
    session_from(&v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_supabase_error_messages() {
        assert_eq!(api_message(r#"{"error":"invalid_grant","error_description":"Invalid login credentials"}"#), "Invalid login credentials");
        assert_eq!(api_message(r#"{"code":400,"msg":"User already registered"}"#), "User already registered");
        assert_eq!(api_message("Bad gateway"), "Bad gateway");
    }

    #[test]
    fn parses_session() {
        let v = json!({
            "access_token": "a", "refresh_token": "r", "expires_in": 3600, "expires_at": 1793600000,
            "user": { "id": "u1", "email": "me@example.com" }
        });
        let s = session_from(&v).unwrap();
        assert_eq!((s.access_token.as_str(), s.user_id.as_str(), s.expires_at), ("a", "u1", 1793600000));
        assert_eq!(s.email.as_deref(), Some("me@example.com"));
    }
}

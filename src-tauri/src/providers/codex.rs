use std::path::PathBuf;

use serde::Deserialize;

use super::{severity_for, ModelUsage, ProviderUsage, USER_AGENT};

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

struct Credentials {
    access_token: String,
    account_id: Option<String>,
}

fn auth_path() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
    Ok(PathBuf::from(home).join(".codex").join("auth.json"))
}

/// Read Codex CLI's ChatGPT OAuth credentials from ~/.codex/auth.json.
///
/// Codex CLI refreshes this file in place, so reading it on every fetch
/// piggybacks on its token refresh. Returns Ok(None) when the file doesn't
/// exist or holds no ChatGPT access token (e.g. API-key auth mode).
fn read_credentials() -> Result<Option<Credentials>, String> {
    let path = auth_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let blob = std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read Codex auth.json: {e}"))?;
    let json: serde_json::Value = serde_json::from_str(&blob)
        .map_err(|e| format!("could not parse Codex auth.json: {e}"))?;

    let tokens = &json["tokens"];
    let Some(access_token) = tokens["access_token"].as_str() else {
        return Ok(None); // API-key mode or signed out
    };
    let account_id = tokens["account_id"].as_str().map(|s| s.to_string());

    Ok(Some(Credentials {
        access_token: access_token.to_string(),
        account_id,
    }))
}

// ---- Raw response (parsed defensively; every field optional) ----

#[derive(Debug, Deserialize, Default)]
struct RawWindow {
    used_percent: Option<f64>,
    reset_at: Option<i64>, // unix seconds
}

#[derive(Debug, Deserialize, Default)]
struct RawRateLimit {
    primary_window: Option<RawWindow>,
    secondary_window: Option<RawWindow>,
}

#[derive(Debug, Deserialize, Default)]
struct RawAdditionalLimit {
    limit_name: Option<String>,
    rate_limit: Option<RawRateLimit>,
}

#[derive(Debug, Deserialize, Default)]
struct RawUsage {
    plan_type: Option<String>,
    rate_limit: Option<RawRateLimit>,
    #[serde(default)]
    additional_rate_limits: Vec<RawAdditionalLimit>,
}

/// Unix seconds → RFC3339, so the frontend treats both providers identically.
fn to_rfc3339(unix_seconds: i64) -> Option<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(unix_seconds, 0).map(|dt| dt.to_rfc3339())
}

fn normalize(raw: RawUsage) -> ProviderUsage {
    let rate_limit = raw.rate_limit.unwrap_or_default();

    // primary_window = rolling 5-hour session, secondary_window = 7-day week.
    let session = rate_limit.primary_window.unwrap_or_default();
    let weekly = rate_limit.secondary_window.unwrap_or_default();

    let session_percent = session.used_percent.unwrap_or(0.0);
    let weekly_percent = weekly.used_percent.unwrap_or(0.0);

    let models: Vec<ModelUsage> = raw
        .additional_rate_limits
        .into_iter()
        .filter_map(|l| {
            let name = l.limit_name?;
            let percent = l.rate_limit?.secondary_window?.used_percent?;
            Some(ModelUsage { name, percent })
        })
        .collect();

    ProviderUsage {
        session_percent,
        session_resets_at: session.reset_at.and_then(to_rfc3339),
        session_severity: severity_for(session_percent),
        weekly_percent,
        weekly_resets_at: weekly.reset_at.and_then(to_rfc3339),
        weekly_severity: severity_for(weekly_percent),
        models,
        plan: raw.plan_type,
    }
}

/// Fetch Codex usage. Ok(None) = no local credentials (provider hidden).
pub async fn fetch() -> Result<Option<ProviderUsage>, String> {
    let Some(creds) = read_credentials()? else {
        return Ok(None);
    };

    let client = reqwest::Client::new();
    let mut request = client
        .get(USAGE_URL)
        .header("Authorization", format!("Bearer {}", creds.access_token))
        .header("User-Agent", USER_AGENT);
    if let Some(account_id) = &creds.account_id {
        request = request.header("chatgpt-account-id", account_id);
    }

    let resp = request
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status();
    if status.as_u16() == 401 {
        return Err("Codex auth expired. Open Codex to refresh your session.".into());
    }
    if !status.is_success() {
        return Err(format!("Codex usage endpoint returned HTTP {}", status.as_u16()));
    }

    let raw: RawUsage = resp
        .json()
        .await
        .map_err(|e| format!("could not parse Codex usage response: {e}"))?;
    Ok(Some(normalize(raw)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_real_shape() {
        let raw: RawUsage = serde_json::from_str(
            r#"{
                "plan_type": "prolite",
                "rate_limit": {
                    "primary_window":   { "used_percent": 8,  "limit_window_seconds": 18000,
                                          "reset_after_seconds": 16734, "reset_at": 1783887076 },
                    "secondary_window": { "used_percent": 91, "limit_window_seconds": 604800,
                                          "reset_after_seconds": 489435, "reset_at": 1784359777 }
                },
                "additional_rate_limits": [
                    { "limit_name": "GPT-5.3-Codex-Spark",
                      "rate_limit": {
                          "primary_window":   { "used_percent": 0, "reset_at": 1783888342 },
                          "secondary_window": { "used_percent": 4, "reset_at": 1784475142 }
                      } }
                ]
            }"#,
        )
        .unwrap();
        let u = normalize(raw);
        assert_eq!(u.session_percent, 8.0);
        assert_eq!(u.weekly_percent, 91.0);
        assert_eq!(u.session_severity, "normal");
        assert_eq!(u.weekly_severity, "critical");
        // unix seconds converted to RFC3339 for the frontend.
        assert_eq!(u.session_resets_at.as_deref(), Some("2026-07-12T20:11:16+00:00"));
        assert_eq!(u.models.len(), 1);
        assert_eq!(u.models[0].name, "GPT-5.3-Codex-Spark");
        assert_eq!(u.models[0].percent, 4.0);
        assert_eq!(u.plan.as_deref(), Some("prolite"));
    }

    #[test]
    fn normalize_missing_fields_defaults_to_zero() {
        let raw: RawUsage = serde_json::from_str("{}").unwrap();
        let u = normalize(raw);
        assert_eq!(u.session_percent, 0.0);
        assert_eq!(u.weekly_percent, 0.0);
        assert_eq!(u.session_severity, "normal");
        assert!(u.session_resets_at.is_none());
        assert!(u.models.is_empty());
    }
}

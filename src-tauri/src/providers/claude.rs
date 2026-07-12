use std::process::Command;

use serde::Deserialize;

use super::{severity_for, ModelUsage, ProviderUsage, USER_AGENT};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

struct Credentials {
    access_token: String,
    subscription_type: Option<String>,
}

/// Read Claude Code's live OAuth credentials from the macOS Keychain.
///
/// Claude Code keeps the canonical (refreshed) token under the
/// `Claude Code-credentials` generic-password item. Reading it fresh on every
/// fetch means we always use a valid token and never have to run the OAuth
/// refresh flow ourselves. Returns Ok(None) when the item doesn't exist
/// (Claude Code not installed / not signed in).
fn read_credentials() -> Result<Option<Credentials>, String> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .map_err(|e| format!("failed to run `security`: {e}"))?;

    if !output.status.success() {
        return Ok(None);
    }

    let blob = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(blob.trim())
        .map_err(|e| format!("could not parse Claude credentials: {e}"))?;

    let oauth = &json["claudeAiOauth"];
    let access_token = oauth["accessToken"]
        .as_str()
        .ok_or("accessToken missing from Claude credentials")?
        .to_string();
    let subscription_type = oauth["subscriptionType"].as_str().map(|s| s.to_string());

    Ok(Some(Credentials {
        access_token,
        subscription_type,
    }))
}

// ---- Raw response (parsed defensively; every field optional) ----

#[derive(Debug, Deserialize, Default)]
struct RawWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawModelScope {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawScope {
    model: Option<RawModelScope>,
}

#[derive(Debug, Deserialize, Default)]
struct RawLimit {
    kind: Option<String>,
    severity: Option<String>,
    percent: Option<f64>,
    scope: Option<RawScope>,
}

#[derive(Debug, Deserialize, Default)]
struct RawUsage {
    five_hour: Option<RawWindow>,
    seven_day: Option<RawWindow>,
    #[serde(default)]
    limits: Vec<RawLimit>,
}

fn limit_severity(limits: &[RawLimit], kind: &str) -> Option<String> {
    limits
        .iter()
        .find(|l| l.kind.as_deref() == Some(kind))
        .and_then(|l| l.severity.clone())
}

fn normalize(raw: RawUsage, plan: Option<String>) -> ProviderUsage {
    let session_percent = raw
        .five_hour
        .as_ref()
        .and_then(|w| w.utilization)
        .unwrap_or(0.0);
    let session_resets_at = raw.five_hour.as_ref().and_then(|w| w.resets_at.clone());
    let session_severity = limit_severity(&raw.limits, "session")
        .unwrap_or_else(|| severity_for(session_percent));

    let weekly_percent = raw
        .seven_day
        .as_ref()
        .and_then(|w| w.utilization)
        .unwrap_or(0.0);
    let weekly_resets_at = raw.seven_day.as_ref().and_then(|w| w.resets_at.clone());
    let weekly_severity = limit_severity(&raw.limits, "weekly_all")
        .unwrap_or_else(|| severity_for(weekly_percent));

    // Per-model weekly usage rides along as "weekly_scoped" entries in
    // `limits[]`, each carrying its own model display name — this is
    // generic across whatever models the account has used, present or future.
    let models: Vec<ModelUsage> = raw
        .limits
        .iter()
        .filter(|l| l.kind.as_deref() == Some("weekly_scoped"))
        .filter_map(|l| {
            let name = l.scope.as_ref()?.model.as_ref()?.display_name.clone()?;
            let percent = l.percent?;
            Some(ModelUsage { name, percent })
        })
        .collect();

    ProviderUsage {
        session_percent,
        session_resets_at,
        session_severity,
        weekly_percent,
        weekly_resets_at,
        weekly_severity,
        models,
        plan,
    }
}

/// Fetch Claude usage. Ok(None) = no local credentials (provider hidden).
pub async fn fetch() -> Result<Option<ProviderUsage>, String> {
    let Some(creds) = read_credentials()? else {
        return Ok(None);
    };

    let client = reqwest::Client::new();
    let resp = client
        .get(USAGE_URL)
        .header("Authorization", format!("Bearer {}", creds.access_token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status();
    if status.as_u16() == 401 {
        return Err("Claude auth expired. Open Claude Code to refresh your session.".into());
    }
    if !status.is_success() {
        return Err(format!("Claude usage endpoint returned HTTP {}", status.as_u16()));
    }

    let raw: RawUsage = resp
        .json()
        .await
        .map_err(|e| format!("could not parse Claude usage response: {e}"))?;
    Ok(Some(normalize(raw, creds.subscription_type)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_real_shape() {
        let raw: RawUsage = serde_json::from_str(
            r#"{
                "five_hour":  { "utilization": 11.0, "resets_at": "2026-06-29T13:19:59Z" },
                "seven_day":  { "utilization": 8.0,  "resets_at": "2026-07-05T02:59:59Z" },
                "limits": [
                    { "kind": "session",    "severity": "normal" },
                    { "kind": "weekly_all", "severity": "warning" },
                    { "kind": "weekly_scoped", "percent": 2,
                      "scope": { "model": { "display_name": "Sonnet" } } },
                    { "kind": "weekly_scoped", "percent": 3,
                      "scope": { "model": { "display_name": "Fable" } } }
                ]
            }"#,
        )
        .unwrap();
        let u = normalize(raw, Some("max".into()));
        assert_eq!(u.session_percent, 11.0);
        assert_eq!(u.weekly_percent, 8.0);
        // weekly severity comes from limits[], overriding the numeric band.
        assert_eq!(u.weekly_severity, "warning");
        assert_eq!(u.session_severity, "normal");
        assert_eq!(u.models.len(), 2);
        assert_eq!(u.models[0].name, "Sonnet");
        assert_eq!(u.models[1].name, "Fable");
        assert_eq!(u.plan.as_deref(), Some("max"));
    }

    #[test]
    fn normalize_missing_fields_defaults_to_zero() {
        let raw: RawUsage = serde_json::from_str("{}").unwrap();
        let u = normalize(raw, None);
        assert_eq!(u.session_percent, 0.0);
        assert_eq!(u.weekly_percent, 0.0);
        assert_eq!(u.session_severity, "normal");
        assert!(u.models.is_empty());
    }
}

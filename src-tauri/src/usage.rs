use crate::keychain::Credentials;
use serde::{Deserialize, Serialize};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

// ---- Raw response (parsed defensively; every field optional) ----

#[derive(Debug, Deserialize, Default)]
struct RawWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawLimit {
    kind: Option<String>,
    severity: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawUsage {
    five_hour: Option<RawWindow>,
    seven_day: Option<RawWindow>,
    seven_day_opus: Option<RawWindow>,
    seven_day_sonnet: Option<RawWindow>,
    #[serde(default)]
    limits: Vec<RawLimit>,
}

// ---- Normalized usage handed to the frontend / tray ----

#[derive(Debug, Clone, Serialize)]
pub struct ModelUsage {
    pub name: String,
    pub percent: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    pub session_percent: f64,
    pub session_resets_at: Option<String>,
    pub session_severity: String,
    pub weekly_percent: f64,
    pub weekly_resets_at: Option<String>,
    pub weekly_severity: String,
    pub models: Vec<ModelUsage>,
    pub subscription_type: Option<String>,
}

/// Map a utilization percentage to a severity band.
fn severity_for(percent: f64) -> String {
    if percent >= 90.0 {
        "critical"
    } else if percent >= 70.0 {
        "warning"
    } else {
        "normal"
    }
    .to_string()
}

fn limit_severity(limits: &[RawLimit], kind: &str) -> Option<String> {
    limits
        .iter()
        .find(|l| l.kind.as_deref() == Some(kind))
        .and_then(|l| l.severity.clone())
}

fn normalize(raw: RawUsage, subscription_type: Option<String>) -> Usage {
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

    let mut models = Vec::new();
    if let Some(u) = raw.seven_day_opus.as_ref().and_then(|w| w.utilization) {
        models.push(ModelUsage {
            name: "Opus".into(),
            percent: u,
        });
    }
    if let Some(u) = raw.seven_day_sonnet.as_ref().and_then(|w| w.utilization) {
        models.push(ModelUsage {
            name: "Sonnet".into(),
            percent: u,
        });
    }

    Usage {
        session_percent,
        session_resets_at,
        session_severity,
        weekly_percent,
        weekly_resets_at,
        weekly_severity,
        models,
        subscription_type,
    }
}

/// The worse of the session/weekly severities — drives the menubar color.
pub fn worst_severity(u: &Usage) -> &'static str {
    let rank = |s: &str| match s {
        "critical" => 2,
        "warning" => 1,
        _ => 0,
    };
    match rank(&u.session_severity).max(rank(&u.weekly_severity)) {
        2 => "critical",
        1 => "warning",
        _ => "normal",
    }
}

/// Fetch and normalize the current usage from Anthropic's oauth/usage endpoint.
pub async fn fetch_usage(creds: &Credentials) -> Result<Usage, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(USAGE_URL)
        .header("Authorization", format!("Bearer {}", creds.access_token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-usage-menubar")
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status();
    if status.as_u16() == 401 {
        return Err("Authentication expired. Open Claude Code to refresh your session.".into());
    }
    if !status.is_success() {
        return Err(format!("usage endpoint returned HTTP {}", status.as_u16()));
    }

    let raw: RawUsage = resp
        .json()
        .await
        .map_err(|e| format!("could not parse usage response: {e}"))?;
    Ok(normalize(raw, creds.subscription_type.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_bands() {
        assert_eq!(severity_for(0.0), "normal");
        assert_eq!(severity_for(69.9), "normal");
        assert_eq!(severity_for(70.0), "warning");
        assert_eq!(severity_for(89.9), "warning");
        assert_eq!(severity_for(90.0), "critical");
        assert_eq!(severity_for(100.0), "critical");
    }

    #[test]
    fn normalize_real_shape() {
        let raw: RawUsage = serde_json::from_str(
            r#"{
                "five_hour":  { "utilization": 11.0, "resets_at": "2026-06-29T13:19:59Z" },
                "seven_day":  { "utilization": 8.0,  "resets_at": "2026-07-05T02:59:59Z" },
                "seven_day_opus": null,
                "seven_day_sonnet": { "utilization": 2.0, "resets_at": "2026-07-05T02:59:59Z" },
                "limits": [
                    { "kind": "session",    "severity": "normal" },
                    { "kind": "weekly_all", "severity": "warning" }
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
        assert_eq!(u.models.len(), 1);
        assert_eq!(u.models[0].name, "Sonnet");
        assert_eq!(u.subscription_type.as_deref(), Some("max"));
        assert_eq!(worst_severity(&u), "warning");
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

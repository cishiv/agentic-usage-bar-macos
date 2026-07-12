pub mod claude;
pub mod codex;

use serde::Serialize;

pub const USER_AGENT: &str = "agentic-usage-bar-macos";

/// Providers the app knows about, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelUsage {
    pub name: String,
    pub percent: f64,
}

/// Normalized usage — identical shape regardless of provider.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderUsage {
    pub session_percent: f64,
    pub session_resets_at: Option<String>, // RFC3339
    pub session_severity: String,
    pub weekly_percent: f64,
    pub weekly_resets_at: Option<String>,
    pub weekly_severity: String,
    pub models: Vec<ModelUsage>,
    pub plan: Option<String>,
}

/// One provider's fetch outcome. Only providers with local credentials are
/// included in a snapshot at all; a provider that has credentials but failed
/// to fetch carries an `error` (and keeps the last good `usage` if we had one).
#[derive(Debug, Clone, Serialize)]
pub struct ProviderSnapshot {
    pub provider: Provider,
    pub usage: Option<ProviderUsage>,
    pub error: Option<String>,
}

/// Map a utilization percentage to a severity band.
pub fn severity_for(percent: f64) -> String {
    if percent >= 90.0 {
        "critical"
    } else if percent >= 70.0 {
        "warning"
    } else {
        "normal"
    }
    .to_string()
}

fn severity_rank(s: &str) -> u8 {
    match s {
        "critical" => 2,
        "warning" => 1,
        _ => 0,
    }
}

/// The worst severity across all providers that have usage — drives the tray icon.
pub fn worst_severity(snapshots: &[ProviderSnapshot]) -> &'static str {
    let worst = snapshots
        .iter()
        .filter(|s| s.error.is_none()) // stale carry-over usage doesn't color the icon
        .filter_map(|s| s.usage.as_ref())
        .flat_map(|u| {
            [
                severity_rank(&u.session_severity),
                severity_rank(&u.weekly_severity),
            ]
        })
        .max()
        .unwrap_or(0);
    match worst {
        2 => "critical",
        1 => "warning",
        _ => "normal",
    }
}

/// Fetch every provider that has local credentials, concurrently.
/// Providers without credentials are omitted entirely.
pub async fn fetch_all() -> Vec<ProviderSnapshot> {
    let (claude, codex) = tokio::join!(claude::fetch(), codex::fetch());
    [
        (Provider::Claude, claude),
        (Provider::Codex, codex),
    ]
    .into_iter()
    .filter_map(|(provider, result)| match result {
        Ok(Some(usage)) => Some(ProviderSnapshot {
            provider,
            usage: Some(usage),
            error: None,
        }),
        Ok(None) => None, // no credentials — provider not shown at all
        Err(e) => Some(ProviderSnapshot {
            provider,
            usage: None,
            error: Some(e),
        }),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(session_severity: &str, weekly_severity: &str) -> ProviderUsage {
        ProviderUsage {
            session_percent: 0.0,
            session_resets_at: None,
            session_severity: session_severity.into(),
            weekly_percent: 0.0,
            weekly_resets_at: None,
            weekly_severity: weekly_severity.into(),
            models: vec![],
            plan: None,
        }
    }

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
    fn worst_severity_spans_providers_and_windows() {
        let snapshots = vec![
            ProviderSnapshot {
                provider: Provider::Claude,
                usage: Some(usage("normal", "normal")),
                error: None,
            },
            ProviderSnapshot {
                provider: Provider::Codex,
                usage: Some(usage("normal", "critical")),
                error: None,
            },
        ];
        assert_eq!(worst_severity(&snapshots), "critical");
    }

    #[test]
    fn worst_severity_ignores_errored_providers() {
        let snapshots = vec![ProviderSnapshot {
            provider: Provider::Codex,
            usage: Some(usage("critical", "critical")), // stale carry-over
            error: Some("auth expired".into()),
        }];
        assert_eq!(worst_severity(&snapshots), "normal");
    }
}

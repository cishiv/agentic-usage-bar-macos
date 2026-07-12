use crate::providers::{worst_severity, Provider, ProviderSnapshot};
use crate::AppState;
use tauri::{
    image::Image,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

pub const TRAY_ID: &str = "main-tray";

const IDLE_TITLE: &str = "–";

fn provider_label(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "C",
        Provider::Codex => "X",
    }
}

/// One segment per provider: `C 11·8` (session·weekly), `C –·–` on error.
fn format_title(snapshots: &[ProviderSnapshot]) -> String {
    if snapshots.is_empty() {
        return IDLE_TITLE.to_string();
    }
    snapshots
        .iter()
        .map(|s| {
            let label = provider_label(s.provider);
            // An errored provider shows –·– even if a stale usage is carried
            // along for the popover.
            match (&s.error, &s.usage) {
                (None, Some(u)) => format!(
                    "{label} {}·{}",
                    u.session_percent.round() as i64,
                    u.weekly_percent.round() as i64
                ),
                _ => format!("{label} –·–"),
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

/// Pick the colored gauge icon matching the severity.
fn tray_icon(severity: &str) -> Image<'static> {
    let bytes: &[u8] = match severity {
        "critical" => include_bytes!("../icons/tray/tray-red.png"),
        "warning" => include_bytes!("../icons/tray/tray-amber.png"),
        _ => include_bytes!("../icons/tray/tray-green.png"),
    };
    Image::from_bytes(bytes).expect("bundled tray icon should be valid png")
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon("normal"))
        .icon_as_template(false)
        .title(IDLE_TITLE)
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_popover(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Reflect the latest snapshots in the menubar: `C 11·8  X 8·11` + colored gauge.
pub fn update_tray(app: &AppHandle, snapshots: &[ProviderSnapshot]) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let _ = tray.set_title(Some(format_title(snapshots)));
    let _ = tray.set_icon(Some(tray_icon(worst_severity(snapshots))));
}

/// Open the popover under the menubar icon, or close it if already open.
pub fn toggle_popover(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }

    // If the popover was hidden a moment ago by the blur from *this* same click
    // on the tray icon, treat the click as a close — don't immediately reopen.
    if let Some(state) = app.try_state::<AppState>() {
        if let Some(hidden_at) = *state.last_hidden.lock().unwrap() {
            if hidden_at.elapsed() < std::time::Duration::from_millis(300) {
                return;
            }
        }
    }

    use tauri_plugin_positioner::{Position, WindowExt};
    let _ = window.move_window(Position::TrayBottomCenter);
    let _ = window.show();
    let _ = window.set_focus();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ProviderUsage;

    fn usage(session: f64, weekly: f64) -> ProviderUsage {
        ProviderUsage {
            session_percent: session,
            session_resets_at: None,
            session_severity: "normal".into(),
            weekly_percent: weekly,
            weekly_resets_at: None,
            weekly_severity: "normal".into(),
            models: vec![],
            plan: None,
        }
    }

    #[test]
    fn title_with_both_providers() {
        let snapshots = vec![
            ProviderSnapshot {
                provider: Provider::Claude,
                usage: Some(usage(11.0, 8.0)),
                error: None,
            },
            ProviderSnapshot {
                provider: Provider::Codex,
                usage: Some(usage(8.4, 11.0)),
                error: None,
            },
        ];
        assert_eq!(format_title(&snapshots), "C 11·8  X 8·11");
    }

    #[test]
    fn title_with_errored_provider() {
        let snapshots = vec![
            ProviderSnapshot {
                provider: Provider::Claude,
                usage: Some(usage(11.0, 8.0)),
                error: None,
            },
            ProviderSnapshot {
                provider: Provider::Codex,
                usage: Some(usage(8.0, 11.0)), // stale carry-over still shows –·–
                error: Some("auth expired".into()),
            },
        ];
        assert_eq!(format_title(&snapshots), "C 11·8  X –·–");
    }

    #[test]
    fn title_with_no_providers() {
        assert_eq!(format_title(&[]), "–");
    }
}

use crate::usage::{worst_severity, Usage};
use crate::AppState;
use tauri::{
    image::Image,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

pub const TRAY_ID: &str = "main-tray";

const IDLE_TITLE: &str = "S –  W –";

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

/// Reflect the latest usage in the menubar: `S 11%  W 8%` + colored gauge.
pub fn update_tray(app: &AppHandle, usage: &Usage) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let title = format!(
        "S {}%  W {}%",
        usage.session_percent.round() as i64,
        usage.weekly_percent.round() as i64
    );
    let _ = tray.set_title(Some(title));
    let _ = tray.set_icon(Some(tray_icon(worst_severity(usage))));
}

/// Show a neutral placeholder when we have no usage (error / not logged in).
pub fn set_tray_idle(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_title(Some(IDLE_TITLE.to_string()));
    }
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

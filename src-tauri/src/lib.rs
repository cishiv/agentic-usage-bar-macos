mod keychain;
mod tray;
mod usage;

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use usage::Usage;

/// Shared app state: the latest snapshot plus a marker used to make the
/// tray-click toggle behave correctly against the popover's blur-to-hide.
pub struct AppState {
    snapshot: Mutex<UsageSnapshot>,
    pub last_hidden: Mutex<Option<Instant>>,
}

/// What the frontend renders. Keeps the last good `usage` even on error.
#[derive(Clone, Default, Serialize)]
pub struct UsageSnapshot {
    usage: Option<Usage>,
    error: Option<String>,
    fetched_at: Option<i64>, // epoch millis
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Read credentials, fetch usage, update the tray, cache + broadcast the result.
async fn do_refresh(app: &AppHandle) -> UsageSnapshot {
    let result = match keychain::read_credentials() {
        Ok(creds) => usage::fetch_usage(&creds).await,
        Err(e) => Err(e),
    };

    let snapshot = match result {
        Ok(u) => {
            eprintln!(
                "[claude-usage] session {}%  weekly {}%",
                u.session_percent.round(),
                u.weekly_percent.round()
            );
            tray::update_tray(app, &u);
            UsageSnapshot {
                usage: Some(u),
                error: None,
                fetched_at: Some(now_millis()),
            }
        }
        Err(e) => {
            eprintln!("[claude-usage] error: {e}");
            tray::set_tray_idle(app);
            let prev = app
                .state::<AppState>()
                .snapshot
                .lock()
                .unwrap()
                .usage
                .clone();
            UsageSnapshot {
                usage: prev,
                error: Some(e),
                fetched_at: Some(now_millis()),
            }
        }
    };

    *app.state::<AppState>().snapshot.lock().unwrap() = snapshot.clone();
    let _ = app.emit("usage-updated", &snapshot);
    snapshot
}

#[tauri::command]
fn get_usage(state: State<AppState>) -> UsageSnapshot {
    state.snapshot.lock().unwrap().clone()
}

#[tauri::command]
async fn refresh_usage(app: AppHandle) -> UsageSnapshot {
    do_refresh(&app).await
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .manage(AppState {
            snapshot: Mutex::new(UsageSnapshot::default()),
            last_hidden: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![get_usage, refresh_usage, quit_app])
        .on_window_event(|window, event| {
            // Click-away dismiss: hide the popover when it loses focus.
            if let WindowEvent::Focused(false) = event {
                if window.label() == "main" {
                    let _ = window.hide();
                    if let Some(state) = window.app_handle().try_state::<AppState>() {
                        *state.last_hidden.lock().unwrap() = Some(Instant::now());
                    }
                }
            }
        })
        .setup(|app| {
            // Menubar-only app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::build_tray(app.handle())?;

            // Initial fetch immediately, then refresh every 2 minutes.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(120));
                loop {
                    ticker.tick().await; // first tick fires right away
                    do_refresh(&handle).await;
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

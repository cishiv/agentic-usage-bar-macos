mod providers;
mod tray;

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use providers::ProviderSnapshot;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

/// Shared app state: the latest snapshot plus a marker used to make the
/// tray-click toggle behave correctly against the popover's blur-to-hide.
pub struct AppState {
    snapshot: Mutex<UsageSnapshot>,
    pub last_hidden: Mutex<Option<Instant>>,
}

/// What the frontend renders: one entry per provider that has credentials.
#[derive(Clone, Default, Serialize)]
pub struct UsageSnapshot {
    providers: Vec<ProviderSnapshot>,
    fetched_at: Option<i64>, // epoch millis
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Fetch all providers, update the tray, cache + broadcast the result.
/// A provider that errored keeps its last good usage so the popover can show
/// stale data alongside the error.
async fn do_refresh(app: &AppHandle) -> UsageSnapshot {
    let mut snapshots = providers::fetch_all().await;

    {
        let prev = app.state::<AppState>();
        let prev = prev.snapshot.lock().unwrap();
        for s in snapshots.iter_mut().filter(|s| s.usage.is_none()) {
            s.usage = prev
                .providers
                .iter()
                .find(|p| p.provider == s.provider)
                .and_then(|p| p.usage.clone());
        }
    }

    for s in &snapshots {
        match (&s.usage, &s.error) {
            (Some(u), None) => eprintln!(
                "[agentic-usage] {:?}: session {}%  weekly {}%",
                s.provider,
                u.session_percent.round(),
                u.weekly_percent.round()
            ),
            (_, Some(e)) => eprintln!("[agentic-usage] {:?} error: {e}", s.provider),
            _ => {}
        }
    }

    tray::update_tray(app, &snapshots);

    let snapshot = UsageSnapshot {
        providers: snapshots,
        fetched_at: Some(now_millis()),
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

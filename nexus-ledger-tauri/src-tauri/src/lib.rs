//! NexusLedger Tauri application library.
//!
//! This crate wires the React frontend (served from `../dist`) to the
//! Tauri v2 webview, exposes IPC commands to the frontend, and launches
//! the nexus-core API server (Axum on port 8080) as a background tokio
//! task on startup so the embedded webview can communicate with it.
//!
//! Auto-update functionality is provided by `tauri-plugin-updater`:
//!   - On startup (after a 5-second delay) the app checks for updates.
//!   - Periodically (every 4 hours) the app re-checks.
//!   - When an update is available, an `update-available` event is emitted
//!     to the frontend.
//!   - The frontend can invoke `check_for_updates` or
//!     `download_and_install_update` commands to manage the flow.

use std::collections::HashMap;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_updater::UpdaterExt;

// ---------------------------------------------------------------------------
// IPC command types
// ---------------------------------------------------------------------------

/// Information returned to the frontend by [`get_app_info`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct AppInfo {
    /// Application version (matches `tauri.conf.json` → `version`).
    pub version: String,
    /// Product name.
    pub name: String,
    /// Whether the embedded API server (nexus-core) is reachable.
    pub api_server_status: String,
    /// Base URL of the internal API server.
    pub api_url: String,
}

/// Lightweight SurrealDB / API-server health probe result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DbStatus {
    pub reachable: bool,
    pub message: String,
}

/// Update metadata returned by [`check_for_updates`] command and emitted
/// in the `update-available` event.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateInfo {
    /// Version available for download.
    pub version: String,
    /// Currently installed version.
    pub current_version: String,
    /// Release date (ISO-8601 string from the update manifest, if present).
    pub date: Option<String>,
    /// Release notes / changelog body from the update manifest.
    pub body: Option<String>,
}

const APP_VERSION: &str = "1.0.0";
const APP_NAME: &str = "NexusLedger";
const API_BASE_URL: &str = "http://localhost:8080";

/// Interval between automatic update checks (4 hours).
const UPDATE_CHECK_INTERVAL_SECS: u64 = 4 * 3600;
/// Delay before the first automatic update check on startup (5 seconds).
const UPDATE_CHECK_INITIAL_DELAY_SECS: u64 = 5;

// ---------------------------------------------------------------------------
// System-tray icon helpers
// ---------------------------------------------------------------------------

/// RGBA colours used for the tray status-indicator circle.
const ICON_GREEN: [u8; 4] = [34, 197, 94, 255]; // #22C55A
const ICON_YELLOW: [u8; 4] = [250, 204, 21, 255]; // #FACC15
const ICON_RED: [u8; 4] = [239, 68, 68, 255]; // #EF4444

/// Generate a small (32 x 32) RGBA image with a filled circle of the given
/// colour on a transparent background.
///
/// This avoids needing external `.ico` / `.png` asset files while still
/// giving the tray icon a clear, colour-coded status indicator.
fn generate_status_icon(color: [u8; 4]) -> tauri::image::Image<'static> {
    const SIZE: usize = 32;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];

    let centre = (SIZE as f32 - 1.0) / 2.0;
    let radius = SIZE as f32 * 0.38;
    let radius_sq = radius * radius;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - centre;
            let dy = y as f32 - centre;
            let dist_sq = dx * dx + dy * dy;
            let idx = (y * SIZE + x) * 4;
            if dist_sq <= radius_sq {
                rgba[idx] = color[0]; // R
                rgba[idx + 1] = color[1]; // G
                rgba[idx + 2] = color[2]; // B
                rgba[idx + 3] = color[3]; // A
            }
            // Outside the circle stays transparent (already zero-filled).
        }
    }

    tauri::image::Image::new_owned(rgba, SIZE as u32, SIZE as u32)
}

// ---------------------------------------------------------------------------
// Auto-update helpers
// ---------------------------------------------------------------------------

/// Check for updates and emit an `update-available` event to the frontend
/// if a newer version is found.
///
/// This is a fire-and-forget helper used by both the startup / periodic
/// background timer and the manual `check_for_updates` command (the command
/// version also returns the result to the caller).
fn check_for_updates_background(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match app.updater() {
            Ok(updater) => {
                match updater.check().await {
                    Ok(Some(update)) => {
                        tracing::info!(
                            "Update available: {} -> {}",
                            update.current_version,
                            update.version
                        );
                        // Emit to frontend so the UI can show a notification.
                        let _ = app.emit(
                            "update-available",
                            serde_json::json!({
                                "version": update.version,
                                "current_version": update.current_version,
                                "date": update.date.map(|d| d.to_string()),
                                "body": update.body,
                            }),
                        );
                    }
                    Ok(None) => {
                        tracing::debug!("No updates available");
                    }
                    Err(e) => {
                        tracing::warn!("Update check failed: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to initialize updater: {}", e);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Tauri commands (invocable from the frontend via `invoke`)
// ---------------------------------------------------------------------------

/// Return application metadata and a live API-server status check.
#[tauri::command]
async fn get_app_info() -> Result<AppInfo, String> {
    let api_status = check_api_server_status().await;
    Ok(AppInfo {
        version: APP_VERSION.to_string(),
        name: APP_NAME.to_string(),
        api_server_status: if api_status.reachable {
            "online".to_string()
        } else {
            "offline".to_string()
        },
        api_url: API_BASE_URL.to_string(),
    })
}

/// Open a URL in the user's default system browser.
#[tauri::command]
async fn open_external_link(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let shell = app.shell();
    shell
        .open(url, None)
        .map_err(|e| format!("Failed to open URL: {}", e))
}

/// Probe the internal API server (nexus-core) and report whether SurrealDB
/// is reachable through it.
#[tauri::command]
async fn check_db_status() -> Result<DbStatus, String> {
    Ok(check_api_server_status().await)
}

/// Return the environment in which the app is running (`debug` or `release`).
#[tauri::command]
async fn get_environment() -> Result<HashMap<String, String>, String> {
    let mut env = HashMap::new();
    env.insert(
        "mode".to_string(),
        if cfg!(debug_assertions) {
            "debug".to_string()
        } else {
            "release".to_string()
        },
    );
    env.insert("platform".to_string(), std::env::consts::OS.to_string());
    env.insert("arch".to_string(), std::env::consts::ARCH.to_string());
    Ok(env)
}

/// Manually trigger an update check and return the result.
///
/// Returns `Ok(Some(UpdateInfo))` when an update is available, `Ok(None)`
/// when the app is up-to-date, or `Err` when the check itself fails.
#[tauri::command]
async fn check_for_updates(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => {
            tracing::info!(
                "Update available (manual check): {} -> {}",
                update.current_version,
                update.version
            );
            // Also emit the event so any listening UI component reacts.
            let _ = app.emit(
                "update-available",
                serde_json::json!({
                    "version": update.version,
                    "current_version": update.current_version,
                    "date": update.date.map(|d| d.to_string()),
                    "body": update.body,
                }),
            );
            Ok(Some(UpdateInfo {
                version: update.version.clone(),
                current_version: update.current_version.clone(),
                date: update.date.map(|d| d.to_string()),
                body: update.body.clone(),
            }))
        }
        Ok(None) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Download and install the latest update, then restart the application.
///
/// Returns `Err` if no update is available or the download/install fails.
#[tauri::command]
async fn download_and_install_update(app: tauri::AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => {
            tracing::info!("Downloading update {}...", update.version);
            update
                .download_and_install(
                    |_, _| {},
                    || {},
                )
                .await
                .map_err(|e| e.to_string())?;
            tracing::info!("Update installed, restarting...");
            app.restart();
            // app.restart() does not return, but the compiler needs this.
            #[allow(unreachable_code)]
            {
                Ok(())
            }
        }
        Ok(None) => Err("No update available".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// System-tray sync-status command
// ---------------------------------------------------------------------------

/// Update the system-tray icon colour, tooltip, and menu-item label to
/// reflect the current sync status.
///
/// Accepts one of: `"online"`, `"syncing"`, `"offline"`, `"error"`.
///
/// | Status    | Icon  | Menu label                  | Tooltip                 |
/// |-----------|-------|-----------------------------|-------------------------|
/// | online    | green | Sync Status: ● Online       | NexusLedger - Online    |
/// | syncing   | yellow| Sync Status: ● Syncing...   | NexusLedger - Syncing...|
/// | offline   | red   | Sync Status: ● Offline      | NexusLedger - Offline   |
/// | error     | red   | Sync Status: ● Error        | NexusLedger - Error     |
#[tauri::command]
async fn update_sync_status(app: tauri::AppHandle, status: String) -> Result<(), String> {
    let (color, label) = match status.as_str() {
        "online" => (ICON_GREEN, "Online"),
        "syncing" => (ICON_YELLOW, "Syncing..."),
        "offline" => (ICON_RED, "Offline"),
        "error" => (ICON_RED, "Error"),
        other => return Err(format!("Unknown sync status: {}", other)),
    };

    let icon = generate_status_icon(color);
    let tooltip = format!("NexusLedger - {}", label);

    if let Some(tray) = app.tray_by_id("main-tray") {
        // Update the tray icon and tooltip.
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_tooltip(Some(tooltip));

        // Rebuild the menu so the "Sync Status" item reflects the new label.
        // (TrayIcon in Tauri v2 does not expose a menu() getter, so we
        // recreate the menu and call set_menu.)
        let open_item =
            MenuItem::with_id(&app, "open", "Open NexusLedger", true, None::<&str>)
                .map_err(|e| e.to_string())?;
        let sync_now_item =
            MenuItem::with_id(&app, "sync_now", "Sync Now", true, None::<&str>)
                .map_err(|e| e.to_string())?;
        let sync_status_item = MenuItem::with_id(
            &app,
            "sync_status",
            &format!("Sync Status: \u{25cf} {}", label),
            false,
            None::<&str>,
        )
        .map_err(|e| e.to_string())?;
        let separator =
            PredefinedMenuItem::separator(&app).map_err(|e| e.to_string())?;
        let quit_item =
            MenuItem::with_id(&app, "quit", "Quit NexusLedger", true, None::<&str>)
                .map_err(|e| e.to_string())?;

        let new_menu = Menu::with_items(
            &app,
            &[
                &open_item,
                &sync_now_item,
                &sync_status_item,
                &separator,
                &quit_item,
            ],
        )
        .map_err(|e| e.to_string())?;

        let _ = tray.set_menu(Some(new_menu));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Background API-server launcher
// ---------------------------------------------------------------------------

/// Attempt to start the nexus-core API server (Axum) in a background tokio
/// task.
///
/// In a fully integrated build the `backend` crate is linked as a dependency
/// and we call its `run_server()` entry point directly.  Until that wiring
/// is in place we spawn an HTTP client that periodically polls the server
/// to keep the background task alive and report status.
fn spawn_api_server() {
    tokio::spawn(async {
        // Give the webview a moment to initialise, then fire off the first
        // health probe.  Subsequent probes are spaced out to avoid log spam.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        loop {
            match client
                .get(format!("{}/api/health", API_BASE_URL))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    tracing::debug!("nexus-core API server is healthy");
                }
                Ok(resp) => {
                    tracing::warn!(
                        "nexus-core API server responded with status {}",
                        resp.status()
                    );
                }
                Err(_) => {
                    // Server not yet started or not linked — this is expected
                    // during early development before the backend crate is
                    // wired into the Tauri build.
                    tracing::debug!("nexus-core API server not yet reachable");
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });
}

/// Spawn the auto-update background task.
///
/// Waits 5 seconds after startup for the first check, then repeats every
/// 4 hours.  Each check emits an `update-available` event to the frontend
/// if a newer version is found.
fn spawn_update_checker(app: &tauri::AppHandle) {
    let update_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // Initial delay to let the app fully initialise (webview, plugins,
        // API server, etc.) before hitting the update endpoint.
        tokio::time::sleep(std::time::Duration::from_secs(
            UPDATE_CHECK_INITIAL_DELAY_SECS,
        ))
        .await;
        check_for_updates_background(&update_handle);

        // Set up a periodic interval for subsequent checks.
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS));
        // The first tick() fires immediately — skip it since we already
        // did the initial check above.
        interval.tick().await;
        loop {
            interval.tick().await;
            check_for_updates_background(&update_handle);
        }
    });
}

/// Synchronous health-check helper used by both IPC commands.
async fn check_api_server_status() -> DbStatus {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap_or_default();

    match client
        .get(format!("{}/api/health", API_BASE_URL))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => DbStatus {
            reachable: true,
            message: "API server and SurrealDB are online".to_string(),
        },
        Ok(resp) => DbStatus {
            reachable: false,
            message: format!("API server returned status {}", resp.status()),
        },
        Err(e) => DbStatus {
            reachable: false,
            message: format!("Unable to reach API server: {}", e),
        },
    }
}

// ---------------------------------------------------------------------------
// Application entry point
// ---------------------------------------------------------------------------

/// Tauri app builder — called from `main.rs`.
///
/// Registers all IPC commands, initialises the shell plugin (for
/// `open_external_link`), the updater plugin (for auto-updates), and
/// spawns the background API-server task and update-checker on startup.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialise structured logging (tracing) so the tracing::info! / debug!
    // calls throughout this crate produce output.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // Launch the nexus-core API server poller in the background.
            spawn_api_server();

            // Start the auto-update checker (5s initial delay, then every 4h).
            spawn_update_checker(app.handle());

            // --- System tray ------------------------------------------------
            //
            // The tray provides a context menu (Open / Sync Now / status /
            // Quit), a colour-coded status icon, and minimises-to-tray
            // behaviour (the actual hide-on-close is handled by
            // `on_window_event` below).

            // Create menu items individually so we can pass references
            // (Menu::with_items expects &[&dyn IsMenuItem]).
            let open_item =
                MenuItem::with_id(app, "open", "Open NexusLedger", true, None::<&str>)?;
            let sync_now_item =
                MenuItem::with_id(app, "sync_now", "Sync Now", true, None::<&str>)?;
            let sync_status_item = MenuItem::with_id(
                app,
                "sync_status",
                "Sync Status: \u{25cf} Online",
                false,
                None::<&str>,
            )?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit_item =
                MenuItem::with_id(app, "quit", "Quit NexusLedger", true, None::<&str>)?;

            let tray_menu = Menu::with_items(app, &[
                &open_item,
                &sync_now_item,
                &sync_status_item,
                &separator,
                &quit_item,
            ])?;

            let _tray = TrayIconBuilder::with_id("main-tray")
                .menu(&tray_menu)
                .tooltip("NexusLedger - Online")
                .icon(generate_status_icon(ICON_GREEN))
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "open" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "sync_now" => {
                            // Tell the frontend to trigger a sync.
                            let _ = app.emit("sync-requested", ());
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    // Left-click on the tray icon restores the main window.
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Log startup information.
            tracing::info!(
                "NexusLedger v{} starting on {}",
                APP_VERSION,
                std::env::consts::OS
            );
            tracing::info!("API server URL: {}", API_BASE_URL);
            tracing::info!("System tray initialised");

            Ok(())
        })
        .on_window_event(|window, event| {
            // Intercept the close button on the main window: hide it to the
            // system tray instead of terminating the process.  The app can
            // still be fully exited via the tray's "Quit" menu item.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            open_external_link,
            check_db_status,
            get_environment,
            check_for_updates,
            download_and_install_update,
            update_sync_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running NexusLedger Tauri application");
}

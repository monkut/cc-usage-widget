mod usage;

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;
use tauri::{Emitter, Manager};
use tauri::image::Image;
use usage::{get_claude_data_dirs, get_current_usage, UsageStats};


#[tauri::command]
fn get_usage(period: &str) -> Result<UsageStats, String> {
    get_current_usage(period)
}

#[tauri::command]
fn get_data_dirs() -> Vec<String> {
    get_claude_data_dirs()
        .iter()
        .map(|p| p.display().to_string())
        .collect()
}

/// Debug command to check WebKit environment variables
#[tauri::command]
fn get_webkit_env() -> std::collections::HashMap<String, String> {
    let vars = [
        "WEBKIT_DISABLE_COMPOSITING_MODE",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS",
        "WEBKIT_USE_SINGLE_WEB_PROCESS",
        "WEBKIT_DISABLE_GPU",
        "GDK_BACKEND",
    ];
    vars.iter()
        .filter_map(|&name| std::env::var(name).ok().map(|v| (name.to_string(), v)))
        .collect()
}

fn setup_file_watcher(app_handle: tauri::AppHandle) {
    thread::spawn(move || {
        let (tx, rx) = channel();

        let config = Config::default().with_poll_interval(Duration::from_secs(2));

        let mut watcher: RecommendedWatcher = match Watcher::new(tx, config) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Failed to create watcher: {:?}", e);
                return;
            }
        };

        let data_dirs = get_claude_data_dirs();
        for dir in &data_dirs {
            if let Err(e) = watcher.watch(dir, RecursiveMode::Recursive) {
                eprintln!("Failed to watch {:?}: {:?}", dir, e);
            }
        }

        // Debounce: only emit after no events for 500ms
        let mut last_event = std::time::Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(_) => {
                    last_event = std::time::Instant::now();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if last_event.elapsed() >= Duration::from_millis(500)
                        && last_event.elapsed() < Duration::from_millis(1000)
                    {
                        let _ = app_handle.emit("usage-updated", ());
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

fn load_icon() -> Image<'static> {
    let icon_bytes = include_bytes!("../icons/128x128.png");
    let img = image::load_from_memory(icon_bytes).expect("Failed to load icon");
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Image::new_owned(rgba.into_raw(), width, height)
}

/// Monitor system suspend/resume via D-Bus and emit events to trigger app recovery.
/// WebKitGTK's multi-process IPC can break after suspend, so we notify the frontend
/// to restart the app when resume is detected.
#[cfg(target_os = "linux")]
fn setup_suspend_monitor(app_handle: tauri::AppHandle) {
    thread::spawn(move || {
        // Use async runtime for zbus signal handling
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("Failed to create tokio runtime: {:?}", e);
                return;
            }
        };

        rt.block_on(async {
            let conn = match zbus::Connection::system().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to connect to system D-Bus: {:?}", e);
                    return;
                }
            };

            // Subscribe to PrepareForSleep signal from systemd-logind
            let rule = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface("org.freedesktop.login1.Manager")
                .unwrap()
                .member("PrepareForSleep")
                .unwrap()
                .build();

            let mut stream = match zbus::MessageStream::for_match_rule(rule, &conn, None).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to subscribe to D-Bus signal: {:?}", e);
                    return;
                }
            };

            while let Some(msg) = futures_util::StreamExt::next(&mut stream).await {
                if let Ok(msg) = msg {
                    // PrepareForSleep(bool) - false means resuming from sleep
                    if let Ok(body) = msg.body().deserialize::<bool>() {
                        if !body {
                            // System just resumed - emit event to trigger recovery
                            let _ = app_handle.emit("system-resumed", ());
                        }
                    }
                }
            }
        });
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Workarounds for WebKitGTK issues on Linux
    #[cfg(target_os = "linux")]
    {
        // Fix transparent window rendering bug
        // https://github.com/tauri-apps/tauri/issues/10626
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        // Disable WebKit sandbox to prevent "Could not connect to localhost"
        // errors after system suspend/resume. WebKit's multi-process architecture
        // uses IPC that can break when child processes become stale.
        std::env::set_var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");
        // Force single web process to avoid IPC issues between multiple web processes
        std::env::set_var("WEBKIT_USE_SINGLE_WEB_PROCESS", "1");
        // Disable hardware acceleration which can cause issues after suspend/resume
        std::env::set_var("WEBKIT_DISABLE_GPU", "1");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_usage, get_data_dirs, get_webkit_env])
        .setup(move |app| {
            setup_file_watcher(app.handle().clone());

            // Monitor system suspend/resume to handle WebKit process recovery
            #[cfg(target_os = "linux")]
            setup_suspend_monitor(app.handle().clone());

            // Set window icon for Linux
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_icon(load_icon());
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

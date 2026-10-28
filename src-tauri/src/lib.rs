//! ProxyPilot desktop shell: window, tray, IPC and the supervisor thread.
//! The decisions themselves live in `proxypilot-core`.

mod autostart;
mod commands;
mod paths;
mod state;
mod supervisor;
mod tray;
mod updates;

use paths::{AppPaths, find_core};
use proxypilot_core::settings::Settings;
use proxypilot_core::traffic::TrafficLog;
use proxypilot_core::{CoreStatus, UpstreamStatus};
use state::{Runtime, Shared, lock};
use std::sync::{Arc, Mutex};
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let autostarted = std::env::args().any(|arg| arg == "--autostart");
    tauri::Builder::default()
        // First, so a second launch only brings the running window forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let handle = app.handle().clone();
            let paths = AppPaths::resolve(&handle)?;
            let (settings, note) = Settings::load(&paths.settings);
            let (tx, rx) = std::sync::mpsc::channel();
            let core_exe = find_core();
            let unprotected = paths::unprotected(&paths.runtime, core_exe.as_deref());
            let shared = Arc::new(Shared {
                app: handle.clone(),
                core_exe,
                unprotected,
                elevated: proxypilot_core::sys::is_elevated(),
                version: app.package_info().version.to_string(),
                settings: Mutex::new(settings),
                runtime: Mutex::new(Runtime {
                    core_status: CoreStatus::Stopped,
                    core_message: String::new(),
                    core_pid: None,
                    upstream_status: UpstreamStatus::Unknown,
                    env: Vec::new(),
                }),
                traffic: Mutex::new(TrafficLog::default()),
                tx,
                tray: Mutex::new(None),
                clash_api: proxypilot_core::config::ClashApi::generate().ok(),
                update: Mutex::new(None),
                paths,
            });
            shared.log(&format!(
                "启动 {}，管理员权限：{}，sing-box：{}",
                shared.version,
                if shared.elevated { "是" } else { "否" },
                shared.core_exe.as_ref().map_or("缺失".into(), |path| path.display().to_string())
            ));
            if let Some(note) = note {
                shared.log(&note);
            }
            if let Some(path) = &shared.unprotected {
                shared.log(&format!("安装目录权限不安全，普通程序可以修改 {}，已停用分流和开机自启", path.display()));
            }
            let autostart = lock(&shared.settings).autostart;
            if autostart && shared.elevated {
                if shared.unprotected.is_some() {
                    // A logon task that runs a replaceable program elevated is the
                    // very thing to avoid, so remove it rather than refresh it.
                    let result = autostart::disable().and_then(|_| {
                        shared.update_settings(|settings| {
                            settings.autostart = false;
                            Ok(())
                        })
                    });
                    if let Err(error) = result {
                        shared.log(&error);
                    }
                } else if let Err(error) = autostart::enable(&shared.paths.runtime) {
                    // Re-register so the task follows the executable if it moved.
                    shared.log(&error);
                }
            }
            app.manage(shared.clone());
            tray::build(&handle, &shared)?;
            supervisor::spawn(shared.clone(), rx);
            updates::spawn(shared.clone());
            if lock(&shared.settings).routing_enabled {
                shared.request_apply();
            }
            shared.emit();
            if !autostarted {
                tray::show_main(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps routing on; "退出" in the tray really quits.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::detect_upstreams,
            commands::set_upstream,
            commands::test_upstream,
            commands::set_routing,
            commands::set_profile_enabled,
            commands::add_site,
            commands::add_app,
            commands::remove_profile,
            commands::list_processes,
            commands::get_connections,
            commands::update_settings,
            commands::open_data_dir,
            commands::run_diagnostics,
            commands::check_update,
            commands::install_update,
            commands::open_releases,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

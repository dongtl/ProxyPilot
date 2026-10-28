//! Tray icon: status in the tooltip, a pause/resume item, open and quit.

use crate::state::{AppState, Msg, Shared, lock};
use proxypilot_core::{CoreStatus, UpstreamStatus};
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

pub struct TrayHandles {
    toggle: MenuItem<Wry>,
    icon: TrayIcon<Wry>,
}

/// macOS: a Dock icon only while the window is open; the menu bar item stays.
pub fn set_dock(app: &AppHandle, visible: bool) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(if visible { tauri::ActivationPolicy::Regular } else { tauri::ActivationPolicy::Accessory });
    #[cfg(not(target_os = "macos"))]
    let _ = (app, visible);
}

pub fn show_main(app: &AppHandle) {
    set_dock(app, true);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn status_text(state: &AppState) -> &'static str {
    match (state.routing_enabled, state.core_status, state.upstream_status) {
        (false, _, _) => "已暂停，全部直连",
        (true, CoreStatus::Failed, _) => "出错了，点开查看",
        (true, CoreStatus::Starting, _) => "正在启动",
        (true, _, UpstreamStatus::Unreachable) => "代理客户端未运行",
        (true, CoreStatus::Running, _) => "分流已开启",
        (true, CoreStatus::Stopped, _) => "未运行",
    }
}

/// Stop the core on the supervisor, then exit. Runs off the main thread: the
/// supervisor may be waiting on the main thread to update this very tray.
pub fn quit(shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let (ack, done) = std::sync::mpsc::channel();
        if shared.tx.send(Msg::Quit(ack)).is_ok() {
            let _ = done.recv_timeout(Duration::from_secs(5));
        }
        shared.log("退出");
        shared.app.exit(0);
    });
}

pub fn build(app: &AppHandle, shared: &Arc<Shared>) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "开启分流", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "打开主界面", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &open, &separator, &quit_item])?;
    // macOS menu bar items open their menu on a click; Windows tray icons
    // open the window and keep the menu for the right button.
    let mut builder =
        TrayIconBuilder::with_id("main").tooltip("ProxyPilot").menu(&menu).show_menu_on_left_click(cfg!(target_os = "macos"));
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let menu_shared = shared.clone();
    let icon = builder
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "toggle" => {
                let shared = menu_shared.clone();
                let app = app.clone();
                std::thread::spawn(move || {
                    let enable = !lock(&shared.settings).routing_enabled;
                    // Nowhere to show an error from the tray; the window explains it.
                    if crate::commands::set_routing_now(&shared, enable).is_err() {
                        show_main(&app);
                    }
                });
            }
            "open" => show_main(app),
            "quit" => quit(menu_shared.clone()),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    *lock(&shared.tray) = Some(TrayHandles { toggle, icon });
    Ok(())
}

pub fn refresh(shared: &Shared, state: &AppState) {
    if let Some(handles) = lock(&shared.tray).as_ref() {
        let _ = handles.toggle.set_text(if state.routing_enabled { "暂停分流" } else { "开启分流" });
        let _ = handles.icon.set_tooltip(Some(format!("ProxyPilot：{}", status_text(state))));
    }
}

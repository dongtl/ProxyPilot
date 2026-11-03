//! A page that seems to be missing sites: offered in the window and, unless
//! the window is in front or the user turned it off, by a system notification.

use crate::state::{Shared, lock};
use proxypilot_core::model::Suggestion;
use proxypilot_core::traffic::Stall;
use tauri::Manager;
use tauri_plugin_notification::NotificationExt;

/// Called with the stalls of one poll of the connection list.
pub fn found(shared: &Shared, stalls: &[Stall], now_ms: u64) {
    let settings = lock(&shared.settings).clone();
    let added = lock(&shared.suggestions).add(stalls, &settings, now_ms);
    if !added.changed {
        return;
    }
    // A count only: the warn-level log never names the sites people visit.
    let sites: usize = added.notify.iter().map(|suggestion| suggestion.sites.len()).sum();
    if sites > 0 {
        shared.log(&format!("发现 {sites} 个直连打不开的网站，已在窗口里提示"));
    }
    shared.emit();
    if settings.notify_missing_sites && !window_in_front(shared) {
        for suggestion in &added.notify {
            notify(shared, suggestion);
        }
    }
}

fn window_in_front(shared: &Shared) -> bool {
    shared.app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false)
    })
}

fn notify(shared: &Shared, suggestion: &Suggestion) {
    let mut sites = suggestion.sites.iter().take(2).cloned().collect::<Vec<_>>().join("、");
    if suggestion.sites.len() > 2 {
        sites.push_str(" 等");
    }
    let body = format!(
        "“{}”的网页用到的 {sites} 直连打不开。打开 ProxyPilot，点一下就能让它们一起走代理。",
        suggestion.profile_name
    );
    let result = shared.app.notification().builder().title("网页可能显示不全").body(body).show();
    if let Err(error) = result {
        shared.log(&format!("系统通知没能显示：{error}"));
    }
}

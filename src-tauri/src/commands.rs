//! IPC commands. Each is async and does its work on a blocking thread, so a
//! slow scan or probe never freezes the window. Errors are Chinese strings the
//! UI shows as-is.

use crate::state::{AppState, Shared, lock};
use proxypilot_core::catalog::bypass_processes;
use proxypilot_core::detect::{detect, upstream_owner};
use proxypilot_core::probe::probe_upstream;
use proxypilot_core::*;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tauri::State;

type Cmd<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Cmd<T> + Send + 'static) -> Cmd<T> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|error| format!("内部错误：{error}"))?
}

/// Change settings, re-apply if routing is on, and return the new state.
async fn mutate(shared: &Arc<Shared>, change: impl FnOnce(&mut proxypilot_core::settings::Settings) -> Cmd<()> + Send + 'static) -> Cmd<AppState> {
    let shared = shared.clone();
    blocking(move || {
        shared.update_settings(change)?;
        if lock(&shared.settings).routing_enabled {
            shared.request_apply();
        }
        Ok(shared.snapshot())
    })
    .await
}

#[tauri::command]
pub async fn get_state(shared: State<'_, Arc<Shared>>) -> Cmd<AppState> {
    Ok(shared.snapshot())
}

#[tauri::command]
pub async fn detect_upstreams(shared: State<'_, Arc<Shared>>, full: bool) -> Cmd<Vec<Candidate>> {
    let core_pid = lock(&shared.runtime).core_pid;
    let exclude: Vec<u32> = std::iter::once(std::process::id()).chain(core_pid).collect();
    blocking(move || Ok(detect(full, &exclude))).await
}

#[tauri::command]
pub async fn set_upstream(shared: State<'_, Arc<Shared>>, upstream: Upstream) -> Cmd<AppState> {
    let shared = shared.inner().clone();
    blocking(move || {
        let upstream = upstream.normalised()?;
        let client = upstream_owner(&upstream).and_then(|(_, name)| name).map(|name| catalog::client_label(Some(&name)).0);
        shared.update_settings(|settings| {
            settings.upstream = Some(upstream.clone());
            settings.upstream_client = client;
            Ok(())
        })?;
        lock(&shared.runtime).upstream_status = UpstreamStatus::Unknown;
        shared.log(&format!("选择上游 {}:{}", upstream.host, upstream.port));
        let _ = shared.tx.send(crate::state::Msg::Check);
        if lock(&shared.settings).routing_enabled {
            shared.request_apply();
        }
        Ok(shared.snapshot())
    })
    .await
}

#[tauri::command]
pub async fn test_upstream(shared: State<'_, Arc<Shared>>) -> Cmd<ProbeReport> {
    let shared = shared.inner().clone();
    let upstream = lock(&shared.settings).upstream.clone().ok_or("还没有选择代理端口")?;
    blocking(move || {
        let report = probe_upstream(&upstream, Duration::from_secs(15));
        // Only a dead local port says the client is gone; a slow site does not.
        let status = if report.code == "local_unavailable" { UpstreamStatus::Unreachable } else { UpstreamStatus::Ok };
        lock(&shared.runtime).upstream_status = status;
        shared.emit();
        Ok(report)
    })
    .await
}

pub fn set_routing_now(shared: &Shared, enabled: bool) -> Cmd<AppState> {
    if enabled && lock(&shared.settings).upstream.is_none() {
        return Err("请先选择代理端口".into());
    }
    shared.update_settings(|settings| {
        settings.routing_enabled = enabled;
        Ok(())
    })?;
    shared.log(if enabled { "开启分流" } else { "暂停分流" });
    shared.request_apply();
    let state = shared.snapshot();
    shared.emit();
    Ok(state)
}

#[tauri::command]
pub async fn set_routing(shared: State<'_, Arc<Shared>>, enabled: bool) -> Cmd<AppState> {
    let shared = shared.inner().clone();
    blocking(move || set_routing_now(&shared, enabled)).await
}

#[tauri::command]
pub async fn set_profile_enabled(shared: State<'_, Arc<Shared>>, id: String, enabled: bool) -> Cmd<AppState> {
    mutate(&shared, move |settings| settings.set_profile_enabled(&id, enabled)).await
}

#[tauri::command]
pub async fn add_site(shared: State<'_, Arc<Shared>>, input: String) -> Cmd<AppState> {
    mutate(&shared, move |settings| settings.add_site(&input).map(|_| ())).await
}

#[tauri::command]
pub async fn add_app(shared: State<'_, Arc<Shared>>, process: String) -> Cmd<AppState> {
    mutate(&shared, move |settings| settings.add_app(&process).map(|_| ())).await
}

#[tauri::command]
pub async fn remove_profile(shared: State<'_, Arc<Shared>>, id: String) -> Cmd<AppState> {
    mutate(&shared, move |settings| settings.remove_profile(&id)).await
}

/// System processes nobody wants to route; showing them only buries the real apps.
const HIDDEN_PROCESSES: &[&str] = &[
    "system", "registry", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe", "lsass.exe",
    "svchost.exe", "fontdrvhost.exe", "dwm.exe", "conhost.exe", "sihost.exe", "ctfmon.exe", "taskhostw.exe",
    "runtimebroker.exe", "searchhost.exe", "startmenuexperiencehost.exe", "shellexperiencehost.exe",
    "textinputhost.exe", "dllhost.exe", "audiodg.exe", "spoolsv.exe", "wmiprvse.exe", "securityhealthservice.exe",
];

#[tauri::command]
pub async fn list_processes(shared: State<'_, Arc<Shared>>) -> Cmd<Vec<RunningProcess>> {
    let direct = lock(&shared.settings).direct_processes.clone();
    blocking(move || {
        let windows_dir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_lowercase();
        let ours = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
        let mut skip: HashSet<String> = bypass_processes(&direct, None).iter().map(|n| n.to_lowercase()).collect();
        skip.extend(HIDDEN_PROCESSES.iter().map(|n| (*n).to_owned()));
        skip.extend(ours);
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for process in sys::processes().unwrap_or_default() {
            let key = process.name.to_lowercase();
            if process.pid <= 4 || skip.contains(&key) || !seen.insert(key) {
                continue;
            }
            // No path means a protected system process.
            let Some(path) = sys::process_path(process.pid) else { continue };
            if path.to_lowercase().starts_with(&windows_dir) {
                continue;
            }
            out.push(RunningProcess { name: process.name, path: Some(path) });
        }
        out.sort_by_key(|p| p.name.to_lowercase());
        Ok(out)
    })
    .await
}

#[tauri::command]
pub async fn get_connections(shared: State<'_, Arc<Shared>>) -> Cmd<Vec<ConnectionRow>> {
    Ok(lock(&shared.traffic).rows())
}

#[tauri::command]
pub async fn update_settings(shared: State<'_, Arc<Shared>>, patch: SettingsPatch) -> Cmd<AppState> {
    let shared = shared.inner().clone();
    blocking(move || {
        let current = lock(&shared.settings).clone();
        // Register the task first and only record what actually took effect.
        if let Some(wanted) = patch.autostart.filter(|wanted| *wanted != current.autostart) {
            if wanted && shared.unprotected.is_some() {
                return Err(crate::paths::UNPROTECTED.into());
            }
            if wanted { crate::autostart::enable(&shared.paths.runtime)? } else { crate::autostart::disable()? }
            shared.log(if wanted { "已开启开机自启" } else { "已关闭开机自启" });
        }
        let mut next = current.clone();
        next.apply_patch(&patch);
        if let Some(wanted) = patch.autostart {
            next.autostart = wanted;
        }
        let rules_changed = next.direct_processes != current.direct_processes
            || next.intranet_domains != current.intranet_domains
            || next.dns_mode != current.dns_mode
            || next.log_level != current.log_level;
        shared.update_settings(|settings| {
            *settings = next;
            Ok(())
        })?;
        if rules_changed && lock(&shared.settings).routing_enabled {
            shared.request_apply();
        }
        Ok(shared.snapshot())
    })
    .await
}

#[tauri::command]
pub async fn run_diagnostics(shared: State<'_, Arc<Shared>>, intranet: Option<String>) -> Cmd<diagnose::DiagnosticReport> {
    let shared = shared.inner().clone();
    blocking(move || {
        let settings = lock(&shared.settings).clone();
        let state = shared.snapshot();
        let routing = settings.routing_enabled && state.core_status == CoreStatus::Running;
        let targets = settings.targets();
        let input = diagnose::DiagnoseInput {
            upstream: settings.upstream.as_ref(),
            routing,
            clash_api: shared.clash_api.as_ref(),
            proxy_domains: &targets.domains,
            intranet: intranet.as_deref(),
        };
        let items = diagnose::run(&input);
        // Preset names are public; custom entries are the user's own sites, so only count them.
        let mut profiles: Vec<String> =
            state.profiles.iter().filter(|p| p.enabled && p.source == ProfileSource::Preset).map(|p| p.name.clone()).collect();
        let custom = settings.custom_profiles.iter().filter(|c| c.enabled).count();
        if custom > 0 {
            profiles.push(format!("自定义 {custom} 项"));
        }
        let upstream = match &settings.upstream {
            None => "未选择".to_owned(),
            Some(up) => {
                let place = if up.ip().is_some_and(|ip| ip.is_loopback() || ip.is_unspecified()) { "本机" } else { "局域网设备" };
                let client = settings.upstream_client.clone().unwrap_or_else(|| "未知程序".into());
                format!("{client}，{place}端口 {}（{}）", up.port, if up.kind == UpstreamKind::Http { "HTTP" } else { "SOCKS5" })
            }
        };
        let context = diagnose::ReportContext {
            version: shared.version.clone(),
            time: crate::paths::beijing_time(),
            elevated: shared.elevated,
            upstream,
            routing: match (settings.routing_enabled, state.core_status) {
                (false, _) => "已暂停".into(),
                (true, CoreStatus::Running) => "已开启".into(),
                (true, CoreStatus::Failed) => format!("出错：{}", state.core_message),
                (true, _) => "启动中".into(),
            },
            dns_mode: if settings.dns_mode == DnsMode::Encrypted { "加密 DNS".into() } else { "系统 DNS".into() },
            profiles,
        };
        let text = diagnose::render(&context, &items, &state.env);
        let _ = std::fs::write(shared.paths.data.join("diagnostics.txt"), &text);
        shared.log(&format!("运行自检：{} 项", items.len()));
        Ok(diagnose::DiagnosticReport { items, text })
    })
    .await
}

#[tauri::command]
pub async fn open_data_dir(shared: State<'_, Arc<Shared>>) -> Cmd<()> {
    let dir = shared.paths.data.clone();
    tauri_plugin_opener::open_path(dir, None::<&str>).map_err(|error| format!("无法打开数据目录：{error}"))
}

/// Null means already up to date.
#[tauri::command]
pub async fn check_update(shared: State<'_, Arc<Shared>>) -> Cmd<Option<crate::updates::UpdateInfo>> {
    crate::updates::check(shared.inner()).await
}

#[tauri::command]
pub async fn install_update(shared: State<'_, Arc<Shared>>) -> Cmd<()> {
    crate::updates::install(shared.inner()).await
}

#[tauri::command]
pub async fn open_releases() -> Cmd<()> {
    tauri_plugin_opener::open_url(crate::updates::RELEASES_URL, None::<&str>).map_err(|error| format!("无法打开浏览器：{error}"))
}

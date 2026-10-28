//! State shared by IPC commands, the tray and the supervisor thread.
//!
//! Lock discipline: take one of `settings`, `runtime`, `traffic`, `update` at a
//! time and copy out what you need. Nothing holds two locks, so nothing can deadlock.

use crate::paths::{AppPaths, append_log};
use crate::tray::TrayHandles;
use proxypilot_core::settings::Settings;
use proxypilot_core::traffic::TrafficLog;
use proxypilot_core::*;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, Emitter};

pub enum Msg {
    /// Rebuild and apply the config (debounced by the supervisor).
    Apply,
    /// Stop routing.
    Stop,
    /// Re-check the upstream and the environment now.
    Check,
    /// Stop the core and acknowledge, before the app exits.
    Quit(Sender<()>),
}

pub struct Runtime {
    pub core_status: CoreStatus,
    pub core_message: String,
    pub core_pid: Option<u32>,
    pub upstream_status: UpstreamStatus,
    pub env: Vec<EnvFinding>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub routing_enabled: bool,
    pub core_status: CoreStatus,
    pub core_message: String,
    pub upstream: Option<Upstream>,
    pub upstream_client: Option<String>,
    pub upstream_status: UpstreamStatus,
    pub profiles: Vec<Profile>,
    pub direct_processes: Vec<String>,
    pub intranet_domains: Vec<String>,
    pub dns_mode: DnsMode,
    pub log_level: LogLevel,
    pub autostart: bool,
    pub check_updates: bool,
    /// A newer release found by the last check, waiting for the user.
    pub update: Option<crate::updates::UpdateInfo>,
    pub elevated: bool,
    pub env: Vec<EnvFinding>,
    pub data_dir: String,
    pub version: String,
}

pub struct Shared {
    pub app: AppHandle,
    pub paths: AppPaths,
    pub core_exe: Option<PathBuf>,
    /// Something we run elevated that ordinary programs could change; while
    /// set, the core is not started and autostart is refused.
    pub unprotected: Option<PathBuf>,
    pub elevated: bool,
    pub version: String,
    pub settings: Mutex<Settings>,
    pub runtime: Mutex<Runtime>,
    pub traffic: Mutex<TrafficLog>,
    pub tx: Sender<Msg>,
    pub tray: Mutex<Option<TrayHandles>>,
    /// The core's Clash API for this run of the app; None if it could not be prepared.
    pub clash_api: Option<proxypilot_core::config::ClashApi>,
    /// The release the last check found, kept so installing does not ask GitHub again.
    pub update: Mutex<Option<tauri_plugin_updater::Update>>,
}

/// A panic elsewhere must not take every later lock down with it.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
    pub fn log(&self, message: &str) {
        append_log(&self.paths.app_log, message);
    }

    pub fn snapshot(&self) -> AppState {
        let settings = lock(&self.settings).clone();
        let (core_status, core_message, upstream_status, mut env) = {
            let runtime = lock(&self.runtime);
            (runtime.core_status, runtime.core_message.clone(), runtime.upstream_status, runtime.env.clone())
        };
        let update = lock(&self.update).as_ref().map(crate::updates::info);
        // A failed apply that fell back to the previous rules is still
        // "running"; say so where the user looks for problems.
        if core_status == CoreStatus::Running && !core_message.is_empty() {
            env.insert(0, EnvFinding {
                id: "apply_failed".into(),
                level: FindingLevel::Warning,
                title: "新规则没有生效".into(),
                detail: core_message.clone(),
            });
        }
        AppState {
            routing_enabled: settings.routing_enabled,
            core_status,
            core_message,
            upstream: settings.upstream.clone(),
            upstream_client: settings.upstream_client.clone(),
            upstream_status,
            profiles: settings.profiles(),
            direct_processes: settings.direct_processes.clone(),
            intranet_domains: settings.intranet_domains.clone(),
            dns_mode: settings.dns_mode,
            log_level: settings.log_level,
            autostart: settings.autostart,
            check_updates: settings.check_updates,
            update,
            elevated: self.elevated,
            env,
            data_dir: self.paths.data.to_string_lossy().into_owned(),
            version: self.version.clone(),
        }
    }

    /// Push the state to the window and the tray.
    pub fn emit(&self) {
        let state = self.snapshot();
        crate::tray::refresh(self, &state);
        let _ = self.app.emit("state-changed", &state);
    }

    /// Change settings and persist them; memory only changes if the save worked,
    /// so what the UI shows is always what is on disk.
    pub fn update_settings<R>(&self, change: impl FnOnce(&mut Settings) -> Result<R, String>) -> Result<R, String> {
        let mut settings = lock(&self.settings);
        let mut next = settings.clone();
        let result = change(&mut next)?;
        next.save(&self.paths.settings).map_err(|error| format!("无法保存设置：{error}"))?;
        *settings = next;
        Ok(result)
    }

    /// Ask the supervisor to rebuild the config, or to stop when routing is off.
    /// Shows "starting" at once so the switch does not look ignored during the debounce.
    pub fn request_apply(&self) {
        let enabled = lock(&self.settings).routing_enabled;
        if enabled {
            lock(&self.runtime).core_status = CoreStatus::Starting;
            let _ = self.tx.send(Msg::Apply);
        } else {
            let _ = self.tx.send(Msg::Stop);
        }
    }
}

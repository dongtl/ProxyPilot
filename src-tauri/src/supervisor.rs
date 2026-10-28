//! The one thread that owns sing-box. Commands and the tray only send it
//! messages, so starting, stopping and restarting never race each other.

use crate::state::{Msg, Shared, lock};
use proxypilot_core::backend::{ApplyRequest, Backend};
use proxypilot_core::detect::{detect, upstream_owner};
use proxypilot_core::envcheck::{EnvInput, evaluate};
use proxypilot_core::probe::upstream_alive;
use proxypilot_core::traffic::fetch_connections;
use proxypilot_core::*;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TICK: Duration = Duration::from_millis(500);
const APPLY_DEBOUNCE: Duration = Duration::from_millis(800);
const TRAFFIC_EVERY: Duration = Duration::from_secs(2);
const HEALTH_EVERY: Duration = Duration::from_secs(30);
const ALIVE_TIMEOUT: Duration = Duration::from_millis(800);
const RESTART_BACKOFF: [u64; 5] = [2, 5, 15, 60, 60];
/// A core that ran this long before dying is a fresh failure, not a crash loop.
const STABLE_AFTER: Duration = Duration::from_secs(300);

struct Supervisor {
    shared: Arc<Shared>,
    /// None when the install is missing sing-box (or, on macOS, the helper files).
    core: Option<Box<dyn Backend>>,
    pending_apply: Option<Instant>,
    restart_at: Option<Instant>,
    failures: usize,
    started_at: Option<Instant>,
    applied_owner: Option<String>,
    last_traffic: Option<Instant>,
    last_health: Option<Instant>,
}

pub fn spawn(shared: Arc<Shared>, rx: Receiver<Msg>) {
    std::thread::Builder::new()
        .name("supervisor".into())
        .spawn(move || {
            let core = backend(&shared);
            if shared.clash_api.is_none() {
                shared.log("无法生成 Clash API 端口");
            }
            let mut supervisor = Supervisor {
                shared,
                core,
                pending_apply: None,
                restart_at: None,
                failures: 0,
                started_at: None,
                applied_owner: None,
                last_traffic: None,
                last_health: None,
            };
            supervisor.run(rx);
        })
        .expect("cannot start supervisor thread");
}

/// sing-box in this elevated process on Windows; the root helper on macOS.
fn backend(shared: &Shared) -> Option<Box<dyn Backend>> {
    let _exe = shared.core_exe.clone()?;
    #[cfg(target_os = "macos")]
    return Some(Box::new(crate::helper_client::HelperBackend::new(shared.version.clone())));
    #[cfg(not(target_os = "macos"))]
    return Some(Box::new(proxypilot_core::backend::LocalBackend::new(_exe, &shared.paths.runtime)));
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Free function rather than a method, so it can run while `self.core` is borrowed.
fn set_core(shared: &Shared, status: CoreStatus, message: &str, pid: Option<u32>) {
    let mut runtime = lock(&shared.runtime);
    runtime.core_status = status;
    runtime.core_message = message.into();
    runtime.core_pid = pid;
}

fn due(last: Option<Instant>, every: Duration) -> bool {
    last.is_none_or(|at| at.elapsed() >= every)
}

impl Supervisor {
    fn run(&mut self, rx: Receiver<Msg>) {
        self.check_health();
        loop {
            match rx.recv_timeout(TICK) {
                Ok(Msg::Apply) => self.pending_apply = Some(Instant::now() + APPLY_DEBOUNCE),
                Ok(Msg::Stop) => self.stop(),
                Ok(Msg::Check) => self.last_health = None,
                Ok(Msg::Quit(ack)) => {
                    if let Some(core) = &mut self.core {
                        core.stop();
                    }
                    let _ = ack.send(());
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.tick();
        }
    }

    fn tick(&mut self) {
        if self.pending_apply.is_some_and(|at| Instant::now() >= at) {
            self.pending_apply = None;
            self.restart_at = None;
            self.failures = 0;
            self.apply();
        }
        if self.restart_at.is_some_and(|at| Instant::now() >= at) {
            self.restart_at = None;
            self.restart();
        }
        self.watch_core();
        if due(self.last_traffic, TRAFFIC_EVERY) {
            self.last_traffic = Some(Instant::now());
            self.poll_traffic();
        }
        if due(self.last_health, HEALTH_EVERY) {
            self.last_health = Some(Instant::now());
            self.check_health();
        }
    }

    fn apply(&mut self) {
        let settings = lock(&self.shared.settings).clone();
        if !settings.routing_enabled {
            self.stop();
            return;
        }
        let ready = self.can_route();
        let failure = if self.core.is_none() {
            Some("缺少 sing-box 核心，请重新安装")
        } else if self.shared.unprotected.is_some() {
            Some(crate::paths::UNPROTECTED)
        } else if !ready {
            Some(if cfg!(target_os = "macos") {
                "后台助手不可用，请把分流开关关掉再打开，按提示输入密码安装"
            } else {
                "没有管理员权限，无法创建分流用的虚拟网卡"
            })
        } else if settings.upstream.is_none() {
            Some("还没有选择代理端口")
        } else if self.shared.clash_api.is_none() {
            Some("无法准备 sing-box 控制接口")
        } else {
            None
        };
        if let Some(message) = failure {
            set_core(&self.shared, CoreStatus::Failed, message, None);
            self.shared.emit();
            return;
        }
        let shared = self.shared.clone();
        let (core, api, upstream) = (self.core.as_mut().unwrap(), shared.clash_api.as_ref().unwrap(), settings.upstream.clone().unwrap());
        set_core(&self.shared, CoreStatus::Starting, "", None);
        self.shared.emit();

        let owner = upstream_owner(&upstream).and_then(|(_, name)| name);
        let targets = settings.targets();
        let outcome = core.apply(&ApplyRequest::from_settings(&settings, &upstream, owner.as_deref(), api));
        let pid = core.pid();
        if outcome.ok {
            self.started_at = Some(Instant::now());
            self.applied_owner = owner;
            set_core(&self.shared, CoreStatus::Running, "", pid);
            self.shared.log(&format!(
                "分流已启用：上游 {}:{}，{} 个程序、{} 个域名走代理",
                upstream.host, upstream.port, targets.processes.len(), targets.domains.len()
            ));
        } else if outcome.adapter_taken {
            // Usually an older build still in the tray. Retry with backoff so
            // routing comes up on its own once the user quits it.
            self.shared.log(&format!("启用分流失败：{}", outcome.message));
            self.applied_owner = owner;
            self.core_down(outcome.message);
            return;
        } else if outcome.kept_previous {
            set_core(&self.shared, CoreStatus::Running, &outcome.message, pid);
            self.shared.log(&format!("应用新规则失败，已保留上一份：{}", outcome.message));
        } else {
            set_core(&self.shared, CoreStatus::Failed, &outcome.message, None);
            self.shared.log(&format!("启用分流失败：{}", outcome.message));
        }
        self.shared.emit();
    }

    fn restart(&mut self) {
        let Some(core) = &mut self.core else { return };
        match core.restart() {
            Ok(()) => {
                self.started_at = Some(Instant::now());
                let pid = core.pid();
                set_core(&self.shared, CoreStatus::Running, "", pid);
                self.shared.log("sing-box 已自动重启");
                self.shared.emit();
            }
            Err(error) => self.core_down(error),
        }
    }

    fn watch_core(&mut self) {
        let Some(reason) = self.core.as_mut().and_then(|core| core.poll_exit()) else { return };
        self.shared.log(&reason);
        if !lock(&self.shared.settings).routing_enabled {
            set_core(&self.shared, CoreStatus::Stopped, "", None);
            self.shared.emit();
            return;
        }
        self.core_down(reason);
    }

    /// The core is gone while routing should be on: retry with backoff, then give up.
    fn core_down(&mut self, reason: String) {
        if self.started_at.is_some_and(|at| at.elapsed() >= STABLE_AFTER) {
            self.failures = 0;
        }
        self.started_at = None;
        self.failures += 1;
        if self.failures > RESTART_BACKOFF.len() {
            set_core(&self.shared, CoreStatus::Failed, &format!("{reason}。已连续重启失败，请查看日志或重新开启分流"), None);
        } else {
            let wait = RESTART_BACKOFF[self.failures - 1];
            self.restart_at = Some(Instant::now() + Duration::from_secs(wait));
            set_core(&self.shared, CoreStatus::Starting, &format!("{reason}。{wait} 秒后自动重启"), None);
        }
        self.shared.emit();
    }

    fn stop(&mut self) {
        if let Some(core) = &mut self.core {
            core.stop();
        }
        self.pending_apply = None;
        self.restart_at = None;
        self.failures = 0;
        self.started_at = None;
        self.applied_owner = None;
        set_core(&self.shared, CoreStatus::Stopped, "", None);
        lock(&self.shared.traffic).clear();
        self.shared.emit();
    }

    fn poll_traffic(&mut self) {
        let (Some(core), Some(api)) = (&mut self.core, self.shared.clash_api.as_ref()) else { return };
        if !core.running() {
            return;
        }
        // Transient failures (core restarting) are expected; the next poll catches up.
        if let Ok(connections) = fetch_connections(api.port, &api.secret, Duration::from_secs(1)) {
            lock(&self.shared.traffic).ingest(&connections, now_ms());
        }
    }

    /// Whether a TUN can be created: the process's own rights on Windows; on
    /// macOS, the helper (connecting may upgrade it). Remembered for the UI.
    fn can_route(&mut self) -> bool {
        let ready = self.shared.elevated && self.core.as_mut().is_some_and(|core| core.ready());
        self.shared.set_can_route(ready);
        ready
    }

    fn check_health(&mut self) {
        let upstream = lock(&self.shared.settings).upstream.clone();
        let mut alive = upstream.as_ref().is_some_and(|up| upstream_alive(up, ALIVE_TIMEOUT));
        if !alive && upstream.is_some() && self.follow_client() {
            alive = true;
        }
        let routing = lock(&self.shared.settings).routing_enabled;
        if alive && routing && self.core.as_mut().is_some_and(|core| core.running()) {
            // Another client now owns the port: its process must join the direct list.
            let upstream = lock(&self.shared.settings).upstream.clone();
            let owner = upstream.as_ref().and_then(upstream_owner).and_then(|(_, name)| name);
            if owner.is_some() && owner != self.applied_owner && self.pending_apply.is_none() {
                self.shared.log("代理端口的占用程序变了，重新应用规则");
                self.pending_apply = Some(Instant::now() + APPLY_DEBOUNCE);
            }
        }
        let ours = self.core.as_ref().is_some_and(|core| core.may_own_adapter());
        let ready = self.can_route();
        let env = evaluate(&EnvInput::gather(
            ready,
            self.shared.core_exe.is_some(),
            self.shared.unprotected.is_some(),
            upstream.is_some(),
            alive,
            ours,
        ));
        {
            let mut runtime = lock(&self.shared.runtime);
            runtime.upstream_status = match (&upstream, alive) {
                (None, _) => UpstreamStatus::Unknown,
                (Some(_), true) => UpstreamStatus::Ok,
                (Some(_), false) => UpstreamStatus::Unreachable,
            };
            runtime.env = env;
        }
        self.shared.emit();
    }

    /// The client we were using restarted on another port: if exactly one
    /// usable port belongs to a client with the same label, move to it.
    fn follow_client(&mut self) -> bool {
        let (label, old) = {
            let settings = lock(&self.shared.settings);
            (settings.upstream_client.clone(), settings.upstream.clone())
        };
        let (Some(label), Some(old)) = (label, old) else { return false };
        if old.kind != UpstreamKind::Socks5 {
            return false;
        }
        let exclude: Vec<u32> = std::iter::once(std::process::id()).chain(self.core.as_ref().and_then(|c| c.pid())).collect();
        let matches: Vec<Candidate> = detect(false, &exclude)
            .into_iter()
            .filter(|c| c.known && c.status == CandidateStatus::Usable && c.client == label)
            .collect();
        let [candidate] = matches.as_slice() else { return false };
        let next = Upstream { kind: UpstreamKind::Socks5, host: candidate.host.clone(), port: candidate.port };
        if next == old || self.shared.update_settings(|s| { s.upstream = Some(next.clone()); Ok(()) }).is_err() {
            return false;
        }
        self.shared.log(&format!("{label} 换到了端口 {}，已自动跟随", next.port));
        if lock(&self.shared.settings).routing_enabled {
            self.pending_apply = Some(Instant::now() + APPLY_DEBOUNCE);
        }
        true
    }
}

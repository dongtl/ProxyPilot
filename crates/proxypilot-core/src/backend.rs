//! What the supervisor needs from whatever runs sing-box. On Windows that is
//! [`LocalBackend`], in the app's own elevated process; on macOS the root
//! helper runs a `LocalBackend` and the app talks to it over a socket.
//!
//! [`ApplyRequest`] is everything that decides the rules and nothing that
//! names a file, so the helper can take it from an unprivileged caller and
//! still choose every path sing-box writes to itself.

use crate::config::{BuildInput, ClashApi, RuntimePaths, build_config, write_config};
use crate::core::{ApplyOutcome, CoreManager, SingBox};
use crate::domain::normalise_domain;
use crate::model::{DnsMode, LogLevel, Upstream};
use crate::platform::Os;
use crate::settings::{Settings, Targets};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Generous for any hand-made rule list, small enough that a hostile caller
/// cannot make the helper build an enormous config.
const MAX_ENTRIES: usize = 4096;
const MAX_ENTRY_LEN: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    pub upstream: Upstream,
    /// Executable that owns the upstream port, kept direct to prevent a loop.
    pub upstream_owner: Option<String>,
    pub proxy_processes: Vec<String>,
    pub proxy_domains: Vec<String>,
    pub direct_processes: Vec<String>,
    pub intranet_domains: Vec<String>,
    pub dns_mode: DnsMode,
    pub log_level: LogLevel,
    pub clash_port: u16,
    pub clash_secret: String,
}

impl ApplyRequest {
    pub fn from_settings(settings: &Settings, upstream: &Upstream, owner: Option<&str>, api: &ClashApi) -> ApplyRequest {
        let targets = settings.targets();
        ApplyRequest {
            upstream: upstream.clone(),
            upstream_owner: owner.map(str::to_owned),
            proxy_processes: targets.processes,
            proxy_domains: targets.domains,
            direct_processes: settings.direct_processes.clone(),
            intranet_domains: settings.intranet_domains.clone(),
            dns_mode: settings.dns_mode,
            log_level: settings.log_level,
            clash_port: api.port,
            clash_secret: api.secret.clone(),
        }
    }

    /// Reject anything settings could not have produced. The helper runs
    /// this on every request; the app's own requests always pass.
    pub fn validate(&self) -> Result<(), String> {
        let upstream = self.upstream.normalised()?;
        if upstream != self.upstream {
            return Err("上游地址格式不对".into());
        }
        if self.clash_port < 1024 {
            return Err("控制接口端口无效".into());
        }
        if !(16..=128).contains(&self.clash_secret.len()) || !self.clash_secret.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("控制接口密钥无效".into());
        }
        let names = self.proxy_processes.iter().chain(&self.direct_processes).chain(&self.upstream_owner);
        let domains = self.proxy_domains.iter().chain(&self.intranet_domains);
        if self.proxy_processes.len() + self.direct_processes.len() + self.proxy_domains.len() + self.intranet_domains.len() > MAX_ENTRIES {
            return Err("分流条目太多".into());
        }
        for name in names {
            if name.is_empty() || name.len() > MAX_ENTRY_LEN || name.chars().any(|c| c.is_control() || c == '/' || c == '\\') {
                return Err("程序名格式不对".into());
            }
        }
        for domain in domains {
            if domain.len() > MAX_ENTRY_LEN || normalise_domain(domain).as_deref() != Ok(domain.as_str()) {
                return Err("域名格式不对".into());
            }
        }
        Ok(())
    }
}

pub trait Backend: Send {
    fn apply(&mut self, request: &ApplyRequest) -> ApplyOutcome;
    /// Start the installed config again, after a crash.
    fn restart(&mut self) -> Result<(), String>;
    fn stop(&mut self);
    fn running(&mut self) -> bool;
    fn pid(&self) -> Option<u32>;
    /// If the core exited on its own since the last call, why.
    fn poll_exit(&mut self) -> Option<String>;
    /// Whether an adapter that looks like ours could be our core's.
    fn may_own_adapter(&self) -> bool;
    /// Whether a TUN could be created now: always for a local core (the
    /// process's own privileges are checked elsewhere); for the macOS helper,
    /// whether it is installed, reachable and at the app's version.
    fn ready(&mut self) -> bool {
        true
    }
}

/// sing-box in this process's own hands.
pub struct LocalBackend {
    core: CoreManager,
    os: Os,
}

impl LocalBackend {
    /// `runtime` holds the configs and logs; only administrators may be able to change it.
    pub fn new(sing_box: PathBuf, runtime: &std::path::Path) -> LocalBackend {
        let ready = Box::new(crate::sys::tun_present);
        LocalBackend { core: CoreManager::new(Box::new(SingBox::new(sing_box)), RuntimePaths::new(runtime), ready), os: Os::CURRENT }
    }

    pub fn with_core(core: CoreManager, os: Os) -> LocalBackend {
        LocalBackend { core, os }
    }
}

impl Backend for LocalBackend {
    fn apply(&mut self, request: &ApplyRequest) -> ApplyOutcome {
        let failed = |message: String| ApplyOutcome { ok: false, message, rolled_back: false, kept_previous: false, adapter_taken: false };
        if let Err(message) = request.validate() {
            return failed(message);
        }
        let targets = Targets { processes: request.proxy_processes.clone(), domains: request.proxy_domains.clone() };
        let api = ClashApi { port: request.clash_port, secret: request.clash_secret.clone() };
        let (candidate, log) = (self.core.paths().candidate.clone(), self.core.paths().log.clone());
        let input = BuildInput {
            upstream: &request.upstream,
            upstream_owner: request.upstream_owner.as_deref(),
            targets: &targets,
            direct_processes: &request.direct_processes,
            intranet_domains: &request.intranet_domains,
            dns_mode: request.dns_mode,
            log_level: request.log_level,
            log_path: &log,
            clash_api: &api,
            os: self.os,
        };
        if let Err(error) = write_config(&build_config(&input), &candidate) {
            return failed(format!("无法写入配置：{error}"));
        }
        self.core.apply(&candidate)
    }

    fn restart(&mut self) -> Result<(), String> {
        self.core.restart()
    }

    fn stop(&mut self) {
        self.core.stop();
    }

    fn running(&mut self) -> bool {
        self.core.running()
    }

    fn pid(&self) -> Option<u32> {
        self.core.pid()
    }

    fn poll_exit(&mut self) -> Option<String> {
        self.core.poll_exit()
    }

    fn may_own_adapter(&self) -> bool {
        self.core.may_own_adapter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::UpstreamKind;

    fn request() -> ApplyRequest {
        ApplyRequest {
            upstream: Upstream { kind: UpstreamKind::Socks5, host: "127.0.0.1".into(), port: 10808 },
            upstream_owner: Some("xray".into()),
            proxy_processes: vec!["Cursor.app".into(), "codex".into()],
            proxy_domains: vec!["openai.com".into()],
            direct_processes: vec!["Corp.exe".into()],
            intranet_domains: vec!["corp.example.cn".into()],
            dns_mode: DnsMode::Encrypted,
            log_level: LogLevel::Warn,
            clash_port: 49152,
            clash_secret: "0123456789abcdef0123456789abcdef".into(),
        }
    }

    #[test]
    fn requests_from_settings_validate() {
        assert_eq!(request().validate(), Ok(()));
        let json = serde_json::to_string(&request()).unwrap();
        assert_eq!(serde_json::from_str::<ApplyRequest>(&json).unwrap(), request());
    }

    #[test]
    fn anything_settings_could_not_produce_is_rejected() {
        let mut bad = request();
        bad.upstream.host = "localhost".into();
        assert!(bad.validate().is_err(), "only normalised IPs");
        let mut bad = request();
        bad.proxy_processes.push("../../etc/passwd".into());
        assert!(bad.validate().is_err());
        let mut bad = request();
        bad.proxy_domains.push("a.com\"},{\"x".into());
        assert!(bad.validate().is_err());
        let mut bad = request();
        bad.clash_secret = "short".into();
        assert!(bad.validate().is_err());
        let mut bad = request();
        bad.clash_port = 80;
        assert!(bad.validate().is_err());
        let mut bad = request();
        bad.direct_processes = vec!["a".into(); MAX_ENTRIES + 1];
        assert!(bad.validate().is_err());
    }
}

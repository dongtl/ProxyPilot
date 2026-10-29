//! Types shared with the frontend. Each mirrors `src/api/types.ts`; change both together.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpstreamKind {
    Socks5,
    Http,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Upstream {
    pub kind: UpstreamKind,
    pub host: String,
    pub port: u16,
}

impl Upstream {
    /// Accept only an IP literal or `localhost`, normalised to an IP.
    ///
    /// The proxy outbound must never need DNS: resolving the upstream's own
    /// address through our tunnel is exactly the kind of loop the rest of the
    /// config is built to avoid, and a typo'd hostname would fail silently
    /// inside sing-box instead of here, where the user can see it.
    pub fn normalised(&self) -> Result<Upstream, String> {
        if self.port == 0 {
            return Err("端口必须在 1 到 65535 之间".into());
        }
        let host = self.host.trim();
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let ip: IpAddr = if host.eq_ignore_ascii_case("localhost") || host.is_empty() {
            IpAddr::from([127, 0, 0, 1])
        } else {
            host.parse()
                .map_err(|_| format!("地址“{host}”无效，请填写 IP 地址，例如 127.0.0.1"))?
        };
        Ok(Upstream { kind: self.kind, host: ip.to_string(), port: self.port })
    }

    pub fn ip(&self) -> Option<IpAddr> {
        self.host.parse().ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CandidateStatus {
    Usable,
    NeedsAuth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub port: u16,
    pub host: String,
    pub process: Option<String>,
    pub pid: Option<u32>,
    pub client: String,
    pub status: CandidateStatus,
    pub known: bool,
    /// The port's owner could not be seen (macOS, a root-run core) and
    /// `client` is the one proxy client found running, not a certainty.
    pub guessed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoreStatus {
    Stopped,
    Starting,
    Running,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpstreamStatus {
    Unknown,
    Ok,
    Unreachable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DnsMode {
    #[default]
    Encrypted,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    #[default]
    Warn,
    Info,
    Debug,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileSource {
    Preset,
    Custom,
}

/// A profile as the UI sees it: presets merged with the user's own entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub processes: Vec<String>,
    pub domains: Vec<String>,
    pub enabled: bool,
    pub source: ProfileSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingLevel {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvFinding {
    pub id: String,
    pub level: FindingLevel,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeStage {
    pub name: String,
    pub ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub ok: bool,
    pub code: String,
    pub message: String,
    pub target: String,
    pub elapsed_ms: u64,
    pub stages: Vec<ProbeStage>,
    /// Failed tries at targets before the one reported, for app.log only.
    #[serde(skip)]
    pub earlier: Vec<String>,
}

impl ProbeReport {
    /// One try: target, outcome, total and per-stage milliseconds.
    pub fn attempt_text(&self) -> String {
        let stages: Vec<String> = self.stages.iter().map(|stage| format!("{} {}", stage.name, stage.ms)).collect();
        format!("{} {} {}ms [{}]", self.target, self.code, self.elapsed_ms, stages.join(" · "))
    }

    /// For app.log: every try, oldest first. Local ports and public test
    /// sites only, so nothing here identifies a node.
    pub fn log_text(&self) -> String {
        let mut tries = self.earlier.clone();
        tries.push(self.attempt_text());
        format!("{}：{}", if self.ok { "通过" } else { "没通过" }, tries.join("；"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningProcess {
    pub name: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    Proxy,
    Direct,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionRow {
    /// Empty when sing-box could not tell which program it was.
    pub process: String,
    pub host: String,
    /// The site `host` belongs to, when that is wider than `host` itself and
    /// safe to offer as "the whole site". See `domain::site_of`.
    pub site: Option<String>,
    pub port: u16,
    pub route: Route,
    pub count: u32,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub direct_processes: Option<Vec<String>>,
    pub intranet_domains: Option<Vec<String>>,
    pub dns_mode: Option<DnsMode>,
    pub log_level: Option<LogLevel>,
    pub autostart: Option<bool>,
    pub check_updates: Option<bool>,
}

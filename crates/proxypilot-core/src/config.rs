//! The sing-box configuration. This is where "everything direct, only the
//! chosen traffic through the proxy" is actually decided; see the design doc,
//! section 3, for the rule order and why each rule sits where it does.

use crate::catalog::bypass_processes;
use crate::model::{DnsMode, LogLevel, Upstream, UpstreamKind};
use crate::platform::{Os, bundle_patterns, process_names};
use crate::settings::{Settings, Targets, write_atomic};
use serde_json::{Value, json};
use std::io;
use std::path::{Path, PathBuf};

/// The adapter's name on Windows. macOS only accepts `utunN`, so there the
/// name is left to sing-box and the adapter is recognised by [`TUN_ADDRESS`].
pub const INTERFACE_NAME: &str = "ProxyPilot";

/// Our end of the tunnel. Private, so direct by the intranet rules below.
pub const TUN_ADDRESS: &str = "172.19.0.1";

/// Payload is re-wrapped by the upstream client (SOCKS → its protocol → TLS →
/// TCP/IP, roughly 80-120 bytes). Matching the physical 1500 forced
/// fragmentation, and ICMP to the origin is dropped so PMTU discovery cannot
/// correct it.
pub const TUN_MTU: u32 = 1400;

/// Intranet ranges kept out of the proxy explicitly. `ip_is_private` already
/// covers most of these, but the corporate VPN depends on it, so state it
/// outright instead of relying on one flag's definition. 100.64/10 is CGNAT
/// and also Tailscale's range.
pub const INTRANET_CIDRS: &[&str] = &[
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "169.254.0.0/16",
    "127.0.0.0/8",
    "100.64.0.0/10",
    "::1/128",
    "fc00::/7",
    "fe80::/10",
];

/// Suffixes that only make sense inside a company or home network.
pub const INTRANET_SUFFIXES: &[&str] = &["local", "lan", "internal", "intranet", "corp", "home.arpa"];

const DIRECT_DOH: &str = "223.5.5.5";
const REMOTE_DOH: &str = "1.1.1.1";

/// Where the core's Clash API listens. Loopback only, a fresh port and secret
/// per run of the app, so other local programs cannot read the connection list.
pub struct ClashApi {
    pub port: u16,
    pub secret: String,
}

impl ClashApi {
    pub fn generate() -> io::Result<ClashApi> {
        let port = std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let mut bytes = [0u8; 16];
        crate::sys::random_bytes(&mut bytes)?;
        let secret = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(ClashApi { port, secret })
    }
}

#[derive(Clone, Copy)]
pub struct BuildInput<'a> {
    pub upstream: &'a Upstream,
    /// Executable that owns the upstream port right now, kept direct to prevent a loop.
    pub upstream_owner: Option<&'a str>,
    pub targets: &'a Targets,
    pub direct_processes: &'a [String],
    pub intranet_domains: &'a [String],
    pub dns_mode: DnsMode,
    pub log_level: LogLevel,
    pub log_path: &'a Path,
    pub clash_api: &'a ClashApi,
    /// The system the config is for: decides the adapter name and how
    /// program names become rules.
    pub os: Os,
}

impl<'a> BuildInput<'a> {
    pub fn from_settings(
        settings: &'a Settings,
        upstream: &'a Upstream,
        targets: &'a Targets,
        upstream_owner: Option<&'a str>,
        log_path: &'a Path,
        clash_api: &'a ClashApi,
    ) -> Self {
        BuildInput {
            upstream,
            upstream_owner,
            targets,
            direct_processes: &settings.direct_processes,
            intranet_domains: &settings.intranet_domains,
            dns_mode: settings.dns_mode,
            log_level: settings.log_level,
            log_path,
            clash_api,
            os: Os::CURRENT,
        }
    }
}

fn intranet_suffixes(extra: &[String]) -> Vec<String> {
    let mut all: Vec<String> = INTRANET_SUFFIXES.iter().map(|s| (*s).to_owned()).collect();
    for domain in extra {
        if !all.contains(domain) {
            all.push(domain.clone());
        }
    }
    all
}

/// Rules sending `names` to `outbound`: one by process name, and on macOS one
/// by path for `.app` bundles. They stay separate rules because sing-box
/// requires every field of one rule to match.
fn program_rules(names: &[String], outbound: &str, os: Os) -> Vec<Value> {
    let mut rules = Vec::new();
    let processes = process_names(names, os);
    if !processes.is_empty() {
        rules.push(json!({"process_name": processes, "outbound": outbound}));
    }
    let bundles = bundle_patterns(names, os);
    if !bundles.is_empty() {
        rules.push(json!({"process_path_regex": bundles, "outbound": outbound}));
    }
    rules
}

pub fn build_route_rules(input: &BuildInput) -> Vec<Value> {
    // Before sniffing: these processes never get their payload inspected,
    // and a corporate VPN or the upstream client must not see its own
    // traffic come back through us.
    let mut rules = program_rules(&bypass_processes(input.direct_processes, input.upstream_owner), "direct", input.os);
    rules.extend([
        json!({"action": "sniff"}),
        json!({"protocol": ["dns"], "action": "hijack-dns"}),
        json!({"ip_cidr": INTRANET_CIDRS, "outbound": "direct"}),
        json!({"ip_is_private": true, "outbound": "direct"}),
        json!({"domain_suffix": intranet_suffixes(input.intranet_domains), "outbound": "direct"}),
    ]);
    rules.extend(program_rules(&input.targets.processes, "proxy", input.os));
    if !input.targets.domains.is_empty() {
        rules.push(json!({"domain_suffix": input.targets.domains, "outbound": "proxy"}));
    }
    rules
}

pub fn build_dns(input: &BuildInput) -> Value {
    // "local" asks the operating system, so intranet names resolve exactly as
    // they would without us, through whatever DNS the corporate VPN installed.
    let mut servers = vec![json!({"type": "local", "tag": "system_dns"})];
    let final_server = match input.dns_mode {
        DnsMode::Encrypted => {
            servers.push(json!({"type": "https", "tag": "direct_dns", "server": DIRECT_DOH, "path": "/dns-query"}));
            "direct_dns"
        }
        DnsMode::System => "system_dns",
    };
    servers.push(json!({
        "type": "https", "tag": "remote_dns", "server": REMOTE_DOH, "path": "/dns-query", "detour": "proxy"
    }));
    // HTTPS/SVCB records can hand the browser alternative addresses and ECH
    // keys that sidestep what the A record told us; answer them empty.
    let mut rules = vec![
        json!({"query_type": [64, 65], "action": "predefined", "rcode": "NOERROR"}),
        json!({"domain_suffix": intranet_suffixes(input.intranet_domains), "server": "system_dns"}),
    ];
    if !input.targets.domains.is_empty() {
        rules.push(json!({"domain_suffix": input.targets.domains, "server": "remote_dns"}));
    }
    json!({
        "servers": servers,
        "rules": rules,
        "final": final_server,
        // Many proxy nodes have no IPv6 egress; an AAAA answer for a proxied
        // site would make the connection fail instead of falling back.
        "strategy": "ipv4_only",
    })
}

pub fn build_config(input: &BuildInput) -> Value {
    let proxy = match input.upstream.kind {
        UpstreamKind::Socks5 => json!({
            "type": "socks", "tag": "proxy", "server": input.upstream.host,
            "server_port": input.upstream.port, "version": "5"
        }),
        UpstreamKind::Http => json!({
            "type": "http", "tag": "proxy", "server": input.upstream.host, "server_port": input.upstream.port
        }),
    };
    let final_dns = if input.dns_mode == DnsMode::Encrypted { "direct_dns" } else { "system_dns" };
    let mut tun = json!({
        "type": "tun",
        "tag": "tun-in",
        // Both families, so IPv6 traffic meets the same rules instead of
        // leaving through the physical adapter untouched. 172.19/30 is
        // private and therefore direct by the rules above.
        "address": [format!("{TUN_ADDRESS}/30"), "fdfe:dcba:9876::1/126".to_owned()],
        "mtu": TUN_MTU,
        "auto_route": true,
        "strict_route": true,
        // "system" hands TCP to the kernel stack; "gvisor" reassembles every
        // packet in userspace, which on Windows terminates each connection
        // locally and hides real latency until the first data exchange.
        "stack": "system",
    });
    if input.os == Os::Windows {
        tun["interface_name"] = json!(INTERFACE_NAME);
    }
    json!({
        "log": {"level": input.log_level.as_str(), "timestamp": true, "output": input.log_path.to_string_lossy()},
        "dns": build_dns(input),
        "inbounds": [tun],
        // direct first: anything that names no detour uses the first outbound.
        "outbounds": [
            {"type": "direct", "tag": "direct"},
            proxy,
        ],
        "route": {
            "rules": build_route_rules(input),
            "final": "direct",
            "auto_detect_interface": true,
            // The first rule matches on process names anyway; stating it keeps
            // process paths in the Clash API for the connections page.
            "find_process": true,
            "default_domain_resolver": {"server": final_dns},
        },
        "experimental": {
            "clash_api": {
                "external_controller": format!("127.0.0.1:{}", input.clash_api.port),
                "secret": input.clash_api.secret,
            }
        },
    })
}

/// Pretty-printed with a trailing newline, written atomically.
pub fn write_config(config: &Value, path: &Path) -> io::Result<()> {
    let mut text = serde_json::to_string_pretty(config).map_err(io::Error::other)?;
    text.push('\n');
    write_atomic(path, text.as_bytes())
}

/// Files the core manager works with, all inside the runtime directory.
pub struct RuntimePaths {
    pub dir: PathBuf,
    pub candidate: PathBuf,
    pub current: PathBuf,
    pub previous: PathBuf,
    pub log: PathBuf,
    pub stderr_log: PathBuf,
}

impl RuntimePaths {
    pub fn new(dir: &Path) -> Self {
        RuntimePaths {
            dir: dir.to_path_buf(),
            candidate: dir.join("sing-box.candidate.json"),
            current: dir.join("sing-box.json"),
            previous: dir.join("sing-box.previous.json"),
            log: dir.join("sing-box.log"),
            stderr_log: dir.join("sing-box.stderr.log"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upstream() -> Upstream {
        Upstream { kind: UpstreamKind::Socks5, host: "127.0.0.1".into(), port: 10808 }
    }

    fn api() -> ClashApi {
        ClashApi { port: 49152, secret: "s3cret".into() }
    }

    fn input<'a>(upstream: &'a Upstream, targets: &'a Targets, api: &'a ClashApi, mode: DnsMode) -> BuildInput<'a> {
        BuildInput {
            upstream,
            upstream_owner: Some("my-core.exe"),
            targets,
            direct_processes: &[],
            intranet_domains: &[],
            dns_mode: mode,
            log_level: LogLevel::Warn,
            log_path: Path::new("C:/data/runtime/sing-box.log"),
            clash_api: api,
            os: Os::Windows,
        }
    }

    fn targets() -> Targets {
        Targets { processes: vec!["codex.exe".into()], domains: vec!["openai.com".into()] }
    }

    #[test]
    fn rule_order_keeps_bypass_first_and_proxy_last() {
        let (up, t, a) = (upstream(), targets(), api());
        let rules = build_route_rules(&input(&up, &t, &a, DnsMode::Encrypted));
        assert_eq!(rules[0]["outbound"], "direct");
        let bypass = rules[0]["process_name"].as_array().unwrap();
        assert!(bypass.iter().any(|v| v == "aTrust.exe"));
        assert_eq!(bypass.last().unwrap(), "my-core.exe");
        assert_eq!(rules[1]["action"], "sniff");
        assert_eq!(rules[2]["action"], "hijack-dns");
        assert_eq!(rules[3]["ip_cidr"][0], "10.0.0.0/8");
        assert_eq!(rules[4]["ip_is_private"], true);
        assert_eq!(rules[5]["outbound"], "direct");
        assert_eq!(rules[6], json!({"process_name": ["codex.exe"], "outbound": "proxy"}));
        assert_eq!(rules[7], json!({"domain_suffix": ["openai.com"], "outbound": "proxy"}));
        assert_eq!(rules.len(), 8);
    }

    #[test]
    fn empty_targets_add_no_proxy_rules() {
        let (up, a) = (upstream(), api());
        let t = Targets::default();
        let config = build_config(&input(&up, &t, &a, DnsMode::Encrypted));
        let rules = config["route"]["rules"].as_array().unwrap();
        assert!(rules.iter().all(|rule| rule["outbound"] != "proxy"));
        assert_eq!(config["route"]["final"], "direct");
        let dns_rules = config["dns"]["rules"].as_array().unwrap();
        assert!(dns_rules.iter().all(|rule| rule["server"] != "remote_dns"));
    }

    #[test]
    fn dns_modes_pick_the_final_server() {
        let (up, t, a) = (upstream(), targets(), api());
        let encrypted = build_dns(&input(&up, &t, &a, DnsMode::Encrypted));
        assert_eq!(encrypted["final"], "direct_dns");
        let system = build_dns(&input(&up, &t, &a, DnsMode::System));
        assert_eq!(system["final"], "system_dns");
        assert!(system["servers"].as_array().unwrap().iter().all(|s| s["tag"] != "direct_dns"));
        assert_eq!(encrypted["rules"][1]["server"], "system_dns");
        assert_eq!(encrypted["rules"][2], json!({"domain_suffix": ["openai.com"], "server": "remote_dns"}));
    }

    #[test]
    fn user_intranet_domains_join_the_builtin_suffixes() {
        let (up, t, a) = (upstream(), targets(), api());
        let extra = vec!["corp.example.cn".to_owned(), "lan".to_owned()];
        let mut built = input(&up, &t, &a, DnsMode::Encrypted);
        built.intranet_domains = &extra;
        let rules = build_route_rules(&built);
        let suffixes = rules[5]["domain_suffix"].as_array().unwrap();
        assert_eq!(suffixes.iter().filter(|v| *v == "lan").count(), 1);
        assert_eq!(suffixes.last().unwrap(), "corp.example.cn");
    }

    #[test]
    fn http_upstream_and_clash_api_are_wired() {
        let up = Upstream { kind: UpstreamKind::Http, host: "127.0.0.1".into(), port: 7890 };
        let (t, a) = (targets(), api());
        let config = build_config(&input(&up, &t, &a, DnsMode::Encrypted));
        assert_eq!(config["outbounds"][0]["tag"], "direct");
        assert_eq!(config["outbounds"][1], json!({"type": "http", "tag": "proxy", "server": "127.0.0.1", "server_port": 7890}));
        assert_eq!(config["experimental"]["clash_api"]["external_controller"], "127.0.0.1:49152");
        assert_eq!(config["inbounds"][0]["interface_name"], INTERFACE_NAME);
        assert_eq!(config["inbounds"][0]["address"][0], "172.19.0.1/30");
    }

    #[test]
    fn macos_matches_bundles_by_path_and_lets_sing_box_name_the_adapter() {
        let (up, a) = (upstream(), api());
        let t = Targets { processes: vec!["Cursor.app".into(), "codex".into()], domains: vec!["openai.com".into()] };
        let mut built = input(&up, &t, &a, DnsMode::Encrypted);
        built.os = Os::MacOs;
        let config = build_config(&built);
        assert!(config["inbounds"][0].get("interface_name").is_none());
        let rules = config["route"]["rules"].as_array().unwrap();
        // Bypass first: names without .exe, then bundles by path, then sniff.
        let names = rules[0]["process_name"].as_array().unwrap();
        assert!(names.iter().any(|v| v == "xray") && names.iter().all(|v| !v.as_str().unwrap().ends_with(".exe")));
        assert_eq!(names.last().unwrap(), "my-core");
        assert_eq!(rules[1]["outbound"], "direct");
        assert!(rules[1]["process_path_regex"].as_array().unwrap().iter().any(|v| v == r"(?i)/aTrust\.app/Contents/"));
        assert_eq!(rules[2]["action"], "sniff");
        let proxy: Vec<&Value> = rules.iter().filter(|rule| rule["outbound"] == "proxy").collect();
        assert_eq!(proxy[0], &json!({"process_name": ["codex"], "outbound": "proxy"}));
        assert_eq!(proxy[1], &json!({"process_path_regex": [r"(?i)/Cursor\.app/Contents/"], "outbound": "proxy"}));
        assert_eq!(proxy[2], &json!({"domain_suffix": ["openai.com"], "outbound": "proxy"}));
    }
}

//! What on this machine would make split routing misbehave. Detection and
//! advice only: we never change another program's settings.

use crate::catalog::running_vpn_clients;
use crate::config::INTERFACE_NAME;
use crate::model::{EnvFinding, FindingLevel};
use crate::sys::{self, InterfaceInfo, SystemProxy};
use std::collections::HashSet;

/// Adapter names other proxy tools give their TUN. Corporate VPN adapters are
/// deliberately absent: those are expected and handled by the bypass list.
const FOREIGN_TUN_HINTS: &[&str] =
    &["meta", "mihomo", "clash", "sing-box", "singbox", "v2ray", "xray", "nekoray", "nekobox", "hiddify", "flclash"];

pub struct EnvInput {
    pub elevated: bool,
    pub core_present: bool,
    pub system_proxy: SystemProxy,
    pub interfaces: Vec<InterfaceInfo>,
    /// Lower-cased running process names.
    pub running: HashSet<String>,
    pub upstream_configured: bool,
    pub upstream_alive: bool,
    /// Whether an adapter named like ours could be our own core's right now.
    pub adapter_ours: bool,
    /// Whether ordinary programs could change what we run elevated.
    pub install_unprotected: bool,
}

impl EnvInput {
    pub fn gather(
        core_present: bool,
        install_unprotected: bool,
        upstream_configured: bool,
        upstream_alive: bool,
        adapter_ours: bool,
    ) -> EnvInput {
        EnvInput {
            elevated: sys::is_elevated(),
            core_present,
            install_unprotected,
            system_proxy: sys::system_proxy(),
            interfaces: sys::interfaces().unwrap_or_default(),
            running: sys::running_process_names(),
            upstream_configured,
            upstream_alive,
            adapter_ours,
        }
    }
}

fn finding(id: &str, level: FindingLevel, title: &str, detail: String) -> EnvFinding {
    EnvFinding { id: id.into(), level, title: title.into(), detail }
}

fn points_here(text: &str) -> bool {
    let text = text.to_lowercase();
    text.contains("127.0.0.1") || text.contains("localhost") || text.contains("[::1]")
}

pub fn evaluate(input: &EnvInput) -> Vec<EnvFinding> {
    let mut out = Vec::new();
    if !input.core_present {
        out.push(finding(
            "core_missing",
            FindingLevel::Error,
            "缺少 sing-box 核心",
            "安装不完整，请重新安装 ProxyPilot。".into(),
        ));
    }
    if input.install_unprotected {
        out.push(finding(
            "install_unprotected",
            FindingLevel::Error,
            "安装目录权限不安全",
            "普通程序也能修改 ProxyPilot 的安装目录，而这里的程序会以管理员身份运行，被替换后就等于拿到了管理员权限。\
             为安全起见已停用分流和开机自启。请用最新的安装包重新安装（装在原来的位置也可以），安装程序会自动修正权限。"
                .into(),
        ));
    }
    if !input.elevated {
        out.push(finding(
            "not_elevated",
            FindingLevel::Error,
            "没有管理员权限",
            "创建分流用的虚拟网卡需要管理员权限。请右键 ProxyPilot，选择“以管理员身份运行”。".into(),
        ));
    }
    let proxy = &input.system_proxy;
    if (proxy.enabled && !proxy.server.is_empty()) || !proxy.auto_config_url.is_empty() {
        let local = (proxy.enabled && points_here(&proxy.server)) || points_here(&proxy.auto_config_url);
        if local {
            out.push(finding(
                "system_proxy",
                FindingLevel::Warning,
                "系统代理已开启",
                "浏览器会按系统代理把网页全部交给代理客户端，绕过这里的分流设置。请在代理客户端里关闭“系统代理”（v2rayN：系统代理设为“清除”；Clash：关闭“系统代理”）。".into(),
            ));
        } else {
            out.push(finding(
                "system_proxy_remote",
                FindingLevel::Info,
                "检测到系统代理设置",
                "系统代理指向另一台机器，可能是公司网络的设置。浏览器流量会先经过它。".into(),
            ));
        }
    }
    let foreign: Vec<&str> = input
        .interfaces
        .iter()
        .filter(|item| item.up && !item.alias.eq_ignore_ascii_case(INTERFACE_NAME))
        .filter(|item| {
            let haystack = format!("{} {}", item.alias, item.description).to_lowercase();
            FOREIGN_TUN_HINTS.iter().any(|hint| haystack.contains(hint))
        })
        .map(|item| item.alias.as_str())
        .collect();
    if !foreign.is_empty() {
        out.push(finding(
            "foreign_tun",
            FindingLevel::Warning,
            "其它代理工具的虚拟网卡已开启",
            format!(
                "检测到 {}。两个虚拟网卡会互相抢路由，请在代理客户端里关闭 TUN / 虚拟网卡模式，只保留本地端口。",
                foreign.join("、")
            ),
        ));
    }
    // Same name, not our core: an older ProxyPilot build, typically. sing-box
    // derives the adapter's identity from its name, so ours cannot be created.
    let taken = !input.adapter_ours && input.interfaces.iter().any(|item| item.up && item.alias.eq_ignore_ascii_case(INTERFACE_NAME));
    if taken {
        out.push(finding(
            "adapter_taken",
            FindingLevel::Error,
            "ProxyPilot 虚拟网卡已被占用",
            "另一个程序创建了同名的虚拟网卡，常见原因是旧版 ProxyPilot 还在运行。请右键托盘里旧版的图标退出它，分流会自动重试；没有恢复的话，把分流开关关掉再打开。".into(),
        ));
    }
    if input.upstream_configured && !input.upstream_alive {
        out.push(finding(
            "upstream_down",
            FindingLevel::Warning,
            "代理客户端未运行",
            "连不上选定的代理端口。请打开代理客户端；如果它换了端口，请在设置里重新选择。没选中的流量不受影响。".into(),
        ));
    }
    let vpns = running_vpn_clients(&input.running);
    if !vpns.is_empty() {
        let names: Vec<&str> = vpns.iter().map(|client| client.label).collect();
        out.push(finding(
            "corporate_vpn",
            FindingLevel::Info,
            "已识别企业 VPN",
            format!("{} 正在运行，它的流量会自动直连，不受分流影响。", names.join("、")),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean() -> EnvInput {
        EnvInput {
            elevated: true,
            core_present: true,
            system_proxy: SystemProxy::default(),
            interfaces: Vec::new(),
            running: HashSet::new(),
            upstream_configured: true,
            upstream_alive: true,
            adapter_ours: true,
            install_unprotected: false,
        }
    }

    fn ids(input: &EnvInput) -> Vec<String> {
        evaluate(input).into_iter().map(|finding| finding.id).collect()
    }

    #[test]
    fn clean_machine_has_no_findings() {
        assert!(evaluate(&clean()).is_empty());
    }

    #[test]
    fn local_system_proxy_warns_and_remote_informs() {
        let mut input = clean();
        input.system_proxy = SystemProxy { enabled: true, server: "127.0.0.1:10809".into(), auto_config_url: String::new() };
        assert_eq!(ids(&input), vec!["system_proxy"]);
        input.system_proxy = SystemProxy { enabled: true, server: "proxy.corp:8080".into(), auto_config_url: String::new() };
        assert_eq!(ids(&input), vec!["system_proxy_remote"]);
        input.system_proxy = SystemProxy { enabled: false, server: "127.0.0.1:7890".into(), auto_config_url: String::new() };
        assert!(ids(&input).is_empty());
        input.system_proxy.auto_config_url = "http://127.0.0.1:7890/pac".into();
        assert_eq!(ids(&input), vec!["system_proxy"]);
    }

    #[test]
    fn foreign_tun_ignores_our_adapter_and_down_adapters() {
        let mut input = clean();
        let adapter = |alias: &str, up| InterfaceInfo { alias: alias.into(), description: "Wintun Userspace Tunnel".into(), up };
        input.interfaces = vec![adapter("ProxyPilot", true), adapter("Mihomo", false), adapter("aTrust VNIC", true)];
        assert!(ids(&input).is_empty());
        input.interfaces.push(adapter("Meta", true));
        let found = evaluate(&input);
        assert_eq!(found[0].id, "foreign_tun");
        assert!(found[0].detail.contains("Meta"));
    }

    #[test]
    fn adapter_with_our_name_and_no_core_of_ours_is_reported() {
        let mut input = clean();
        input.interfaces = vec![InterfaceInfo { alias: "ProxyPilot".into(), description: "sing-tun Tunnel".into(), up: true }];
        assert!(ids(&input).is_empty());
        input.adapter_ours = false;
        assert_eq!(ids(&input), vec!["adapter_taken"]);
        input.interfaces[0].up = false;
        assert!(ids(&input).is_empty());
    }

    #[test]
    fn reports_missing_privileges_core_upstream_and_vpn() {
        let mut input = clean();
        input.elevated = false;
        input.core_present = false;
        input.upstream_alive = false;
        input.running.insert("atrust.exe".into());
        assert_eq!(ids(&input), vec!["core_missing", "not_elevated", "upstream_down", "corporate_vpn"]);
    }

    #[test]
    fn writable_install_folder_is_an_error() {
        let mut input = clean();
        input.install_unprotected = true;
        let found = evaluate(&input);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].id.as_str(), found[0].level), ("install_unprotected", FindingLevel::Error));
    }
}

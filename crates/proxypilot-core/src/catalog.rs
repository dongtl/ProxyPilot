//! Which processes must never have their traffic put inside our tunnel, and
//! which ones are proxy clients we can offer as an upstream.
//!
//! A corporate SSL-VPN builds its own tunnel to a concentrator that sits at a
//! public address, and that address differs per company and changes. So the
//! intranet rules cannot cover it: the intranet CIDRs and `ip_is_private` keep
//! traffic to 10/8 and friends direct, but the client's traffic to its own
//! gateway is traffic to the public internet as far as routing can tell. The
//! only stable thing about it is which program sends it, which is why this
//! keys on process names.
//!
//! Nesting that tunnel inside ours does not fail cleanly. It usually
//! half-works: the handshake completes, then MTU and keepalives interact badly
//! and the intranet becomes slow and unreliable rather than unreachable, which
//! is a much worse thing to debug than a refused connection.
//!
//! The catalogue is applied in full and unconditionally. A rule naming a
//! process nobody has installed never matches, so listing a client costs
//! nothing, while missing one costs the user their intranet. That trade is why
//! there is no switch here. It is a starting point, not an authority: builds
//! get renamed and rebranded, which is why the user can add their own and why
//! the UI offers the processes actually running instead of a text box.

use crate::platform::{display_name, name_key};
use std::collections::HashSet;

pub struct VpnClient {
    pub id: &'static str,
    pub label: &'static str,
    pub processes: &'static [&'static str],
}

// Each list holds the Windows executables, then macOS names. On macOS the
// `.exe` is dropped when rules are built (see `platform`), so `aTrustAgent.exe`
// also covers the `aTrustAgent` daemon; `.app` entries match every process
// inside that bundle.
pub const VPN_CATALOGUE: &[VpnClient] = &[
    VpnClient { id: "atrust", label: "深信服 aTrust", processes: &[
        "aTrust.exe", "aTrustTray.exe", "aTrustAgent.exe", "aTrustService.exe",
        "aTrustXtunnel.exe", "aTrustHelper.exe", "aTrustUpdate.exe",
        "aTrust.app",
    ] },
    VpnClient { id: "easyconnect", label: "深信服 EasyConnect", processes: &[
        "EasyConnect.exe", "EasyMonitor.exe", "ECAgent.exe", "ECAgentProxy.exe",
        "SangforServiceClient.exe",
        "EasyConnect.app",
    ] },
    VpnClient { id: "secoclient", label: "华为 SecoClient", processes: &[
        "SecoClient.exe", "SecoClientService.exe",
        "SecoClient.app",
    ] },
    VpnClient { id: "anyconnect", label: "Cisco AnyConnect / Secure Client", processes: &[
        "vpnui.exe", "vpnagent.exe", "vpncli.exe", "csc_ui.exe", "acwebsecagent.exe",
        "vpnagentd", "Cisco Secure Client.app", "Cisco AnyConnect Secure Mobility Client.app",
    ] },
    VpnClient { id: "forticlient", label: "FortiClient", processes: &[
        "FortiClient.exe", "FortiSSLVPNdaemon.exe", "FortiTray.exe", "FortiESNAC.exe",
        "FortiClient.app",
    ] },
    VpnClient { id: "globalprotect", label: "Palo Alto GlobalProtect", processes: &[
        "PanGPA.exe", "PanGPS.exe", "PanGpHip.exe",
        "GlobalProtect.app",
    ] },
    VpnClient { id: "ivanti", label: "Ivanti / Pulse Secure", processes: &[
        "Pulse.exe", "PulseSecureService.exe", "dsAccessService.exe", "dsNcService.exe", "JamUI.exe",
        "Pulse Secure.app", "Ivanti Secure Access.app",
    ] },
    VpnClient { id: "inode", label: "H3C iNode", processes: &[
        "iNodeClient.exe", "iNodeMon.exe", "iNodeHelper.exe",
        "iNodeClient.app",
    ] },
    VpnClient { id: "citrix", label: "Citrix Gateway / Workspace", processes: &[
        "nsepa.exe", "nsauto.exe", "CitrixWorkspaceApp.exe", "SelfServicePlugin.exe", "wfcrun32.exe",
        "Citrix Workspace.app",
    ] },
    VpnClient { id: "checkpoint", label: "Check Point Endpoint / Harmony", processes: &[
        "TrGUI.exe", "TracSrvWrapper.exe", "EPCClient.exe",
        "Endpoint Security VPN.app",
    ] },
    VpnClient { id: "netextender", label: "SonicWall NetExtender / Mobile Connect", processes: &[
        "NetExtender.exe", "NEService.exe",
        "NetExtender.app",
    ] },
    VpnClient { id: "zscaler", label: "Zscaler Client Connector", processes: &[
        "ZSATray.exe", "ZSATrayManager.exe", "ZSAService.exe", "ZSATunnel.exe",
        "Zscaler.app",
    ] },
    VpnClient { id: "netskope", label: "Netskope Client", processes: &[
        "stAgentUI.exe", "stAgentSvc.exe",
        "Netskope Client.app",
    ] },
    VpnClient { id: "openvpn", label: "OpenVPN", processes: &[
        "openvpn.exe", "openvpn-gui.exe", "openvpnserv.exe", "openvpnconnect.exe",
        "Tunnelblick.app", "OpenVPN Connect.app",
    ] },
    VpnClient { id: "wireguard", label: "WireGuard", processes: &["wireguard.exe", "wg.exe", "WireGuard.app"] },
    VpnClient { id: "ipsec", label: "Windows 内置 VPN / IPsec", processes: &["rasphone.exe", "rasdial.exe"] },
];

/// Not a VPN, but each reaches internal services over its own channel and
/// behaves the same way when tunnelled: mostly working, intermittently not.
/// Kept apart from the catalogue so the reason stays legible.
pub const INTRANET_APPLICATIONS: &[&str] =
    &["EnterpriseWeChat.exe", "WXWork.exe", "DingTalk.exe", "企业微信.app", "WeCom.app", "钉钉.app", "DingTalk.app"];

/// Proxy cores and clients. Their own traffic must stay direct for the obvious
/// reason: routing a proxy's outbound through itself is a loop. The process
/// that owns the chosen upstream port is added on top of this at build time,
/// so a client missing here is still safe as long as it is the upstream.
pub const PROXY_CORES: &[&str] = &[
    "v2rayN.exe", "v2ray.exe", "xray.exe", "sing-box.exe", "mihomo.exe", "clash.exe",
    "verge-mihomo.exe", "verge-mihomo-alpha.exe", "clash-meta.exe", "clash-verge.exe",
    "FlClash.exe", "FlClashCore.exe", "nekobox_core.exe", "nekoray_core.exe", "hiddify-core.exe",
    "HiddifyCli.exe", "sslocal.exe", "Shadowsocks.exe", "hysteria.exe", "naive.exe",
    "naiveproxy.exe", "tuic.exe", "juicity.exe", "tun2socks.exe",
    // Clash for Windows: the GUI runs its core as clash-win64 / clash-win32.
    "Clash for Windows.exe", "clash-win64.exe", "clash-win32.exe",
    "v2rayN.app", "Clash Verge.app", "ClashX.app", "ClashX Pro.app", "FlClash.app", "Hiddify.app",
    "Clash for Windows.app",
    "NekoRay.app", "ShadowsocksX-NG.app", "Surge.app", "Stash.app", "Shadowrocket.app", "sing-box.app",
];

/// How a port owner is shown to the user. Matched case-insensitively against
/// the executable name; the first matching row wins.
pub const CLIENT_LABELS: &[(&[&str], &str)] = &[
    (&["xray.exe", "v2ray.exe", "v2rayN.exe"], "Xray 核心（v2rayN 等）"),
    (&["verge-mihomo.exe", "verge-mihomo-alpha.exe", "clash-verge.exe"], "Clash Verge"),
    (&["FlClash.exe", "FlClashCore.exe"], "FlClash"),
    (&["clash-win64.exe", "clash-win32.exe", "Clash for Windows.exe"], "Clash for Windows"),
    (&["mihomo.exe", "clash.exe", "clash-meta.exe"], "Clash / Mihomo"),
    (&["sing-box.exe"], "sing-box 核心"),
    (&["nekobox_core.exe", "nekoray_core.exe"], "NekoBox"),
    (&["hiddify-core.exe", "HiddifyCli.exe"], "Hiddify"),
    (&["sslocal.exe", "Shadowsocks.exe"], "Shadowsocks"),
    (&["hysteria.exe"], "Hysteria"),
    (&["naive.exe", "naiveproxy.exe"], "NaiveProxy"),
    (&["tuic.exe"], "TUIC"),
];

/// Ports proxy clients ship with by default. Probing only these plus the ports
/// owned by known clients keeps the default scan from poking at every local
/// service; "全部扫描" covers everything else.
pub const COMMON_PORTS: &[u16] = &[
    10808, 10809, 10810, 7890, 7891, 7897, 7898, 1080, 1081, 2080, 2081, 20170, 20171, 12334,
];

pub fn catalogue_processes() -> impl Iterator<Item = &'static str> {
    VPN_CATALOGUE.iter().flat_map(|client| client.processes.iter().copied())
}

/// Keep the non-empty strings; a hand-edited settings file can contain anything.
pub fn normalise_names<'a>(names: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    names.into_iter().map(|name| name.trim()).filter(|name| !name.is_empty()).map(str::to_owned).collect()
}

/// Everything that stays direct, built-ins first, without duplicates.
///
/// Compared by `platform::name_key`: case-insensitively because Windows
/// process names are, and without `.exe` so `xray.exe` and macOS's `xray`
/// are one entry. A list holding both `aTrust.exe` and `atrust.exe` would be
/// two rules doing one job.
pub fn bypass_processes(configured: &[String], upstream_owner: Option<&str>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    let builtins = PROXY_CORES.iter().copied().chain(catalogue_processes()).chain(INTRANET_APPLICATIONS.iter().copied());
    let extra = normalise_names(configured);
    let owner = upstream_owner.map(str::trim).filter(|name| !name.is_empty());
    for name in builtins.map(str::to_owned).chain(extra).chain(owner.map(str::to_owned)) {
        if seen.insert(name_key(&name)) {
            ordered.push(name);
        }
    }
    ordered
}

/// Label for a port owner, and whether it is a client we recognise.
pub fn client_label(process: Option<&str>) -> (String, bool) {
    let Some(process) = process else {
        return ("未知程序".into(), false);
    };
    let key = name_key(process);
    for (names, label) in CLIENT_LABELS {
        if names.iter().any(|name| name_key(name) == key) {
            return ((*label).into(), true);
        }
    }
    let known = PROXY_CORES.iter().any(|name| name_key(name) == key);
    (display_name(process), known)
}

/// Catalogue entries with a process in `running`, which holds `name_key`s
/// of the running processes' names and of the bundles they run from.
pub fn running_vpn_clients(running: &HashSet<String>) -> Vec<&'static VpnClient> {
    VPN_CATALOGUE
        .iter()
        .filter(|client| client.processes.iter().any(|name| running.contains(&name_key(name))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bypass_keeps_builtins_first_and_dedups_case_insensitively() {
        let configured = vec!["ATRUST.exe".to_owned(), "  ".to_owned(), "Corp.exe".to_owned()];
        let list = bypass_processes(&configured, Some("my-core.exe"));
        assert_eq!(list[0], "v2rayN.exe");
        assert!(list.iter().any(|name| name == "aTrust.exe"));
        assert!(!list.iter().any(|name| name == "ATRUST.exe"));
        assert_eq!(list[list.len() - 2], "Corp.exe");
        assert_eq!(list.last().unwrap(), "my-core.exe");
    }

    #[test]
    fn upstream_owner_already_listed_is_not_repeated() {
        let list = bypass_processes(&[], Some("XRAY.EXE"));
        assert_eq!(list.iter().filter(|name| name.eq_ignore_ascii_case("xray.exe")).count(), 1);
    }

    #[test]
    fn labels_known_clients_and_falls_back_to_the_name() {
        assert_eq!(client_label(Some("Xray.exe")), ("Xray 核心（v2rayN 等）".into(), true));
        assert_eq!(client_label(Some("tun2socks.exe")), ("tun2socks".into(), true));
        assert_eq!(client_label(Some("python.exe")), ("python".into(), false));
        assert_eq!(client_label(None), ("未知程序".into(), false));
        assert_eq!(client_label(Some("clash-win64.exe")), ("Clash for Windows".into(), true));
        // FlClash's core, which owns the mixed port (FlClash.exe itself only talks to it).
        assert_eq!(client_label(Some("FlClashCore.exe")), ("FlClash".into(), true));
        // macOS names: no .exe, same labels.
        assert_eq!(client_label(Some("verge-mihomo")), ("Clash Verge".into(), true));
        assert_eq!(client_label(Some("FlClashCore")), ("FlClash".into(), true));
        assert_eq!(client_label(Some("node")), ("node".into(), false));
    }

    #[test]
    fn macos_names_dedup_against_windows_ones_and_find_running_vpns() {
        let list = bypass_processes(&["xray".to_owned(), "Corp.app".to_owned()], None);
        assert_eq!(list.iter().filter(|name| name_key(name) == "xray").count(), 1);
        assert!(list.iter().any(|name| name == "aTrust.app"));
        let running: HashSet<String> = ["atrustagent".to_owned()].into();
        assert_eq!(running_vpn_clients(&running).iter().map(|c| c.id).collect::<Vec<_>>(), vec!["atrust"]);
    }
}

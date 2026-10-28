//! Parsers for the macOS command-line tools `sys::macos` reads. They are pure
//! text-in, data-out and compiled everywhere, so their tests run on Windows
//! too; only running the tools is macOS-specific.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use super::{ListenerInfo, SystemProxy};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// `127.0.0.1.7890`, `*.7890`, `::1.7890`: netstat's address.port form.
fn netstat_address(text: &str, v6: bool) -> Option<(IpAddr, u16)> {
    let (host, port) = text.rsplit_once('.')?;
    let port: u16 = port.parse().ok()?;
    let ip = match host {
        "*" if v6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        "*" => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        // Scoped (fe80::1%lo0) addresses are link-local, never a local proxy.
        host if host.contains('%') => return None,
        host => host.parse().ok()?,
    };
    Some((ip, port))
}

/// Listening TCP sockets from `netstat -an -p tcp`. It sees every user's
/// sockets but not their owners, so pids are 0 here.
pub fn netstat_listeners(text: &str) -> Vec<ListenerInfo> {
    let mut out = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 6 || fields[5] != "LISTEN" || !fields[0].starts_with("tcp") {
            continue;
        }
        // tcp46 is one dual-stack socket; report it as IPv6 any, which
        // accepts IPv4 connections too.
        let v6 = fields[0].ends_with('6');
        if let Some((ip, port)) = netstat_address(fields[3], v6) {
            out.push(ListenerInfo { ip, port, pid: 0 });
        }
    }
    out
}

/// `127.0.0.1:7890`, `*:7890`, `[::1]:7890`: lsof's host:port form.
fn lsof_address(text: &str) -> Option<(IpAddr, u16)> {
    let (host, port) = text.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    let ip = match host {
        "*" => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        host => host.trim_start_matches('[').trim_end_matches(']').parse().ok()?,
    };
    Some((ip, port))
}

/// Listening sockets with owners from `lsof -nP -iTCP -sTCP:LISTEN -Fpn`
/// (`p` pid lines, then `n` address lines). Only the caller's own processes
/// are visible to an ordinary user.
pub fn lsof_listeners(text: &str) -> Vec<ListenerInfo> {
    let mut out = Vec::new();
    let mut pid: Option<u32> = None;
    for line in text.lines() {
        let Some(tag) = line.chars().next() else { continue };
        let value = &line[tag.len_utf8()..];
        match tag {
            'p' => pid = value.parse().ok(),
            'n' => {
                if let (Some(pid), Some((ip, port))) = (pid, lsof_address(value)) {
                    out.push(ListenerInfo { ip, port, pid });
                }
            }
            _ => {}
        }
    }
    out
}

/// netstat's complete list, with owners filled in from lsof where it knows
/// them. A port lsof cannot attribute keeps pid 0 and is still probed.
pub fn merge_listeners(all: Vec<ListenerInfo>, owned: &[ListenerInfo]) -> Vec<ListenerInfo> {
    let by_port: HashMap<u16, u32> = owned.iter().map(|listener| (listener.port, listener.pid)).collect();
    let mut out: Vec<ListenerInfo> = all
        .into_iter()
        .map(|mut listener| {
            if let Some(pid) = by_port.get(&listener.port) {
                listener.pid = *pid;
            }
            listener
        })
        .collect();
    // netstat can fail where lsof did not; keep what lsof saw.
    for listener in owned {
        if !out.iter().any(|known| known.port == listener.port) {
            out.push(listener.clone());
        }
    }
    out
}

/// `scutil --proxy`: the proxy of the active network service.
pub fn scutil_proxy(text: &str) -> SystemProxy {
    let mut values: HashMap<&str, &str> = HashMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':') {
            values.insert(key.trim(), value.trim());
        }
    }
    let on = |key: &str| values.get(key).is_some_and(|value| *value == "1");
    let server = [("HTTPEnable", "HTTPProxy", "HTTPPort"), ("HTTPSEnable", "HTTPSProxy", "HTTPSPort"), ("SOCKSEnable", "SOCKSProxy", "SOCKSPort")]
        .iter()
        .filter(|(enable, _, _)| on(enable))
        .find_map(|(_, host, port)| {
            let host = values.get(host)?;
            Some(match values.get(port) {
                Some(port) => format!("{host}:{port}"),
                None => (*host).to_owned(),
            })
        });
    SystemProxy {
        enabled: server.is_some(),
        server: server.unwrap_or_default(),
        auto_config_url: if on("ProxyAutoConfigEnable") {
            values.get("ProxyAutoConfigURLString").copied().unwrap_or_default().to_owned()
        } else {
            String::new()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NETSTAT: &str = "Active Internet connections (including servers)
Proto Recv-Q Send-Q  Local Address          Foreign Address        (state)
tcp4       0      0  127.0.0.1.10808        *.*                    LISTEN
tcp46      0      0  *.7897                 *.*                    LISTEN
tcp6       0      0  ::1.631                *.*                    LISTEN
tcp6       0      0  fe80::1%lo0.5000       *.*                    LISTEN
tcp4       0      0  192.168.1.5.52344      17.253.1.2.443         ESTABLISHED
udp4       0      0  *.5353                 *.*
";

    #[test]
    fn netstat_lists_listeners_of_every_user() {
        let list = netstat_listeners(NETSTAT);
        assert_eq!(
            list,
            vec![
                ListenerInfo { ip: "127.0.0.1".parse().unwrap(), port: 10808, pid: 0 },
                ListenerInfo { ip: "::".parse().unwrap(), port: 7897, pid: 0 },
                ListenerInfo { ip: "::1".parse().unwrap(), port: 631, pid: 0 },
            ]
        );
    }

    #[test]
    fn lsof_owners_fill_in_netstat() {
        let lsof = "p812\nf9\nn127.0.0.1:10808\np900\nn*:2080\nn[::1]:2081\n";
        let owned = lsof_listeners(lsof);
        assert_eq!(owned.len(), 3);
        assert_eq!(owned[2], ListenerInfo { ip: "::1".parse().unwrap(), port: 2081, pid: 900 });
        let merged = merge_listeners(netstat_listeners(NETSTAT), &owned);
        assert_eq!(merged[0].pid, 812);
        assert_eq!(merged[1].pid, 0, "root-owned port stays unattributed");
        assert!(merged.iter().any(|l| l.port == 2080 && l.pid == 900));
    }

    #[test]
    fn scutil_reports_the_enabled_proxy_and_pac() {
        let text = "<dictionary> {
  ExceptionsList : <array> {
    0 : *.local
  }
  HTTPEnable : 0
  HTTPSEnable : 1
  HTTPSPort : 7890
  HTTPSProxy : 127.0.0.1
  ProxyAutoConfigEnable : 1
  ProxyAutoConfigURLString : http://127.0.0.1:7890/pac
  SOCKSEnable : 0
}";
        let proxy = scutil_proxy(text);
        assert!(proxy.enabled);
        assert_eq!(proxy.server, "127.0.0.1:7890");
        assert_eq!(proxy.auto_config_url, "http://127.0.0.1:7890/pac");
        assert_eq!(scutil_proxy("<dictionary> {\n  HTTPEnable : 0\n}"), SystemProxy::default());
    }
}

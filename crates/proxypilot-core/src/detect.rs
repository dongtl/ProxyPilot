//! Finding the user's proxy client: which local ports speak SOCKS5, and who owns them.

use crate::catalog::{COMMON_PORTS, client_label};
use crate::model::{Candidate, CandidateStatus};
use crate::probe::socks5_greeting;
use crate::sys;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

const GREETING_TIMEOUT: Duration = Duration::from_millis(300);

/// A listening socket with its owner's executable name resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    pub ip: IpAddr,
    pub port: u16,
    pub pid: u32,
    pub process: Option<String>,
}

/// Listeners reachable from this machine: loopback or all-interfaces only.
/// A socket bound to one LAN address is somebody's server, not a local proxy.
pub fn listeners() -> Vec<Listener> {
    let names: HashMap<u32, String> =
        sys::processes().unwrap_or_default().into_iter().map(|process| (process.pid, process.name)).collect();
    sys::tcp_listeners()
        .unwrap_or_default()
        .into_iter()
        .filter(|listener| listener.ip.is_loopback() || listener.ip.is_unspecified())
        .map(|listener| Listener {
            ip: listener.ip,
            port: listener.port,
            pid: listener.pid,
            process: names.get(&listener.pid).cloned(),
        })
        .collect()
}

/// One connect address per port, preferring IPv4 loopback, filtered to the
/// ports worth a greeting. `exclude` holds our own pids (the app and its core).
pub fn select_targets(listeners: &[Listener], full: bool, exclude: &[u32]) -> Vec<(IpAddr, Listener)> {
    let mut by_port: HashMap<u16, (IpAddr, Listener)> = HashMap::new();
    for listener in listeners {
        if sys::kernel_pid(listener.pid) || (listener.pid != 0 && exclude.contains(&listener.pid)) {
            continue;
        }
        let known = client_label(listener.process.as_deref()).1;
        if !full && !known && !COMMON_PORTS.contains(&listener.port) {
            continue;
        }
        let connect: IpAddr = match listener.ip {
            IpAddr::V4(_) => IpAddr::from([127, 0, 0, 1]),
            IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::from([127, 0, 0, 1]),
            IpAddr::V6(_) => IpAddr::from(std::net::Ipv6Addr::LOCALHOST),
        };
        let replace = match by_port.get(&listener.port) {
            None => true,
            Some((existing, _)) => existing.is_ipv6() && connect.is_ipv4(),
        };
        if replace {
            by_port.insert(listener.port, (connect, listener.clone()));
        }
    }
    let mut targets: Vec<_> = by_port.into_values().collect();
    targets.sort_by_key(|(_, listener)| listener.port);
    targets
}

/// Usable first, then recognised clients, then the usual ports, then by number.
pub fn rank(candidates: &mut [Candidate]) {
    candidates.sort_by_key(|candidate| {
        (
            candidate.status != CandidateStatus::Usable,
            !candidate.known,
            COMMON_PORTS.iter().position(|port| *port == candidate.port).unwrap_or(usize::MAX),
            candidate.port,
        )
    });
}

pub fn detect(full: bool, exclude: &[u32]) -> Vec<Candidate> {
    let targets = select_targets(&listeners(), full, exclude);
    let mut found: Vec<Candidate> = Vec::new();
    // Greetings are local and short, but a full scan can cover a hundred
    // ports; a few threads keep it around a second.
    std::thread::scope(|scope| {
        let handles: Vec<_> = targets
            .chunks(8)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|(ip, listener)| {
                            let status = socks5_greeting(*ip, listener.port, GREETING_TIMEOUT)?;
                            let (client, known) = client_label(listener.process.as_deref());
                            Some(Candidate {
                                port: listener.port,
                                host: ip.to_string(),
                                process: listener.process.clone(),
                                // 0: the owner is another user's process we cannot see.
                                pid: (listener.pid != 0).then_some(listener.pid),
                                client,
                                status,
                                known,
                            })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            if let Ok(chunk) = handle.join() {
                found.extend(chunk);
            }
        }
    });
    rank(&mut found);
    found
}

/// Who owns this local port right now. Used to keep the upstream client's own
/// traffic direct, and to notice when the user switched clients.
pub fn port_owner(port: u16) -> Option<(u32, Option<String>)> {
    listeners()
        .into_iter()
        .find(|listener| listener.port == port && listener.pid != 0 && !sys::kernel_pid(listener.pid))
        .map(|listener| (listener.pid, listener.process))
}

/// Owner of the upstream's port, only when the upstream is on this machine.
/// For an upstream on another LAN host, a local process on the same port
/// number is unrelated and must not be added to the direct list.
pub fn upstream_owner(upstream: &crate::model::Upstream) -> Option<(u32, Option<String>)> {
    let ip = upstream.ip()?;
    if !(ip.is_loopback() || ip.is_unspecified()) {
        return None;
    }
    port_owner(upstream.port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(ip: &str, port: u16, pid: u32, process: &str) -> Listener {
        Listener { ip: ip.parse().unwrap(), port, pid, process: Some(process.into()) }
    }

    #[test]
    fn default_scan_keeps_known_clients_and_common_ports_only() {
        let all = vec![
            listener("127.0.0.1", 10808, 100, "xray.exe"),
            listener("127.0.0.1", 5432, 200, "postgres.exe"),
            listener("127.0.0.1", 7890, 300, "something.exe"),
            listener("127.0.0.1", 30000, 400, "mihomo.exe"),
            listener("0.0.0.0", 445, 4, "System"),
        ];
        let ports: Vec<u16> = select_targets(&all, false, &[]).iter().map(|(_, l)| l.port).collect();
        assert_eq!(ports, vec![7890, 10808, 30000]);
        let ports: Vec<u16> = select_targets(&all, true, &[]).iter().map(|(_, l)| l.port).collect();
        assert_eq!(ports, vec![5432, 7890, 10808, 30000]);
    }

    #[test]
    fn excludes_our_own_processes_and_prefers_ipv4() {
        let all = vec![
            listener("::1", 10808, 100, "xray.exe"),
            listener("127.0.0.1", 10808, 100, "xray.exe"),
            listener("::", 7897, 300, "verge-mihomo.exe"),
            listener("127.0.0.1", 9090, 999, "sing-box.exe"),
        ];
        let targets = select_targets(&all, true, &[999]);
        assert_eq!(targets.len(), 2);
        assert!(targets.iter().all(|(ip, _)| ip.is_ipv4()));
    }

    #[test]
    fn ranking_puts_usable_known_common_ports_first() {
        let make = |port, status, known| Candidate {
            port,
            host: "127.0.0.1".into(),
            process: None,
            pid: None,
            client: String::new(),
            status,
            known,
        };
        let mut list = vec![
            make(30000, CandidateStatus::Usable, false),
            make(7890, CandidateStatus::NeedsAuth, true),
            make(7897, CandidateStatus::Usable, true),
            make(10808, CandidateStatus::Usable, true),
        ];
        rank(&mut list);
        let ports: Vec<u16> = list.iter().map(|c| c.port).collect();
        assert_eq!(ports, vec![10808, 7897, 30000, 7890]);
    }
}

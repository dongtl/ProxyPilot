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

/// The one proxy client running, by label, for a port whose owner we cannot
/// see: on macOS the app is an ordinary user and a root-run core's sockets are
/// hidden from it, but the process list is not. `None` when there is no such
/// client or more than one, so a guess is never a coin toss. sing-box is left
/// out: the helper runs ours as root, and from outside it looks like any other.
pub fn guess_client(programs: &[String]) -> Option<String> {
    let mut labels: Vec<String> = Vec::new();
    for program in programs {
        if crate::platform::name_key(program) == "sing-box" {
            continue;
        }
        let (label, known) = client_label(Some(program));
        if known && !labels.contains(&label) {
            labels.push(label);
        }
    }
    match labels.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    }
}

/// What each running process is, as `guess_client` wants it: the outermost
/// `.app` it runs from, else its executable name.
fn running_programs() -> Vec<String> {
    sys::processes()
        .unwrap_or_default()
        .into_iter()
        .filter(|process| process.pid != 0 && !sys::kernel_pid(process.pid))
        .map(|process| match sys::process_path(process.pid) {
            Some(path) => crate::platform::program_of(&path),
            None => process.name,
        })
        .collect()
}

/// Usable first, then recognised (or guessed) clients, then the usual ports, then by number.
pub fn rank(candidates: &mut [Candidate]) {
    candidates.sort_by_key(|candidate| {
        (
            candidate.status != CandidateStatus::Usable,
            !(candidate.known || candidate.guessed),
            COMMON_PORTS.iter().position(|port| *port == candidate.port).unwrap_or(usize::MAX),
            candidate.port,
        )
    });
}

pub fn detect(full: bool, exclude: &[u32]) -> Vec<Candidate> {
    let targets = select_targets(&listeners(), full, exclude);
    // Only worth the process walk when some port's owner is hidden from us.
    let guess = if targets.iter().any(|(_, listener)| listener.pid == 0) { guess_client(&running_programs()) } else { None };
    let guess = guess.as_ref();
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
                            let (mut client, known) = client_label(listener.process.as_deref());
                            // 0: the owner is another user's process we cannot see.
                            let hidden = listener.pid == 0;
                            let guessed = hidden && guess.is_some();
                            if let Some(label) = guess.filter(|_| hidden) {
                                client = label.clone();
                            }
                            Some(Candidate {
                                port: listener.port,
                                host: ip.to_string(),
                                process: listener.process.clone(),
                                pid: (!hidden).then_some(listener.pid),
                                client,
                                status,
                                known,
                                guessed,
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

/// Label of the client behind the upstream, and whether it is a guess (the
/// port's owner is hidden, see `guess_client`). Only for an upstream on this machine.
pub fn upstream_client(upstream: &crate::model::Upstream) -> (Option<String>, bool) {
    if let Some((_, name)) = upstream_owner(upstream) {
        return (name.map(|name| client_label(Some(&name)).0), false);
    }
    let local = upstream.ip().is_some_and(|ip| ip.is_loopback() || ip.is_unspecified());
    let hidden = local && listeners().iter().any(|listener| listener.port == upstream.port && listener.pid == 0);
    match hidden.then(|| guess_client(&running_programs())).flatten() {
        Some(label) => (Some(label), true),
        None => (None, false),
    }
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
        // pid 4 is Windows' System process; elsewhere it is an ordinary program.
        let full = if cfg!(windows) { vec![5432, 7890, 10808, 30000] } else { vec![445, 5432, 7890, 10808, 30000] };
        assert_eq!(ports, full);
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
            guessed: false,
        };
        let guessed = Candidate { guessed: true, ..make(7891, CandidateStatus::Usable, false) };
        let mut list = vec![
            make(30000, CandidateStatus::Usable, false),
            make(7890, CandidateStatus::NeedsAuth, true),
            make(7897, CandidateStatus::Usable, true),
            make(10808, CandidateStatus::Usable, true),
            guessed,
        ];
        rank(&mut list);
        let ports: Vec<u16> = list.iter().map(|c| c.port).collect();
        assert_eq!(ports, vec![10808, 7891, 7897, 30000, 7890]);
    }

    #[test]
    fn guesses_only_when_exactly_one_proxy_client_runs() {
        let names = |list: &[&str]| list.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
        // FlClash's app and its core are one client; our own sing-box and ordinary programs do not count.
        assert_eq!(guess_client(&names(&["Finder", "FlClash.app", "FlClashCore", "sing-box"])), Some("FlClash".into()));
        assert_eq!(guess_client(&names(&["Clash Verge.app"])), Some("Clash Verge".into()));
        assert_eq!(guess_client(&names(&["FlClash.app", "Clash Verge.app"])), None);
        assert_eq!(guess_client(&names(&["Finder", "sing-box", "Safari.app"])), None);
    }
}

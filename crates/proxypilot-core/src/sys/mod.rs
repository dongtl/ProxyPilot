//! Operating-system facts the rest of the crate needs, behind one small API.
//! Windows and macOS are implemented; other platforms get empty answers, so
//! the logic above still compiles and tests everywhere.

use std::net::IpAddr;

pub mod parse;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerInfo {
    pub ip: IpAddr,
    pub port: u16,
    /// 0 when the owner could not be seen (macOS: another user's process).
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceInfo {
    pub alias: String,
    pub description: String,
    pub up: bool,
}

/// The system proxy browsers follow regardless of any TUN: WinINet's
/// per-user setting on Windows, the active network service's on macOS.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemProxy {
    pub enabled: bool,
    pub server: String,
    pub auto_config_url: String,
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;

#[cfg(not(any(windows, target_os = "macos")))]
mod other;
#[cfg(not(any(windows, target_os = "macos")))]
pub use self::other::*;

/// Whether an adapter with this alias exists and is up.
pub fn interface_up(alias: &str) -> bool {
    interfaces()
        .map(|list| list.iter().any(|item| item.up && item.alias.eq_ignore_ascii_case(alias)))
        .unwrap_or(false)
}

/// Whether our TUN adapter is up. Windows lets us name it; macOS numbers its
/// `utunN` adapters itself, so there it is the one holding our address.
pub fn tun_present() -> bool {
    #[cfg(windows)]
    return interface_up(crate::config::INTERFACE_NAME);
    #[cfg(not(windows))]
    return has_address(crate::config::TUN_ADDRESS.parse().expect("valid address"));
}

/// Pids that are the kernel rather than a program: Windows' idle (0) and
/// System (4) processes, which own SMB, HTTP.sys and the like. Elsewhere 0
/// only means the owner is unknown, and the port is still worth a look.
pub fn kernel_pid(pid: u32) -> bool {
    cfg!(windows) && pid <= 4
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;

    /// The real system calls, on whichever system runs the tests. On macOS
    /// this is the only check they get before a tester's Mac.
    #[test]
    fn sees_our_own_listener_process_and_loopback() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let me = std::process::id();
        let found = tcp_listeners().unwrap();
        assert!(found.iter().any(|l| l.port == port && l.pid == me), "own listener on {port} not found");
        let list = processes().unwrap();
        assert!(list.iter().any(|p| p.pid == me && !p.name.is_empty()));
        assert!(process_path(me).is_some_and(|path| path.contains("proxypilot_core")));
        assert!(interfaces().unwrap().iter().any(|i| i.up));
        let mut bytes = [0u8; 300];
        random_bytes(&mut bytes).unwrap();
        assert!(bytes.iter().any(|b| *b != 0));
        #[cfg(target_os = "macos")]
        assert!(has_address("127.0.0.1".parse().unwrap()));
        let _ = system_proxy();
    }
}

/// `platform::name_key`s of running processes, for catalogue matching. On
/// macOS the bundles they run from are added, so `aTrust.app` matches too.
pub fn running_process_names() -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    for process in processes().unwrap_or_default() {
        names.insert(crate::platform::name_key(&process.name));
        if cfg!(target_os = "macos")
            && let Some(bundle) = process_path(process.pid).as_deref().and_then(crate::platform::bundle_of)
        {
            names.insert(crate::platform::name_key(&bundle));
        }
    }
    names
}

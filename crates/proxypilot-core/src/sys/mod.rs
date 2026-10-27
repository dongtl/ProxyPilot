//! Operating-system facts the rest of the crate needs, behind one small API.
//! Windows is implemented; other platforms get empty answers until their shell
//! exists, so the logic above still compiles and tests everywhere.

use std::net::IpAddr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerInfo {
    pub ip: IpAddr,
    pub port: u16,
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

/// The per-user WinINet proxy, which browsers follow regardless of any TUN.
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

#[cfg(not(windows))]
mod other;
#[cfg(not(windows))]
pub use self::other::*;

/// Whether an adapter with this alias exists and is up.
pub fn interface_up(alias: &str) -> bool {
    interfaces()
        .map(|list| list.iter().any(|item| item.up && item.alias.eq_ignore_ascii_case(alias)))
        .unwrap_or(false)
}

/// Lower-cased names of running processes, for catalogue matching.
pub fn running_process_names() -> std::collections::HashSet<String> {
    processes().unwrap_or_default().into_iter().map(|process| process.name.to_lowercase()).collect()
}

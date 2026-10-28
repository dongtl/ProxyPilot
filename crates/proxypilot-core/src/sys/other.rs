//! Placeholders for platforms without a shell yet. Each answers "nothing
//! found" rather than failing, so callers need no platform checks.

use super::{InterfaceInfo, ListenerInfo, ProcessInfo, SystemProxy};
use std::io;
use std::process::{Child, Command};

pub fn tcp_listeners() -> io::Result<Vec<ListenerInfo>> {
    Ok(Vec::new())
}

pub fn processes() -> io::Result<Vec<ProcessInfo>> {
    Ok(Vec::new())
}

pub fn process_path(_pid: u32) -> Option<String> {
    None
}

pub fn interfaces() -> io::Result<Vec<InterfaceInfo>> {
    Ok(Vec::new())
}

pub fn has_address(_ip: std::net::IpAddr) -> bool {
    false
}

pub fn is_elevated() -> bool {
    false
}

pub fn system_proxy() -> SystemProxy {
    SystemProxy::default()
}

pub fn security_descriptor(_path: &std::path::Path) -> io::Result<String> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "SDDL is Windows-only"))
}

pub fn random_bytes(_buf: &mut [u8]) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "no secure random source wired for this platform"))
}

pub fn hide_window(_command: &mut Command) {}

pub struct Job;

impl Job {
    pub fn new() -> io::Result<Job> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "job objects are Windows-only"))
    }

    pub fn assign(&self, _child: &Child) -> io::Result<()> {
        Ok(())
    }
}

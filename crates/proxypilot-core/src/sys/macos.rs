//! macOS calls: libproc for processes, getifaddrs for adapters, and the
//! system tools (netstat, lsof, scutil) whose output `parse` reads. Socket
//! owners of other users, root-run proxy cores included, are invisible to an
//! ordinary process; such ports come back with pid 0 and are still probed.

use super::{InterfaceInfo, ListenerInfo, ProcessInfo, SystemProxy, parse};
use std::ffi::CStr;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// stdout of a system tool, or empty. lsof exits 1 when nothing matched.
fn tool_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

pub fn tcp_listeners() -> io::Result<Vec<ListenerInfo>> {
    let all = parse::netstat_listeners(&tool_output("/usr/sbin/netstat", &["-an", "-p", "tcp"]));
    let owned = parse::lsof_listeners(&tool_output("/usr/sbin/lsof", &["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpn"]));
    Ok(parse::merge_listeners(all, &owned))
}

fn pids() -> io::Result<Vec<c_int>> {
    let mut capacity = 1024usize;
    for _ in 0..6 {
        let mut buffer = vec![0 as c_int; capacity];
        let bytes = (capacity * size_of::<c_int>()) as c_int;
        let count = unsafe { libc::proc_listallpids(buffer.as_mut_ptr().cast::<c_void>(), bytes) };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        // A full buffer may have been cut short; grow and ask again.
        if (count as usize) < capacity {
            buffer.truncate(count as usize);
            return Ok(buffer);
        }
        capacity *= 2;
    }
    Err(io::Error::other("process list kept growing"))
}

fn proc_name(pid: c_int) -> Option<String> {
    let mut buffer = [0u8; 256];
    let len = unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast::<c_void>(), buffer.len() as u32) };
    (len > 0).then(|| String::from_utf8_lossy(&buffer[..len as usize]).into_owned())
}

pub fn process_path(pid: u32) -> Option<String> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let len = unsafe { libc::proc_pidpath(pid as c_int, buffer.as_mut_ptr().cast::<c_void>(), buffer.len() as u32) };
    (len > 0).then(|| String::from_utf8_lossy(&buffer[..len as usize]).into_owned())
}

/// Names are the executable's file name, as sing-box matches them; proc_name
/// alone would cut them at 16 characters.
pub fn processes() -> io::Result<Vec<ProcessInfo>> {
    Ok(pids()?
        .into_iter()
        .filter(|pid| *pid > 0)
        .filter_map(|pid| {
            let name = process_path(pid as u32)
                .and_then(|path| path.rsplit('/').next().map(str::to_owned))
                .filter(|name| !name.is_empty())
                .or_else(|| proc_name(pid))?;
            Some(ProcessInfo { pid: pid as u32, name })
        })
        .collect())
}

/// getifaddrs, freed on drop.
struct Addrs(*mut libc::ifaddrs);

impl Addrs {
    fn get() -> io::Result<Addrs> {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        if unsafe { libc::getifaddrs(&mut head) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Addrs(head))
    }

    /// (name, up, address) for each entry.
    fn entries(&self) -> Vec<(String, bool, Option<IpAddr>)> {
        let mut out = Vec::new();
        let mut cursor = self.0;
        while !cursor.is_null() {
            let entry = unsafe { &*cursor };
            let name = unsafe { CStr::from_ptr(entry.ifa_name as *const c_char) }.to_string_lossy().into_owned();
            let up = entry.ifa_flags & libc::IFF_UP as u32 != 0;
            out.push((name, up, unsafe { address(entry.ifa_addr) }));
            cursor = entry.ifa_next;
        }
        out
    }
}

impl Drop for Addrs {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { libc::freeifaddrs(self.0) };
        }
    }
}

unsafe fn address(addr: *const libc::sockaddr) -> Option<IpAddr> {
    if addr.is_null() {
        return None;
    }
    unsafe {
        match (*addr).sa_family as c_int {
            libc::AF_INET => {
                let v4 = &*(addr as *const libc::sockaddr_in);
                Some(IpAddr::V4(Ipv4Addr::from(u32::from_be(v4.sin_addr.s_addr))))
            }
            libc::AF_INET6 => {
                let v6 = &*(addr as *const libc::sockaddr_in6);
                Some(IpAddr::V6(Ipv6Addr::from(v6.sin6_addr.s6_addr)))
            }
            _ => None,
        }
    }
}

/// One row per adapter. macOS has no adapter descriptions; the alias is all there is.
pub fn interfaces() -> io::Result<Vec<InterfaceInfo>> {
    let mut out: Vec<InterfaceInfo> = Vec::new();
    for (name, up, _) in Addrs::get()?.entries() {
        if !out.iter().any(|known| known.alias == name) {
            out.push(InterfaceInfo { alias: name, description: String::new(), up });
        }
    }
    Ok(out)
}

/// Whether an adapter that is up holds `ip`.
pub fn has_address(ip: IpAddr) -> bool {
    Addrs::get().is_ok_and(|addrs| addrs.entries().iter().any(|(_, up, address)| *up && *address == Some(ip)))
}

/// Root. The app itself never is; only the helper that runs sing-box.
pub fn is_elevated() -> bool {
    unsafe { libc::geteuid() == 0 }
}

pub fn system_proxy() -> SystemProxy {
    parse::scutil_proxy(&tool_output("/usr/sbin/scutil", &["--proxy"]))
}

pub fn security_descriptor(_path: &Path) -> io::Result<String> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "SDDL is Windows-only"))
}

pub fn random_bytes(buf: &mut [u8]) -> io::Result<()> {
    // getentropy hands out at most 256 bytes per call.
    for chunk in buf.chunks_mut(256) {
        if unsafe { libc::getentropy(chunk.as_mut_ptr().cast::<c_void>(), chunk.len()) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn hide_window(_command: &mut Command) {}

/// macOS has no job objects. The helper runs sing-box as its own child and
/// launchd kills the helper's process group when it stops, which gives the
/// same "the core never outlives its owner" guarantee.
pub struct Job;

impl Job {
    pub fn new() -> io::Result<Job> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "job objects are Windows-only"))
    }

    pub fn assign(&self, _child: &Child) -> io::Result<()> {
        Ok(())
    }
}

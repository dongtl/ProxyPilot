//! Win32 calls. Each function owns its handles and frees them before returning.

use super::{InterfaceInfo, ListenerInfo, ProcessInfo, SystemProxy};
use std::io;
use std::mem::{size_of, zeroed};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, INVALID_HANDLE_VALUE, LocalFree, NO_ERROR,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetExtendedTcpTable, GetIfTable2, MIB_IF_TABLE2, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID,
    MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows_sys::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, TOKEN_ELEVATION,
    TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// GetExtendedTcpTable into a u32-aligned buffer, growing it until it fits.
fn tcp_table(family: u32) -> io::Result<Vec<u32>> {
    let mut buffer: Vec<u32> = Vec::new();
    let mut size: u32 = 0;
    for _ in 0..8 {
        let pointer = if buffer.is_empty() { null_mut() } else { buffer.as_mut_ptr().cast() };
        let result = unsafe { GetExtendedTcpTable(pointer, &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0) };
        if result == NO_ERROR && !buffer.is_empty() {
            return Ok(buffer);
        }
        if result != ERROR_INSUFFICIENT_BUFFER && result != NO_ERROR {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
        // The table can grow between calls; leave headroom.
        let words = (size as usize).div_ceil(4) + 64;
        buffer = vec![0u32; words];
        size = (words * 4) as u32;
    }
    Err(io::Error::other("TCP table kept growing"))
}

pub fn tcp_listeners() -> io::Result<Vec<ListenerInfo>> {
    let mut out = Vec::new();
    let v4 = tcp_table(AF_INET as u32)?;
    unsafe {
        let table = v4.as_ptr() as *const MIB_TCPTABLE_OWNER_PID;
        let count = (*table).dwNumEntries as usize;
        let capacity = (v4.len() * 4 - size_of::<u32>()) / size_of::<MIB_TCPROW_OWNER_PID>();
        let rows = std::slice::from_raw_parts((*table).table.as_ptr(), count.min(capacity));
        for row in rows {
            out.push(ListenerInfo {
                ip: IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes())),
                port: u16::from_be(row.dwLocalPort as u16),
                pid: row.dwOwningPid,
            });
        }
    }
    // IPv6 can be disabled; IPv4 listeners are still worth returning.
    if let Ok(v6) = tcp_table(AF_INET6 as u32) {
        unsafe {
            let table = v6.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID;
            let count = (*table).dwNumEntries as usize;
            let capacity = (v6.len() * 4 - size_of::<u32>()) / size_of::<MIB_TCP6ROW_OWNER_PID>();
            let rows = std::slice::from_raw_parts((*table).table.as_ptr(), count.min(capacity));
            for row in rows {
                out.push(ListenerInfo {
                    ip: IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
                    port: u16::from_be(row.dwLocalPort as u16),
                    pid: row.dwOwningPid,
                });
            }
        }
    }
    Ok(out)
}

pub fn processes() -> io::Result<Vec<ProcessInfo>> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut out = Vec::new();
        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snapshot, &mut entry);
        while ok != 0 {
            out.push(ProcessInfo { pid: entry.th32ProcessID, name: from_wide(&entry.szExeFile) });
            ok = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        Ok(out)
    }
}

pub fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        (ok != 0).then(|| String::from_utf16_lossy(&buffer[..len as usize]))
    }
}

pub fn interfaces() -> io::Result<Vec<InterfaceInfo>> {
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = null_mut();
        let result = GetIfTable2(&mut table);
        if result != NO_ERROR {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
        let count = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), count);
        let mut out: Vec<InterfaceInfo> = Vec::new();
        for row in rows {
            // Filter drivers (WFP, QoS, ...) appear as extra rows named after
            // the adapter they sit on; they are not adapters of their own.
            let filter = row.InterfaceAndOperStatusFlags._bitfield & 0b10 != 0;
            if filter {
                continue;
            }
            out.push(InterfaceInfo {
                alias: from_wide(&row.Alias),
                description: from_wide(&row.Description),
                up: row.OperStatus == IfOperStatusUp,
            });
        }
        FreeMibTable(table.cast());
        Ok(out)
    }
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// Owner and DACL of a file or folder as SDDL, for `acl::admin_only`.
pub fn security_descriptor(path: &Path) -> io::Result<String> {
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let info = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    unsafe {
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        let status = GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            info,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        );
        if status != NO_ERROR {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let mut text: *mut u16 = null_mut();
        let mut len = 0u32;
        let ok = ConvertSecurityDescriptorToStringSecurityDescriptorW(descriptor, SDDL_REVISION_1, info, &mut text, &mut len);
        let result = if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            let sddl = from_wide(std::slice::from_raw_parts(text, len as usize));
            LocalFree(text.cast());
            Ok(sddl)
        };
        LocalFree(descriptor);
        result
    }
}

const INTERNET_SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

fn reg_dword(subkey: &str, value: &str) -> Option<u32> {
    let (subkey, value) = (wide(subkey), wide(value));
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&mut data as *mut u32).cast(),
            &mut size,
        )
    };
    (result == NO_ERROR).then_some(data)
}

fn reg_string(subkey: &str, value: &str) -> Option<String> {
    let (subkey, value) = (wide(subkey), wide(value));
    let mut buffer = vec![0u16; 2048];
    let mut size = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    (result == NO_ERROR).then(|| from_wide(&buffer))
}

pub fn system_proxy() -> SystemProxy {
    SystemProxy {
        enabled: reg_dword(INTERNET_SETTINGS, "ProxyEnable").unwrap_or(0) != 0,
        server: reg_string(INTERNET_SETTINGS, "ProxyServer").unwrap_or_default(),
        auto_config_url: reg_string(INTERNET_SETTINGS, "AutoConfigURL").unwrap_or_default(),
    }
}

pub fn random_bytes(buf: &mut [u8]) -> io::Result<()> {
    let status = unsafe { BCryptGenRandom(null_mut(), buf.as_mut_ptr(), buf.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status != 0 {
        return Err(io::Error::other(format!("BCryptGenRandom failed: {status:#x}")));
    }
    Ok(())
}

pub fn hide_window(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

/// A Job Object that kills its processes when the last handle closes, which
/// happens when we exit for any reason, including a crash.
pub struct Job(HANDLE);

// The handle is only used through Win32 calls that are thread-safe.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    pub fn new() -> io::Result<Job> {
        unsafe {
            let handle = CreateJobObjectW(null(), null());
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                let error = io::Error::last_os_error();
                CloseHandle(handle);
                return Err(error);
            }
            Ok(Job(handle))
        }
    }

    pub fn assign(&self, child: &Child) -> io::Result<()> {
        let ok = unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

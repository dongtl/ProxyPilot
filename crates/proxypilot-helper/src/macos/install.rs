//! Putting the helper in place and taking it away. Only files that match the
//! manifest signed with the release key are ever written, whether the user
//! typed their password for an install or the app asked for an upgrade.

use super::fsx::{ensure_system_dir, read_regular, root_dir, write_root_file};
use super::VERSION;
use proxypilot_core::helper::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_BINARY: u64 = 256 * 1024 * 1024;
const MAX_SMALL: u64 = 1024 * 1024;
pub const PID_FILE: &str = "sing-box.pid";

pub fn install_dir() -> PathBuf {
    PathBuf::from(INSTALL_DIR)
}

pub fn data_dir() -> PathBuf {
    PathBuf::from(DATA_DIR)
}

/// The helper and sing-box from `dir`, checked against its signed manifest.
/// `exact`: the manifest must be for this very helper (an install from the
/// bundle it ships in); otherwise it must not be older (an upgrade).
pub fn verified_files(dir: &Path, exact: bool) -> Result<(Manifest, Vec<u8>, Vec<u8>), String> {
    let manifest_bytes = read_regular(&dir.join(MANIFEST_FILE), MAX_SMALL)?;
    let signature = String::from_utf8(read_regular(&dir.join(SIGNATURE_FILE), MAX_SMALL)?).map_err(|_| "签名文件格式不对".to_owned())?;
    let manifest = verify_manifest(&manifest_bytes, &signature, RELEASE_KEY)?;
    let allowed = if exact { manifest.version == VERSION } else { may_replace(VERSION, &manifest.version) };
    if !allowed {
        return Err(format!("后台助手 {} 不能换成 {}", VERSION, manifest.version));
    }
    let helper = checked_file(&manifest, HELPER_FILE, read_regular(&dir.join(HELPER_FILE), MAX_BINARY)?)?;
    let sing_box = checked_file(&manifest, SING_BOX_FILE, read_regular(&dir.join(SING_BOX_FILE), MAX_BINARY)?)?;
    Ok((manifest, helper, sing_box))
}

/// Write the checked files into the root-owned install folder.
pub fn place_files(helper: &[u8], sing_box: &[u8]) -> Result<(), String> {
    ensure_system_dir(Path::new("/Library/PrivilegedHelperTools"))?;
    root_dir(&install_dir(), 0o755)?;
    write_root_file(&install_dir().join(SING_BOX_FILE), sing_box, 0o755)?;
    // The helper last: a half-finished upgrade keeps the old, working helper.
    write_root_file(&install_dir().join(HELPER_FILE), helper, 0o755)
}

fn launchctl(args: &[&str]) -> bool {
    Command::new("/bin/launchctl").args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

pub fn install(owner: u32, from: Option<&str>) -> Result<(), String> {
    let dir = match from {
        Some(dir) => PathBuf::from(dir),
        None => std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)).ok_or("找不到安装文件所在的文件夹")?,
    };
    let (_, helper, sing_box) = verified_files(&dir, true)?;
    kill_stale_core();
    launchctl(&["bootout", &format!("system/{LABEL}")]);
    place_files(&helper, &sing_box)?;
    root_dir(&data_dir(), 0o700)?;
    let program = install_dir().join(HELPER_FILE).to_string_lossy().into_owned();
    let log = data_dir().join("helper.log").to_string_lossy().into_owned();
    let owner = owner.to_string();
    let plist = launchd_plist(LABEL, &[&program, "serve", "--owner", &owner], true, Some(&log));
    write_root_file(Path::new(PLIST), plist.as_bytes(), 0o644)?;
    if !launchctl(&["bootstrap", "system", PLIST]) {
        return Err("无法注册后台助手（launchctl bootstrap 失败）".into());
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if std::os::unix::net::UnixStream::connect(SOCKET).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err("后台助手没有启动".into())
}

/// Remove the regular files directly inside `dir`, then `dir` itself if it
/// is empty. Never recursive: nothing we did not put there is touched.
fn remove_dir_files(dir: &Path) {
    let Ok(meta) = fs::symlink_metadata(dir) else { return };
    if !meta.is_dir() {
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_file() || kind.is_symlink()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let _ = fs::remove_dir(dir);
}

pub fn uninstall() -> Result<(), String> {
    kill_stale_core();
    launchctl(&["bootout", &format!("system/{LABEL}")]);
    let _ = fs::remove_file(PLIST);
    let _ = fs::remove_file(SOCKET);
    remove_dir_files(&install_dir());
    remove_dir_files(&data_dir());
    if Path::new(PLIST).exists() || install_dir().exists() {
        return Err("后台助手没有完全移除".into());
    }
    Ok(())
}

/// Stop a sing-box the helper left running, e.g. when it crashed. Only the
/// process whose executable is our installed copy: never anyone else's sing-box.
pub fn kill_stale_core() {
    let pid_file = data_dir().join(PID_FILE);
    let Some(pid) = fs::read_to_string(&pid_file).ok().and_then(|text| text.trim().parse::<i32>().ok()) else { return };
    let ours = install_dir().join(SING_BOX_FILE);
    if pid > 1 && proxypilot_core::sys::process_path(pid as u32).is_some_and(|path| Path::new(&path) == ours) {
        unsafe { libc::kill(pid, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && proxypilot_core::sys::process_path(pid as u32).is_some() {
            std::thread::sleep(Duration::from_millis(100));
        }
        if proxypilot_core::sys::process_path(pid as u32).is_some_and(|path| Path::new(&path) == ours) {
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
    let _ = fs::remove_file(pid_file);
}

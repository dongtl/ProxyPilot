//! Files the helper runs as root must be changeable by root only: anything
//! another user could rewrite would hand them root. Everything here refuses
//! symlinks and checks owner and mode, never trusting what a path points to.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

/// Owned by root, not writable by group or others, and not a symlink.
pub fn root_only(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| !meta.file_type().is_symlink() && meta.uid() == 0 && meta.mode() & 0o022 == 0)
}

/// `path` and every folder above it are `root_only`; `None` when they all
/// are, otherwise the first one that is not.
pub fn unprotected(path: &Path) -> Option<std::path::PathBuf> {
    path.ancestors().find(|part| !part.as_os_str().is_empty() && !root_only(part)).map(Path::to_path_buf)
}

/// Create `dir` (one level) owned by root with `mode`, or check an existing one.
pub fn root_dir(dir: &Path, mode: u32) -> Result<(), String> {
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err(format!("{} 不是普通文件夹，已停止安装", dir.display()));
        }
        Ok(_) => {}
        Err(_) => fs::create_dir(dir).map_err(|error| format!("无法创建 {}：{error}", dir.display()))?,
    }
    std::os::unix::fs::chown(dir, Some(0), Some(0)).map_err(|error| format!("无法设置 {} 的所有者：{error}", dir.display()))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(mode)).map_err(|error| format!("无法设置 {} 的权限：{error}", dir.display()))
}

/// Write `bytes` to `path` as root with `mode`: a temporary file next to it,
/// flushed, then renamed over, so a reader never sees half a file and a
/// symlink at `path` is replaced rather than followed.
pub fn write_root_file(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let temporary = path.with_extension("new");
    let _ = fs::remove_file(&temporary);
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::os::unix::fs::chown(&temporary, Some(0), Some(0))?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| format!("无法写入 {}：{error}", path.display()))
}

/// Create a system folder (`/Library/PrivilegedHelperTools`) if it is
/// missing. An existing one is the system's and is left exactly as it is.
pub fn ensure_system_dir(dir: &Path) -> Result<(), String> {
    if fs::symlink_metadata(dir).is_ok() {
        return Ok(());
    }
    root_dir(dir, 0o755)
}

/// Read a file the caller named, refusing anything but a regular file.
pub fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|error| format!("找不到 {}：{error}", path.display()))?;
    if !meta.is_file() || meta.len() > limit {
        return Err(format!("{} 不是有效的文件", path.display()));
    }
    fs::read(path).map_err(|error| format!("无法读取 {}：{error}", path.display()))
}

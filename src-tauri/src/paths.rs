//! Where things live, and the app's own log.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

const APP_LOG_LIMIT: u64 = 1024 * 1024;

pub const UNPROTECTED: &str = "安装目录的权限不安全：普通程序也能替换 ProxyPilot 以管理员身份运行的文件。\
    为安全起见已停用分流和开机自启，请用最新的安装包重新安装，安装程序会自动修正权限";

pub struct AppPaths {
    pub data: PathBuf,
    pub settings: PathBuf,
    pub runtime: PathBuf,
    pub app_log: PathBuf,
}

impl AppPaths {
    pub fn resolve(app: &tauri::AppHandle) -> Result<AppPaths, String> {
        // The override exists for tests and side-by-side dev setups only.
        // Release builds run elevated, and an elevated process must not take
        // its working directory from the user's environment.
        let data = match std::env::var_os("PROXYPILOT_HOME").filter(|_| cfg!(debug_assertions)) {
            Some(path) => PathBuf::from(path),
            None => app.path().app_local_data_dir().map_err(|error| format!("找不到数据目录：{error}"))?,
        };
        let runtime = runtime_dir().unwrap_or_else(|| data.join("runtime"));
        for dir in [&data, &runtime] {
            fs::create_dir_all(dir).map_err(|error| format!("无法创建数据目录 {}：{error}", dir.display()))?;
        }
        Ok(AppPaths { settings: data.join("settings.json"), app_log: data.join("app.log"), runtime, data })
    }
}

/// sing-box's config, working folder and logs. Installed, they sit in the
/// install folder, which only administrators can change: sing-box runs
/// elevated, and a config ordinary programs could rewrite would let them have
/// it write files anywhere. Dev builds keep them with the other data.
fn runtime_dir() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return None;
    }
    Some(std::env::current_exe().ok()?.parent()?.join("runtime"))
}

/// The first of the places whose contents run elevated (our folder, our
/// executable, its runtime folder, sing-box) that an ordinary program could
/// change. Installed builds on Windows only; `None` when all are admin-only.
pub fn unprotected(runtime: &Path, core: Option<&Path>) -> Option<PathBuf> {
    if cfg!(debug_assertions) || !cfg!(windows) {
        return None;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    [dir.as_path(), exe.as_path(), runtime]
        .into_iter()
        .chain(core)
        .find(|path| !proxypilot_core::acl::admin_only(path))
        .map(Path::to_path_buf)
}

/// The bundled sing-box: next to our executable when installed (Tauri copies
/// the sidecar there in dev too), or the fetched sidecar in a plain cargo run.
pub fn find_core() -> Option<PathBuf> {
    if cfg!(debug_assertions)
        && let Some(path) = std::env::var_os("PROXYPILOT_SING_BOX") {
            return Some(PathBuf::from(path));
        }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let installed = dir.join(if cfg!(windows) { "sing-box.exe" } else { "sing-box" });
    if installed.exists() {
        return Some(installed);
    }
    if cfg!(debug_assertions) {
        let fetched = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/sing-box-x86_64-pc-windows-msvc.exe");
        if fetched.exists() {
            return Some(fetched);
        }
    }
    None
}

/// Append one line to app.log. Events and errors only: never node details,
/// credentials or visited domains.
pub fn append_log(path: &Path, message: &str) {
    if fs::metadata(path).map(|meta| meta.len() > APP_LOG_LIMIT).unwrap_or(false) {
        let _ = fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {message}", utc_timestamp());
    }
}

/// For the self-check report, which a Chinese tester reads; UTC+8 without a
/// time-zone database is exact for China, and the label says what it is.
pub fn beijing_time() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) + 8 * 3600;
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}（北京时间）", rest / 3600, rest % 3600 / 60)
}

fn utc_timestamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// Days since 1970-01-01 to (year, month, day); Howard Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::civil_from_days;

    #[test]
    fn converts_days_to_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }
}

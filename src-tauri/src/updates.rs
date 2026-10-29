//! Checking GitHub for a new release and installing it.
//!
//! Requests go through the user's proxy client when it is up, because GitHub
//! is slow or unreachable from many networks here; otherwise straight out
//! (honouring a system proxy). The package is checked against the public key
//! built into the app before anything runs, so whoever sits on the network
//! path cannot hand us a different installer.

use crate::state::{Msg, Shared, lock};
use proxypilot_core::probe::upstream_alive;
use proxypilot_core::{Upstream, UpstreamKind};
use serde::Serialize;
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};
use tauri::{Emitter, Url};
use tauri_plugin_updater::{Error, Update, UpdaterExt};

pub const RELEASES_URL: &str = "https://github.com/dongtl/ProxyPilot/releases";
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
/// The installer is about 20 MB and a proxied path can be slow.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const ALIVE_TIMEOUT: Duration = Duration::from_millis(800);
/// Late enough that routing and the proxy client are up after a logon.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(20);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the background thread looks at the clock; a failed check is
/// retried on the next wake instead of a day later.
const WAKE_EVERY: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    /// Release notes as written on the release page; may be empty.
    pub notes: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

pub fn info(update: &Update) -> UpdateInfo {
    UpdateInfo { version: update.version.clone(), notes: update.body.clone().unwrap_or_default().trim().to_owned() }
}

/// The proxy client as a URL reqwest understands, if it is answering now.
/// `socks5h` so GitHub's name is resolved by the proxy, not locally.
/// The second value: a proxy is chosen but its port did not answer.
fn proxy_url(upstream: &Upstream) -> (Option<Url>, bool) {
    if !upstream_alive(upstream, ALIVE_TIMEOUT) {
        return (None, true);
    }
    let scheme = if upstream.kind == UpstreamKind::Socks5 { "socks5h" } else { "http" };
    let host = if upstream.host.contains(':') { format!("[{}]", upstream.host) } else { upstream.host.clone() };
    (format!("{scheme}://{host}:{}", upstream.port).parse().ok(), false)
}

/// Which way a request went, for app.log. The proxy is a local port, never a node.
fn route(proxy: Option<&Url>, silent: bool) -> String {
    match proxy {
        Some(proxy) => format!("走代理 {proxy}"),
        None if silent => "直连（代理端口没响应）".into(),
        None => "直连".into(),
    }
}

/// The error and what caused it, for app.log: reqwest's own message is only
/// "error sending request", the reason (timeout, refused, TLS) sits below it.
fn chain(error: &dyn std::error::Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        // `#[error(transparent)]` repeats the inner message; keep it once.
        if parts.last().is_none_or(|last| !last.contains(&text)) {
            parts.push(text);
        }
        source = cause.source();
    }
    parts.join(" ← ")
}

/// Worth retrying without the proxy: the request itself did not get through.
fn is_network(error: &Error) -> bool {
    matches!(error, Error::Reqwest(_) | Error::Network(_) | Error::Io(_))
}

fn describe(error: &Error) -> String {
    match error {
        Error::Reqwest(_) | Error::Network(_) | Error::Io(_) => {
            "连不上 GitHub。请确认代理客户端正在运行，稍后再试；也可以到发布页手动下载".into()
        }
        Error::ReleaseNotFound => "暂时没有获取到版本信息，请稍后再试".into(),
        Error::TargetNotFound(_) | Error::TargetsNotFound(_) => "新版本还没有适合这台电脑的安装包".into(),
        Error::Minisign(_) | Error::SignedVersionMismatch { .. } | Error::MissingSignedVersion => {
            "安装包没有通过签名校验，已停止更新。请到发布页手动下载".into()
        }
        other => format!("更新没有成功：{other}"),
    }
}

/// macOS: the running copy cannot be replaced where it is.
const OPENED_IN_PLACE: &str = "ProxyPilot 现在是从磁盘映像或下载文件夹里直接打开的，没法就地更新。\
    请先点窗口顶部的“移到应用程序文件夹”，重新打开后再更新";

/// After the download and signature check passed, what failed is replacing
/// the program files, which has nothing to do with the network.
fn describe_install(error: &Error) -> String {
    match error {
        Error::Minisign(_) | Error::SignedVersionMismatch { .. } | Error::MissingSignedVersion => describe(error),
        Error::PreviousAppNotRestored(kept) => format!(
            "更新没有完成，原来的 ProxyPilot 被留在了 {}。请把它拖回“应用程序”文件夹，或到发布页重新下载安装",
            kept.display()
        ),
        _ if cfg!(target_os = "macos") => "新版本已下载并通过校验，但替换程序文件没有成功：可能是没有输入密码，\
            或系统不允许修改“应用程序”里的 ProxyPilot。可以到发布页下载 dmg，退出 ProxyPilot 后拖进“应用程序”替换"
            .into(),
        other => format!("新版本已下载并通过校验，但安装没有成功：{other}。可以到发布页下载安装包手动安装"),
    }
}

/// Where a bundle lies, for the log, and whether an update can replace it there.
/// Gatekeeper runs an app opened before it was moved from a random read-only
/// folder (App Translocation); a disk image is read-only too.
#[cfg(any(target_os = "macos", test))]
fn classify_bundle(bundle: &str, read_only: bool) -> (&'static str, bool) {
    let translocated = bundle.contains("/AppTranslocation/");
    let place = if translocated {
        "系统隔离的临时位置（App Translocation）"
    } else if bundle.starts_with("/Volumes/") {
        "磁盘映像或外接磁盘"
    } else if bundle.starts_with("/Applications/") {
        "“应用程序”文件夹"
    } else {
        "其它文件夹"
    };
    (place, !(translocated || read_only))
}

/// macOS: the running bundle's place and whether it can be replaced; `None`
/// outside a bundle (a dev build).
#[cfg(target_os = "macos")]
fn bundle_place() -> Option<(&'static str, bool)> {
    let bundle = crate::relocate::running_bundle()?;
    let read_only = bundle.parent().is_some_and(read_only_fs);
    Some(classify_bundle(&bundle.to_string_lossy(), read_only))
}

/// A read-only file system only: "permission denied" is what the updater's
/// password prompt is for.
#[cfg(target_os = "macos")]
fn read_only_fs(dir: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else { return false };
    let refused = unsafe { libc::access(path.as_ptr(), libc::W_OK) } != 0;
    // Read errno right away, before anything else can change it.
    refused && std::io::Error::last_os_error().raw_os_error() == Some(libc::EROFS)
}

#[cfg(not(target_os = "macos"))]
fn bundle_place() -> Option<(&'static str, bool)> {
    None
}

/// Stop routing and the tray before the installer replaces our files. The
/// same steps as quitting from the tray, minus the exit, which the updater does.
fn before_exit(shared: &Shared) {
    let (ack, done) = std::sync::mpsc::channel();
    if shared.tx.send(Msg::Quit(ack)).is_ok() {
        let _ = done.recv_timeout(Duration::from_secs(5));
    }
    shared.log("退出并安装更新");
    shared.app.cleanup_before_exit();
}

fn updater(shared: &Arc<Shared>, proxy: Option<&Url>) -> Result<tauri_plugin_updater::Updater, Error> {
    // Weak: the hook ends up inside the pending update, which Shared owns.
    let hook: Weak<Shared> = Arc::downgrade(shared);
    let mut builder = shared
        .app
        .updater_builder()
        .timeout(CHECK_TIMEOUT)
        // The installer's own restart runs us as a standard user, and we need
        // administrator rights; the install hook reopens us elevated instead.
        .restart_after_install(false)
        .on_before_exit(move || {
            if let Some(shared) = hook.upgrade() {
                before_exit(&shared);
            }
        });
    if let Some(proxy) = proxy {
        builder = builder.proxy(proxy.clone());
    }
    builder.build()
}

/// Ask GitHub for a newer release. Remembers what it finds so the window can
/// offer it and `install` can use it.
pub async fn check(shared: &Arc<Shared>) -> Result<Option<UpdateInfo>, String> {
    let upstream = lock(&shared.settings).upstream.clone();
    let (proxy, silent) = tauri::async_runtime::spawn_blocking(move || upstream.as_ref().map(proxy_url))
        .await
        .ok()
        .flatten()
        .unwrap_or((None, false));
    let mut way = route(proxy.as_ref(), silent);
    let mut result = match updater(shared, proxy.as_ref()) {
        Ok(updater) => updater.check().await,
        Err(error) => Err(error),
    };
    if proxy.is_some()
        && let Err(error) = &result
        && is_network(error)
    {
        shared.log(&format!("检查更新失败（{way}），改为直连再试：{}", chain(error)));
        way = route(None, false);
        result = match updater(shared, None) {
            Ok(updater) => updater.check().await,
            Err(error) => Err(error),
        };
    }
    match result {
        Ok(found) => {
            let info = found.as_ref().map(info);
            match &info {
                Some(info) => shared.log(&format!("发现新版本 {}（{way}）", info.version)),
                None => shared.log(&format!("检查更新：已是最新版本（{way}）")),
            }
            *lock(&shared.update) = found;
            shared.emit();
            Ok(info)
        }
        Err(error) => {
            shared.log(&format!("检查更新失败（{way}）：{}", chain(&error)));
            Err(describe(&error))
        }
    }
}

/// Download, verify, and hand over to the installer. On Windows a successful
/// install never returns: the app exits and the installer reopens it.
pub async fn install(shared: &Arc<Shared>) -> Result<(), String> {
    let mut update = lock(&shared.update).clone().ok_or("请先检查更新")?;
    // Refuse before a 20 MB download that could never be put in place.
    let place = bundle_place();
    if let Some((spot, false)) = place {
        shared.log(&format!("没有更新：程序在{spot}，不能就地替换"));
        return Err(OPENED_IN_PLACE.into());
    }
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    shared.log(&format!("开始下载新版本 {}", update.version));
    let app = shared.app.clone();
    let mut downloaded: u64 = 0;
    let mut reported: u64 = 0;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                // Every 1% (or 256 KB when the size is unknown) is plenty for a progress bar.
                let step = total.map_or(256 * 1024, |total| (total / 100).max(1));
                if downloaded - reported >= step {
                    reported = downloaded;
                    let _ = app.emit("update-progress", Progress { downloaded, total });
                }
            },
            || {},
        )
        .await
        .map_err(|error| {
            shared.log(&format!("下载更新失败：{}", chain(&error)));
            describe(&error)
        })?;
    let _ = app.emit("update-progress", Progress { downloaded, total: Some(downloaded) });
    match update.install(bytes) {
        // Windows exits inside `install`; elsewhere the files are already replaced.
        Ok(()) => {
            before_exit(shared);
            shared.app.restart();
        }
        Err(error) => {
            let place = place.map(|(place, _)| format!("（程序在{place}）")).unwrap_or_default();
            shared.log(&format!("安装更新失败{place}：{}", chain(&error)));
            // If the exit hook already stopped the supervisor, this app can no
            // longer route; start over rather than linger half shut down.
            if shared.tx.send(Msg::Check).is_err() {
                shared.app.restart();
            }
            Err(describe_install(&error))
        }
    }
}

/// Check shortly after start and then about once a day, while the setting is on.
pub fn spawn(shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK_AFTER);
            let mut last_ok: Option<SystemTime> = None;
            loop {
                // Wall-clock time, so a laptop asleep for two days checks on waking.
                // A clock set back reads as "due" rather than never.
                let due = last_ok.is_none_or(|at| at.elapsed().ok().is_none_or(|elapsed| elapsed >= CHECK_EVERY));
                let enabled = lock(&shared.settings).check_updates;
                if enabled && due && tauri::async_runtime::block_on(check(&shared)).is_ok() {
                    last_ok = Some(SystemTime::now());
                }
                std::thread::sleep(WAKE_EVERY);
            }
        })
        .expect("cannot start update thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_failures_are_not_reported_as_network_trouble() {
        let denied = Error::Io(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Failed to move the new app into place"));
        assert!(describe(&denied).contains("GitHub"));
        assert!(!describe_install(&denied).contains("GitHub"));
        let kept = Error::PreviousAppNotRestored("/Users/someone/ProxyPilot.app".into());
        assert!(describe_install(&kept).contains("/Users/someone/ProxyPilot.app"));
    }

    #[test]
    fn a_bundle_opened_from_the_disk_image_or_translocated_cannot_be_replaced() {
        assert_eq!(classify_bundle("/Applications/ProxyPilot.app", false), ("“应用程序”文件夹", true));
        let translocated = "/private/var/folders/x/T/AppTranslocation/1234/d/ProxyPilot.app";
        assert!(!classify_bundle(translocated, true).1);
        assert!(!classify_bundle("/Volumes/ProxyPilot/ProxyPilot.app", true).1);
        // A writable external disk is fine.
        assert!(classify_bundle("/Volumes/Data/Apps/ProxyPilot.app", false).1);
    }
}

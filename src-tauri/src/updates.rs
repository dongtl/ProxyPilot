//! Checking GitHub for a new release and installing it.
//!
//! Nothing is downloaded or installed unless the user asks: the scheduled
//! check only finds out, and the window offers the release. A check or a
//! download the user started can be cancelled, and gives up within seconds
//! when the network does not answer instead of spinning for minutes.
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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Url};
use tauri_plugin_updater::{Error, Update, UpdaterExt};

pub const RELEASES_URL: &str = "https://github.com/dongtl/ProxyPilot/releases";
/// Reaching GitHub, through the proxy client or not.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// The whole check; the release manifest is a few hundred bytes.
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// A download that has received nothing new for this long is stuck. There is
/// no limit on the whole download, so a slow line still gets there.
const STALL_TIMEOUT: Duration = Duration::from_secs(30);
// reqwest's read limit also runs while connecting. Were it the shorter one,
// a GitHub that cannot be reached would be reported as a stuck download.
const _: () = assert!(CONNECT_TIMEOUT.as_millis() < STALL_TIMEOUT.as_millis());
const ALIVE_TIMEOUT: Duration = Duration::from_millis(800);
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(20);
/// After a logon the proxy client often opens its port after we start.
const PROXY_WAIT_LIMIT: Duration = Duration::from_secs(3 * 60);
const PROXY_POLL: Duration = Duration::from_secs(5);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// A failed check is retried after 5 minutes, then 10, 20, 40, and hourly.
const RETRY_FIRST: Duration = Duration::from_secs(5 * 60);
const RETRY_MAX: Duration = Duration::from_secs(60 * 60);
/// How often the background thread looks at the clock.
const WAKE_EVERY: Duration = Duration::from_secs(60);
const BUSY: &str = "正在检查或下载更新，请等它结束，或者先点“取消”";

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
    /// Downloaded and verified; the files are being replaced and it is too
    /// late to cancel.
    installing: bool,
}

/// The last check, for the settings page.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastCheck {
    /// Milliseconds since 1970.
    pub at: u64,
    /// What went wrong, as shown to the user; None when it worked.
    pub error: Option<String>,
}

/// A check or download the user started, which they can cancel.
struct Job {
    id: u64,
    abort: Option<Box<dyn Fn() + Send>>,
    cancelled: bool,
    /// Past the point of no return: replacing the program files.
    installing: bool,
}

#[derive(Default)]
pub struct UpdateState {
    /// The release the last check found, kept so installing does not ask GitHub again.
    pub found: Option<Update>,
    pub last_check: Option<LastCheck>,
    /// Checks failed in a row, for the retry delay.
    failures: u32,
    job: Option<Job>,
    next_id: u64,
}

impl UpdateState {
    /// Claim the one slot for a cancellable check or download.
    fn start(&mut self) -> Result<u64, &'static str> {
        if self.job.is_some() {
            return Err(BUSY);
        }
        self.next_id += 1;
        self.job = Some(Job { id: self.next_id, abort: None, cancelled: false, installing: false });
        Ok(self.next_id)
    }

    /// Hand over the task's abort; a cancel that came in before it runs it now.
    fn set_abort(&mut self, id: u64, abort: Box<dyn Fn() + Send>) {
        if let Some(job) = self.job.as_mut().filter(|job| job.id == id) {
            if job.cancelled {
                abort();
            }
            job.abort = Some(abort);
        }
    }

    fn cancel(&mut self) -> Result<(), String> {
        let Some(job) = self.job.as_mut() else { return Ok(()) };
        if job.installing {
            return Err("正在安装新版本，已经不能取消了，请稍候".into());
        }
        job.cancelled = true;
        if let Some(abort) = &job.abort {
            abort();
        }
        Ok(())
    }

    /// Whether the files may be replaced: false once the user cancelled. From
    /// here on cancelling is refused.
    fn begin_install(&mut self) -> bool {
        let Some(job) = self.job.as_mut() else { return true };
        job.installing = !job.cancelled;
        job.installing
    }

    /// Free the slot; whether the job was cancelled.
    fn finish(&mut self, id: u64) -> bool {
        self.job.take_if(|job| job.id == id).is_some_and(|job| job.cancelled)
    }

    /// Note how a check went. A failure keeps the release found earlier.
    fn record(&mut self, at: u64, outcome: Result<Option<Update>, String>) {
        match outcome {
            Ok(found) => {
                self.failures = 0;
                self.found = found;
                self.last_check = Some(LastCheck { at, error: None });
            }
            Err(error) => {
                self.failures += 1;
                self.last_check = Some(LastCheck { at, error: Some(error) });
            }
        }
    }

    /// Whether the scheduled check should run now. Wall-clock time, so a
    /// laptop asleep for two days checks on waking; a clock set back reads as due.
    fn due(&self, now: u64) -> bool {
        let Some(last) = &self.last_check else { return true };
        let wait = if self.failures == 0 { CHECK_EVERY } else { retry_after(self.failures) };
        now.checked_sub(last.at).is_none_or(|elapsed| elapsed >= wait.as_millis() as u64)
    }
}

/// 5, 10, 20 and 40 minutes, then hourly.
fn retry_after(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(8);
    RETRY_FIRST.saturating_mul(1 << doublings).min(RETRY_MAX)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
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

/// Reached GitHub, then nothing new came for `STALL_TIMEOUT`.
fn stalled(error: &Error) -> bool {
    matches!(error, Error::Reqwest(error) if error.is_timeout() && !error.is_connect())
}

fn describe_download(error: &Error) -> String {
    if stalled(error) {
        return format!(
            "下载卡住了：{} 秒没有收到新的数据。可以点“重试”，或者到发布页手动下载",
            STALL_TIMEOUT.as_secs()
        );
    }
    describe(error)
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
        })
        // Also used by the download: give up quickly on a GitHub that does
        // not answer, and on a download that stopped moving.
        .configure_client(|client| client.connect_timeout(CONNECT_TIMEOUT).read_timeout(STALL_TIMEOUT));
    if let Some(proxy) = proxy {
        builder = builder.proxy(proxy.clone());
    }
    builder.build()
}

/// Which way to reach GitHub right now: the proxy client if it answers.
async fn current_route(shared: &Shared) -> (Option<Url>, bool) {
    let upstream = lock(&shared.settings).upstream.clone();
    tauri::async_runtime::spawn_blocking(move || upstream.as_ref().map(proxy_url))
        .await
        .ok()
        .flatten()
        .unwrap_or((None, false))
}

/// Ask GitHub for a newer release. Remembers what it finds, and how the check
/// went, so the window can offer the release and `install` can use it.
pub async fn check(shared: &Arc<Shared>) -> Result<Option<UpdateInfo>, String> {
    let (proxy, silent) = current_route(shared).await;
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
    let outcome = match result {
        Ok(found) => {
            match &found {
                Some(update) => shared.log(&format!("发现新版本 {}（{way}）", update.version)),
                None => shared.log(&format!("检查更新：已是最新版本（{way}）")),
            }
            Ok(found)
        }
        Err(error) => {
            shared.log(&format!("检查更新失败（{way}）：{}", chain(&error)));
            Err(describe(&error))
        }
    };
    let info = outcome.as_ref().map(|found| found.as_ref().map(info)).map_err(Clone::clone);
    lock(&shared.update).record(now_ms(), outcome);
    shared.emit();
    info
}

enum Finished<T> {
    Done(T),
    Cancelled,
}

/// Run a check or download the user started, so `cancel` can stop it.
async fn run_job<T: Send + 'static>(
    shared: &Arc<Shared>,
    what: &str,
    work: impl Future<Output = Result<T, String>> + Send + 'static,
) -> Result<Finished<T>, String> {
    let id = lock(&shared.update).start()?;
    let task = tauri::async_runtime::spawn(work);
    let handle = task.inner().abort_handle();
    lock(&shared.update).set_abort(id, Box::new(move || handle.abort()));
    let result = task.await;
    if lock(&shared.update).finish(id) {
        shared.log(&format!("已取消{what}"));
        return Ok(Finished::Cancelled);
    }
    match result {
        Ok(result) => result.map(Finished::Done),
        Err(error) => Err(format!("{what}没有完成：{error}")),
    }
}

/// Stop the check or download the user started; refused once installing.
pub fn cancel(shared: &Shared) -> Result<(), String> {
    lock(&shared.update).cancel()
}

/// How a check the user asked for ended.
#[derive(Serialize)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum CheckOutcome {
    Latest,
    Found { update: UpdateInfo },
    Cancelled,
}

/// The settings page's "检查更新": the scheduled check, but cancellable.
pub async fn check_now(shared: &Arc<Shared>) -> Result<CheckOutcome, String> {
    let worker = shared.clone();
    Ok(match run_job(shared, "检查更新", async move { check(&worker).await }).await? {
        Finished::Done(Some(update)) => CheckOutcome::Found { update },
        Finished::Done(None) => CheckOutcome::Latest,
        Finished::Cancelled => CheckOutcome::Cancelled,
    })
}

/// The window's "立即更新". Resolves only when cancelled: a successful install
/// ends with the app exiting (Windows) or restarting (macOS).
pub async fn install_now(shared: &Arc<Shared>) -> Result<(), String> {
    let worker = shared.clone();
    run_job(shared, "下载更新", async move { install(&worker).await }).await.map(|_| ())
}

/// Download, verify, and hand over to the installer. On Windows a successful
/// install never returns: the app exits and the installer reopens it.
async fn install(shared: &Arc<Shared>) -> Result<(), String> {
    let mut update = lock(&shared.update).found.clone().ok_or("请先检查更新")?;
    // Refuse before a 20 MB download that could never be put in place.
    let place = bundle_place();
    if let Some((spot, false)) = place {
        shared.log(&format!("没有更新：程序在{spot}，不能就地替换"));
        return Err(OPENED_IN_PLACE.into());
    }
    // The check may be days old; the proxy client it went through may be gone.
    let (proxy, silent) = current_route(shared).await;
    let mut way = route(proxy.as_ref(), silent);
    update.proxy = proxy;
    // The check's limit is for a small file; a download is only stopped when it stalls.
    update.timeout = None;
    shared.log(&format!("开始下载新版本 {}（{way}）", update.version));
    let mut result = download(shared, &update).await;
    // A download that stalled did reach GitHub; going direct would not help.
    if update.proxy.is_some()
        && let Err(error) = &result
        && is_network(error)
        && !stalled(error)
    {
        shared.log(&format!("下载更新失败（{way}），改为直连再试：{}", chain(error)));
        way = route(None, false);
        update.proxy = None;
        result = download(shared, &update).await;
    }
    let bytes = result.map_err(|error| {
        shared.log(&format!("下载更新失败（{way}）：{}", chain(&error)));
        describe_download(&error)
    })?;
    if !lock(&shared.update).begin_install() {
        return Ok(());
    }
    let _ = shared.app.emit("update-progress", Progress {
        downloaded: bytes.len() as u64,
        total: Some(bytes.len() as u64),
        installing: true,
    });
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

/// One download attempt, reporting progress to the window.
async fn download(shared: &Shared, update: &Update) -> Result<Vec<u8>, Error> {
    let mut downloaded: u64 = 0;
    let mut reported: u64 = 0;
    update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                // Every 1% (or 256 KB when the size is unknown) is plenty for a progress bar.
                let step = total.map_or(256 * 1024, |total| (total / 100).max(1));
                if downloaded - reported >= step {
                    reported = downloaded;
                    let _ = shared.app.emit("update-progress", Progress { downloaded, total, installing: false });
                }
            },
            || {},
        )
        .await
}

/// Check shortly after start and then about once a day, while the setting is
/// on; a failed check is tried again within minutes. Only checks: the window
/// offers what it finds and the user decides.
pub fn spawn(shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK_AFTER);
            wait_for_proxy(&shared);
            loop {
                let enabled = lock(&shared.settings).check_updates;
                let due = lock(&shared.update).due(now_ms());
                if enabled && due && tauri::async_runtime::block_on(check(&shared)).is_err() {
                    let failures = lock(&shared.update).failures;
                    shared.log(&format!("{} 分钟后自动再检查一次", retry_after(failures).as_secs() / 60));
                }
                std::thread::sleep(WAKE_EVERY);
            }
        })
        .expect("cannot start update thread");
}

/// After a logon the proxy client may open its port after we start; checking
/// before that fails for nothing. Gives up after a few minutes, since the
/// client may simply be closed.
fn wait_for_proxy(shared: &Shared) {
    let started = Instant::now();
    loop {
        let Some(upstream) = lock(&shared.settings).upstream.clone() else { return };
        if upstream_alive(&upstream, ALIVE_TIMEOUT) {
            return;
        }
        if started.elapsed() >= PROXY_WAIT_LIMIT {
            shared.log("代理端口一直没响应，先直连检查更新");
            return;
        }
        std::thread::sleep(PROXY_POLL);
    }
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

    #[test]
    fn a_failed_check_is_retried_within_minutes_then_less_often() {
        let minutes = |failures| retry_after(failures).as_secs() / 60;
        assert_eq!([1, 2, 3, 4, 5, 50].map(minutes), [5, 10, 20, 40, 60, 60]);

        let mut state = UpdateState::default();
        assert!(state.due(0));
        let at = 1_000_000_000;
        state.record(at, Err("连不上 GitHub".into()));
        assert_eq!(state.last_check, Some(LastCheck { at, error: Some("连不上 GitHub".into()) }));
        assert!(!state.due(at + 4 * 60 * 1000));
        assert!(state.due(at + 5 * 60 * 1000));
        state.record(at, Err("连不上 GitHub".into()));
        assert!(!state.due(at + 5 * 60 * 1000));
        state.record(at, Ok(None));
        assert_eq!(state.last_check, Some(LastCheck { at, error: None }));
        assert!(!state.due(at + 23 * 60 * 60 * 1000));
        assert!(state.due(at + 24 * 60 * 60 * 1000));
        // A clock set back does not stop the checks.
        assert!(state.due(at - 1));
    }

    #[test]
    fn one_job_at_a_time_and_no_cancelling_once_installing() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let aborted = Arc::new(AtomicU32::new(0));
        let counter = |aborted: &Arc<AtomicU32>| {
            let aborted = aborted.clone();
            Box::new(move || {
                aborted.fetch_add(1, Ordering::SeqCst);
            })
        };
        let mut state = UpdateState::default();
        assert!(state.cancel().is_ok(), "nothing to cancel is fine");

        let first = state.start().unwrap();
        assert_eq!(state.start(), Err(BUSY));
        // A cancel before the task is running aborts it as soon as it is.
        state.cancel().unwrap();
        state.set_abort(first, counter(&aborted));
        assert_eq!(aborted.load(Ordering::SeqCst), 1);
        assert!(!state.begin_install(), "a cancelled download is not installed");
        assert!(state.finish(first));

        let second = state.start().unwrap();
        state.set_abort(second, counter(&aborted));
        assert!(state.begin_install());
        assert!(state.cancel().is_err());
        assert_eq!(aborted.load(Ordering::SeqCst), 1);
        assert!(!state.finish(second));
        assert!(state.start().is_ok(), "the slot is free again");
    }

    /// The updater's client gets these limits; a download that stops moving
    /// must end with "stuck", one that cannot connect with "no GitHub".
    #[test]
    fn a_stalled_download_and_an_unreachable_server_are_told_apart() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let stalls = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.read(&mut [0; 1024]);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\nabc");
            std::thread::sleep(Duration::from_secs(5));
        });
        let refused = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();

        let _ = rustls::crypto::ring::default_provider().install_default();
        let fetch = |addr: std::net::SocketAddr| {
            tauri::async_runtime::block_on(async move {
                // Shorter connect limit, as in the app. Windows takes about
                // two seconds to report a refused connection.
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .connect_timeout(Duration::from_millis(300))
                    .read_timeout(Duration::from_millis(600))
                    .build()
                    .unwrap();
                let response = client.get(format!("http://{addr}/")).send().await?;
                response.bytes().await.map(|_| ())
            })
            .map_err(Error::Reqwest)
            .unwrap_err()
        };
        let stuck = fetch(stalls);
        assert!(stalled(&stuck), "{stuck:?}");
        assert!(describe_download(&stuck).contains("卡住"));
        let gone = fetch(refused);
        assert!(!stalled(&gone), "{gone:?}");
        assert!(describe_download(&gone).contains("GitHub"));
    }
}

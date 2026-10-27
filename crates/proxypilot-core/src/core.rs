//! Running sing-box: replace its configuration as a transaction and keep the
//! last working one to fall back to.
//!
//! The order matters. A candidate is validated before anything is touched;
//! the running config is backed up before the core stops; the new file
//! replaces the old one atomically; and if the new core does not bring the
//! adapter up in time, the previous config is restored and started again. The
//! user ends up either on the new rules or on the old ones, never on none.

use crate::config::RuntimePaths;
use crate::sys;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const LOG_ROTATE_BYTES: u64 = 20 * 1024 * 1024;
/// How long a core we stopped may keep its adapter around while Windows tears it down.
const ADAPTER_RELEASE: Duration = Duration::from_secs(5);

/// Shown when an adapter with our name is up and it is not our core's.
pub const ADAPTER_TAKEN: &str = "ProxyPilot 虚拟网卡已被其它程序占用，常见原因是旧版 ProxyPilot 还在运行，请先退出它";

pub trait CoreProcess: Send {
    fn pid(&self) -> u32;
    /// `Some(code)` once the process has exited.
    fn exited(&mut self) -> Option<i32>;
    fn kill(&mut self);
}

pub trait Runner: Send {
    /// `sing-box check`; `Err` carries what sing-box printed.
    fn check(&self, config: &Path) -> Result<(), String>;
    fn start(&self, config: &Path, workdir: &Path, log: &Path) -> io::Result<Box<dyn CoreProcess>>;
}

pub struct SingBox {
    executable: PathBuf,
    job: Option<sys::Job>,
}

impl SingBox {
    /// The Job Object is best effort: without it the core still runs, it just
    /// is not guaranteed to die with us.
    pub fn new(executable: PathBuf) -> SingBox {
        SingBox { executable, job: sys::Job::new().ok() }
    }
}

struct ChildProcess(Child);

impl CoreProcess for ChildProcess {
    fn pid(&self) -> u32 {
        self.0.id()
    }

    fn exited(&mut self) -> Option<i32> {
        match self.0.try_wait() {
            Ok(Some(status)) => Some(status.code().unwrap_or(-1)),
            Ok(None) => None,
            Err(_) => Some(-1),
        }
    }

    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Runner for SingBox {
    fn check(&self, config: &Path) -> Result<(), String> {
        let mut command = Command::new(&self.executable);
        command.arg("check").arg("-c").arg(config).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        sys::hide_window(&mut command);
        let mut child = command.spawn().map_err(|error| format!("无法运行 sing-box：{error}"))?;
        // Drain both pipes on their own threads so a chatty check cannot fill
        // a pipe and stall while we wait for it to exit.
        let mut stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let out = thread::spawn(move || {
            let mut text = String::new();
            let _ = stdout.read_to_string(&mut text);
            text
        });
        let err = thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        let deadline = Instant::now() + CHECK_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let text = format!("{}{}", err.join().unwrap_or_default(), out.join().unwrap_or_default());
        match status {
            Some(status) if status.success() => Ok(()),
            Some(_) => Err(clean_output(&text)),
            None => Err("sing-box 配置检查超时".into()),
        }
    }

    fn start(&self, config: &Path, workdir: &Path, log: &Path) -> io::Result<Box<dyn CoreProcess>> {
        fs::create_dir_all(workdir)?;
        let output = fs::OpenOptions::new().create(true).append(true).open(log)?;
        let mut command = Command::new(&self.executable);
        command
            .arg("run")
            .arg("-c")
            .arg(config)
            .arg("-D")
            .arg(workdir)
            .stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(output);
        sys::hide_window(&mut command);
        let child = command.spawn()?;
        if let Some(job) = &self.job {
            let _ = job.assign(&child);
        }
        Ok(Box::new(ChildProcess(child)))
    }
}

/// sing-box colours its output and prefixes timestamps; keep the message.
fn clean_output(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Skip "ESC [ ... letter".
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        plain.push(c);
    }
    let lines: Vec<&str> = plain.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let tail = &lines[lines.len().saturating_sub(5)..];
    if tail.is_empty() { "sing-box 未给出原因".into() } else { tail.join("\n") }
}

/// Last few lines a log gained after byte `from`, for "why did it exit".
/// The log is appended to across runs; reading from where this run began
/// keeps an earlier run's errors out of this run's explanation.
fn log_tail(path: &Path, from: u64) -> String {
    let Ok(bytes) = fs::read(path) else { return String::new() };
    let from = usize::try_from(from).unwrap_or(usize::MAX).min(bytes.len());
    let start = from.max(bytes.len().saturating_sub(4096));
    clean_output(&String::from_utf8_lossy(&bytes[start..]))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOutcome {
    pub ok: bool,
    pub message: String,
    pub rolled_back: bool,
    /// True when a previous config is running again after a failure.
    pub kept_previous: bool,
    /// Nothing was started because another program holds an adapter with
    /// our name. The new config is installed; starting later can succeed.
    pub adapter_taken: bool,
}

impl ApplyOutcome {
    fn ok(message: &str) -> Self {
        ApplyOutcome { ok: true, message: message.into(), rolled_back: false, kept_previous: false, adapter_taken: false }
    }

    fn failed(message: String, kept_previous: bool) -> Self {
        ApplyOutcome { ok: false, message, rolled_back: false, kept_previous, adapter_taken: false }
    }
}

pub struct CoreManager {
    runner: Box<dyn Runner>,
    paths: RuntimePaths,
    ready: Box<dyn Fn() -> bool + Send>,
    startup_grace: Duration,
    process: Option<Box<dyn CoreProcess>>,
    last_start_error: String,
    /// Where the stderr log ended when the current run began.
    log_offset: u64,
    /// Until then, an adapter with our name may still be our last core's.
    released_at: Option<Instant>,
    adapter_taken: bool,
}

impl CoreManager {
    /// `ready` says whether the core is serving; in production, whether our
    /// adapter is up.
    pub fn new(runner: Box<dyn Runner>, paths: RuntimePaths, ready: Box<dyn Fn() -> bool + Send>) -> CoreManager {
        CoreManager {
            runner,
            paths,
            ready,
            startup_grace: Duration::from_secs(5),
            process: None,
            last_start_error: String::new(),
            log_offset: 0,
            released_at: None,
            adapter_taken: false,
        }
    }

    pub fn with_startup_grace(mut self, grace: Duration) -> Self {
        self.startup_grace = grace;
        self
    }

    pub fn paths(&self) -> &RuntimePaths {
        &self.paths
    }

    pub fn running(&mut self) -> bool {
        match &mut self.process {
            Some(process) => process.exited().is_none(),
            None => false,
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.process.as_ref().map(|process| process.pid())
    }

    /// Whether an adapter with our name could be our core's: one is running
    /// or not yet reaped, or one stopped recently enough that Windows may
    /// still be removing its adapter.
    pub fn may_own_adapter(&self) -> bool {
        self.process.is_some() || self.released_at.is_some_and(|at| Instant::now() < at)
    }

    fn mark_released(&mut self) {
        self.released_at = Some(Instant::now() + ADAPTER_RELEASE);
    }

    /// An adapter left by the core we just stopped gets time to go away.
    /// One that is up with no core of ours behind it belongs to someone else.
    fn adapter_free(&self) -> bool {
        loop {
            if !(self.ready)() {
                return true;
            }
            if !self.released_at.is_some_and(|at| Instant::now() < at) {
                return false;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    /// If the core we started has exited on its own, forget it and say why.
    pub fn poll_exit(&mut self) -> Option<String> {
        let code = self.process.as_mut()?.exited()?;
        self.process = None;
        self.mark_released();
        let tail = log_tail(&self.paths.stderr_log, self.log_offset);
        Some(if tail.is_empty() { format!("sing-box 意外退出（代码 {code}）") } else { format!("sing-box 意外退出（代码 {code}）：{tail}") })
    }

    pub fn validate(&self, candidate: &Path) -> Result<(), String> {
        self.runner.check(candidate)
    }

    fn rotate_log(&self) {
        for path in [&self.paths.log, &self.paths.stderr_log] {
            if fs::metadata(path).map(|meta| meta.len() > LOG_ROTATE_BYTES).unwrap_or(false) {
                let rotated = path.with_extension("log.1");
                let _ = fs::remove_file(&rotated);
                let _ = fs::rename(path, rotated);
            }
        }
    }

    fn start_current(&mut self) -> bool {
        self.last_start_error.clear();
        self.adapter_taken = false;
        // `ready` cannot tell whose adapter it sees. Starting while one with
        // our name is up would report success at once, and then the core
        // would die failing to create its own.
        if !self.adapter_free() {
            self.adapter_taken = true;
            self.last_start_error = ADAPTER_TAKEN.into();
            return false;
        }
        // The writer is stopped here, so rotating cannot race with it.
        self.rotate_log();
        self.log_offset = fs::metadata(&self.paths.stderr_log).map(|meta| meta.len()).unwrap_or(0);
        let mut process = match self.runner.start(&self.paths.current, &self.paths.dir, &self.paths.stderr_log) {
            Ok(process) => process,
            Err(error) => {
                self.last_start_error = format!("无法启动 sing-box：{error}");
                return false;
            }
        };
        let deadline = Instant::now() + self.startup_grace;
        loop {
            if let Some(code) = process.exited() {
                self.mark_released();
                let tail = log_tail(&self.paths.stderr_log, self.log_offset);
                self.last_start_error =
                    if tail.is_empty() { format!("sing-box 提前退出（代码 {code}）") } else { format!("sing-box 提前退出（代码 {code}）：{tail}") };
                return false;
            }
            if (self.ready)() {
                self.process = Some(process);
                return true;
            }
            if Instant::now() >= deadline {
                self.last_start_error = "等待 ProxyPilot 虚拟网卡就绪超时".into();
                process.kill();
                self.mark_released();
                return false;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn restore_previous(&mut self, reason: String, had_current: bool) -> ApplyOutcome {
        if !(had_current && self.paths.previous.exists()) {
            return ApplyOutcome::failed(reason, false);
        }
        let rollback = self.paths.current.with_extension("json.rollback");
        let restored = fs::copy(&self.paths.previous, &rollback).and_then(|_| fs::rename(&rollback, &self.paths.current));
        if let Err(error) = restored {
            return ApplyOutcome { rolled_back: true, ..ApplyOutcome::failed(format!("{reason}；恢复上一配置时出错：{error}"), false) };
        }
        if self.start_current() {
            return ApplyOutcome { rolled_back: true, ..ApplyOutcome::failed(format!("{reason}，已恢复上一配置"), true) };
        }
        let detail = if self.last_start_error.is_empty() { "未知错误".to_owned() } else { self.last_start_error.clone() };
        ApplyOutcome { rolled_back: true, ..ApplyOutcome::failed(format!("{reason}；上一配置也无法启动：{detail}"), false) }
    }

    /// Validate `candidate`, then swap it in. See the module docs for the order.
    pub fn apply(&mut self, candidate: &Path) -> ApplyOutcome {
        if let Err(message) = self.validate(candidate) {
            let running = self.running();
            return ApplyOutcome::failed(format!("新配置未通过检查：{message}"), running);
        }
        let had_current = self.paths.current.exists();
        if had_current
            && let Err(error) = fs::copy(&self.paths.current, &self.paths.previous) {
                let running = self.running();
                return ApplyOutcome::failed(format!("无法备份当前配置：{error}"), running);
            }
        self.stop();
        let incoming = self.paths.current.with_extension("json.next");
        let installed = fs::copy(candidate, &incoming).and_then(|_| fs::rename(&incoming, &self.paths.current));
        if let Err(error) = installed {
            return self.restore_previous(format!("安装新配置失败：{error}"), had_current);
        }
        if self.start_current() {
            return ApplyOutcome::ok("分流已启用");
        }
        if self.adapter_taken {
            // The previous config would meet the same adapter; nothing to roll back to.
            return ApplyOutcome { adapter_taken: true, ..ApplyOutcome::failed(ADAPTER_TAKEN.into(), false) };
        }
        let detail = if self.last_start_error.is_empty() { "未知错误".to_owned() } else { self.last_start_error.clone() };
        self.restore_previous(format!("新配置启动失败：{detail}"), had_current)
    }

    /// Start the installed config again, after a crash.
    pub fn restart(&mut self) -> Result<(), String> {
        if !self.paths.current.exists() {
            return Err("没有可用的配置".into());
        }
        self.stop();
        if self.start_current() { Ok(()) } else { Err(self.last_start_error.clone()) }
    }

    pub fn stop(&mut self) {
        if let Some(mut process) = self.process.take() {
            if process.exited().is_none() {
                process.kill();
            }
            self.mark_released();
        }
    }
}

impl Drop for CoreManager {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Script {
        /// Config file contents that fail `check`.
        rejected: Arc<Mutex<Vec<String>>>,
        /// Config file contents whose process exits immediately.
        crashing: Arc<Mutex<Vec<String>>>,
        starts: Arc<AtomicUsize>,
        alive: Arc<AtomicBool>,
    }

    struct FakeProcess {
        crashed: bool,
        alive: Arc<AtomicBool>,
    }

    impl CoreProcess for FakeProcess {
        fn pid(&self) -> u32 {
            42
        }
        fn exited(&mut self) -> Option<i32> {
            if self.crashed || !self.alive.load(Ordering::SeqCst) { Some(1) } else { None }
        }
        fn kill(&mut self) {
            self.alive.store(false, Ordering::SeqCst);
        }
    }

    impl Runner for Script {
        fn check(&self, config: &Path) -> Result<(), String> {
            let text = fs::read_to_string(config).unwrap();
            if self.rejected.lock().unwrap().contains(&text) { Err("bad".into()) } else { Ok(()) }
        }
        fn start(&self, config: &Path, _: &Path, log: &Path) -> io::Result<Box<dyn CoreProcess>> {
            let n = self.starts.fetch_add(1, Ordering::SeqCst) + 1;
            let text = fs::read_to_string(config).unwrap();
            let crashed = self.crashing.lock().unwrap().contains(&text);
            if crashed {
                // Like sing-box, append to the shared log rather than truncate it.
                let mut file = fs::OpenOptions::new().create(true).append(true).open(log)?;
                writeln!(file, "FATAL start {n}")?;
            }
            self.alive.store(!crashed, Ordering::SeqCst);
            Ok(Box::new(FakeProcess { crashed, alive: self.alive.clone() }))
        }
    }

    fn manager(dir: &Path, script: &Script) -> CoreManager {
        let alive = script.alive.clone();
        CoreManager::new(Box::new(script.clone()), RuntimePaths::new(dir), Box::new(move || alive.load(Ordering::SeqCst)))
            .with_startup_grace(Duration::from_millis(200))
    }

    fn candidate(dir: &Path, text: &str) -> PathBuf {
        let path = dir.join("candidate.json");
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn first_apply_installs_and_starts() {
        let dir = tempfile::tempdir().unwrap();
        let script = Script::default();
        let mut core = manager(dir.path(), &script);
        let outcome = core.apply(&candidate(dir.path(), "v1"));
        assert!(outcome.ok, "{outcome:?}");
        assert!(core.running());
        assert_eq!(fs::read_to_string(&core.paths().current).unwrap(), "v1");
    }

    #[test]
    fn rejected_candidate_leaves_running_core_alone() {
        let dir = tempfile::tempdir().unwrap();
        let script = Script::default();
        let mut core = manager(dir.path(), &script);
        assert!(core.apply(&candidate(dir.path(), "v1")).ok);
        script.rejected.lock().unwrap().push("v2".into());
        let outcome = core.apply(&candidate(dir.path(), "v2"));
        assert!(!outcome.ok);
        assert!(outcome.kept_previous);
        assert_eq!(script.starts.load(Ordering::SeqCst), 1);
        assert_eq!(fs::read_to_string(&core.paths().current).unwrap(), "v1");
    }

    #[test]
    fn crashing_candidate_rolls_back_to_previous() {
        let dir = tempfile::tempdir().unwrap();
        let script = Script::default();
        let mut core = manager(dir.path(), &script);
        assert!(core.apply(&candidate(dir.path(), "v1")).ok);
        script.crashing.lock().unwrap().push("v2".into());
        let outcome = core.apply(&candidate(dir.path(), "v2"));
        assert!(!outcome.ok);
        assert!(outcome.rolled_back && outcome.kept_previous, "{outcome:?}");
        assert!(core.running());
        assert_eq!(fs::read_to_string(&core.paths().current).unwrap(), "v1");
    }

    #[test]
    fn crash_on_first_apply_has_nothing_to_roll_back_to() {
        let dir = tempfile::tempdir().unwrap();
        let script = Script::default();
        script.crashing.lock().unwrap().push("v1".into());
        let mut core = manager(dir.path(), &script);
        let outcome = core.apply(&candidate(dir.path(), "v1"));
        assert!(!outcome.ok && !outcome.rolled_back);
        assert!(!core.running());
    }

    #[test]
    fn poll_exit_reports_a_crash_once_and_restart_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let script = Script::default();
        let mut core = manager(dir.path(), &script);
        assert!(core.apply(&candidate(dir.path(), "v1")).ok);
        assert!(core.poll_exit().is_none());
        script.alive.store(false, Ordering::SeqCst);
        assert!(core.poll_exit().unwrap().contains("意外退出"));
        assert!(core.poll_exit().is_none());
        core.restart().unwrap();
        assert!(core.running());
    }

    #[test]
    fn adapter_held_by_another_program_is_not_mistaken_for_ours() {
        let dir = tempfile::tempdir().unwrap();
        let script = Script::default();
        // Up before we start anything: somebody else's adapter with our name.
        script.alive.store(true, Ordering::SeqCst);
        let mut core = manager(dir.path(), &script);
        let outcome = core.apply(&candidate(dir.path(), "v1"));
        assert!(!outcome.ok && outcome.adapter_taken && !outcome.kept_previous, "{outcome:?}");
        assert_eq!(outcome.message, ADAPTER_TAKEN);
        assert_eq!(script.starts.load(Ordering::SeqCst), 0);
        assert!(!core.may_own_adapter());
        // The other program quits; the installed config starts.
        script.alive.store(false, Ordering::SeqCst);
        core.restart().unwrap();
        assert!(core.running() && core.may_own_adapter());
        assert_eq!(script.starts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn exit_reason_comes_from_this_run_only() {
        let dir = tempfile::tempdir().unwrap();
        let script = Script::default();
        script.crashing.lock().unwrap().push("v1".into());
        let mut core = manager(dir.path(), &script);
        assert!(core.apply(&candidate(dir.path(), "v1")).message.contains("start 1"));
        let error = core.restart().unwrap_err();
        assert!(error.contains("start 2") && !error.contains("start 1"), "{error}");
    }

    #[test]
    fn stopped_core_may_still_own_its_adapter_for_a_moment() {
        let dir = tempfile::tempdir().unwrap();
        let script = Script::default();
        let mut core = manager(dir.path(), &script);
        assert!(!core.may_own_adapter());
        assert!(core.apply(&candidate(dir.path(), "v1")).ok);
        core.stop();
        assert!(!core.running());
        assert!(core.may_own_adapter());
    }

    #[test]
    fn clean_output_strips_colours_and_keeps_the_tail() {
        let text = "\u{1b}[31mFATAL\u{1b}[0m[0000] decode config: unknown field \"x\"\n\n";
        assert_eq!(clean_output(text), "FATAL[0000] decode config: unknown field \"x\"");
    }
}

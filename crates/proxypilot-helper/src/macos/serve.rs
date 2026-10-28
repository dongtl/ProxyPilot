//! The socket the app talks to. Only the user who installed the helper (and
//! root) may connect. The connection that last applied rules owns the core:
//! when it closes, because the app quit or crashed, sing-box stops and the
//! network is back to direct, as a Job Object does on Windows.

use super::VERSION;
use super::fsx::{root_dir, unprotected};
use super::install::{PID_FILE, data_dir, install_dir, kill_stale_core, place_files, verified_files};
use proxypilot_core::backend::{Backend, LocalBackend};
use proxypilot_core::helper::*;
use std::fs;
use std::io::BufReader;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

struct Daemon {
    backend: LocalBackend,
    /// The connection whose rules are running.
    controller: Option<u64>,
    /// Why sing-box must not be started: a file it runs from is not root-only.
    refusal: Option<String>,
}

fn lock(daemon: &Mutex<Daemon>) -> MutexGuard<'_, Daemon> {
    daemon.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let (mut uid, mut gid) = (0, 0);
    (unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } == 0).then_some(uid)
}

pub fn serve(owner: u32) -> Result<(), String> {
    kill_stale_core();
    root_dir(&data_dir(), 0o700)?;
    let sing_box = install_dir().join(SING_BOX_FILE);
    let refusal = [install_dir().join(HELPER_FILE), sing_box.clone(), data_dir()]
        .iter()
        .find_map(|path| unprotected(path))
        .map(|path| format!("{} 的权限不安全，普通用户可以修改它。请移除后台助手后重新安装", path.display()));
    let daemon = Arc::new(Mutex::new(Daemon { backend: LocalBackend::new(sing_box, &data_dir()), controller: None, refusal }));

    let _ = fs::remove_file(SOCKET);
    let listener = UnixListener::bind(SOCKET).map_err(|error| format!("无法监听 {SOCKET}：{error}"))?;
    // Anyone may connect; `peer_uid` decides who is answered.
    fs::set_permissions(SOCKET, fs::Permissions::from_mode(0o666)).map_err(|error| error.to_string())?;
    let next_id = AtomicU64::new(1);
    for stream in listener.incoming().flatten() {
        if !peer_uid(&stream).is_some_and(|uid| uid == 0 || uid == owner) {
            continue;
        }
        let id = next_id.fetch_add(1, Ordering::Relaxed);
        let daemon = daemon.clone();
        std::thread::spawn(move || connection(id, stream, &daemon));
    }
    Ok(())
}

fn sync_pid(backend: &LocalBackend) {
    let path = data_dir().join(PID_FILE);
    match backend.pid() {
        Some(pid) => {
            let _ = fs::write(path, pid.to_string());
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
}

fn connection(id: u64, stream: UnixStream, daemon: &Mutex<Daemon>) {
    let Ok(writer) = stream.try_clone() else { return };
    let (mut reader, mut writer) = (BufReader::new(stream), writer);
    while let Ok(Some(request)) = read_message::<Request>(&mut reader) {
        let upgrade = matches!(request, Request::Upgrade { .. });
        let reply = dispatch(id, request, daemon);
        let sent = write_message(&mut writer, &reply).is_ok();
        if upgrade && reply.ok {
            // launchd starts the new helper; the app reconnects to it.
            std::process::exit(0);
        }
        if !sent {
            break;
        }
    }
    let mut daemon = lock(daemon);
    if daemon.controller == Some(id) {
        daemon.backend.stop();
        daemon.controller = None;
        sync_pid(&daemon.backend);
    }
}

fn state(backend: &mut LocalBackend) -> CoreState {
    // poll_exit first: it forgets an exited core, which `running` then reflects.
    let exited = backend.poll_exit();
    CoreState { running: backend.running(), pid: backend.pid(), may_own_adapter: backend.may_own_adapter(), exited }
}

fn dispatch(id: u64, request: Request, daemon: &Mutex<Daemon>) -> Reply {
    let mut daemon = lock(daemon);
    let reply = match request {
        Request::Hello => Reply { ok: true, version: VERSION.into(), ..Reply::default() },
        Request::Apply { request } => match daemon.refusal.clone() {
            Some(refusal) => Reply::failed(refusal),
            None => {
                let outcome = daemon.backend.apply(&request);
                daemon.controller = Some(id);
                Reply { ok: true, outcome: Some(outcome), state: state(&mut daemon.backend), ..Reply::default() }
            }
        },
        Request::Restart => match daemon.refusal.clone() {
            Some(refusal) => Reply::failed(refusal),
            None => match daemon.backend.restart() {
                Ok(()) => {
                    daemon.controller = Some(id);
                    Reply { ok: true, state: state(&mut daemon.backend), ..Reply::default() }
                }
                Err(error) => Reply::failed(error),
            },
        },
        Request::Stop => {
            daemon.backend.stop();
            daemon.controller = None;
            Reply { ok: true, ..Reply::default() }
        }
        Request::Status => Reply { ok: true, state: state(&mut daemon.backend), ..Reply::default() },
        Request::Upgrade { dir } => match verified_files(Path::new(&dir), false) {
            Ok((manifest, helper, sing_box)) => {
                daemon.backend.stop();
                daemon.controller = None;
                match place_files(&helper, &sing_box) {
                    Ok(()) => Reply { ok: true, version: manifest.version, ..Reply::default() },
                    Err(error) => Reply::failed(error),
                }
            }
            Err(error) => Reply::failed(error),
        },
    };
    sync_pid(&daemon.backend);
    reply
}

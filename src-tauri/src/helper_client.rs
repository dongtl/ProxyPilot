//! macOS: the app's side of the background helper. The app runs as the user;
//! the helper, installed once with the user's password, runs sing-box as root.
//! See docs/specs/2026-10-28-macos-design.md.

use proxypilot_core::backend::{ApplyRequest, Backend};
use proxypilot_core::core::ApplyOutcome;
use proxypilot_core::helper::{self, Client, CoreState, Reply, Request};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The helper files inside this app: `Contents/Resources/helper`.
pub fn bundled_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.parent()?.join("Resources").join("helper");
    dir.join(helper::HELPER_FILE).exists().then_some(dir)
}

pub fn installed() -> bool {
    Path::new(helper::PLIST).exists() && Path::new(helper::INSTALL_DIR).join(helper::HELPER_FILE).exists()
}

pub struct HelperBackend {
    version: String,
    client: Option<Client>,
    state: CoreState,
}

impl HelperBackend {
    pub fn new(version: String) -> HelperBackend {
        HelperBackend { version, client: None, state: CoreState::default() }
    }

    fn hello(client: &mut Client) -> Result<String, String> {
        let reply = client.call(&Request::Hello).map_err(|error| error.to_string())?;
        if reply.ok { Ok(reply.version) } else { Err(reply.error) }
    }

    /// Connect, and bring the helper to this app's version: the upgrade is
    /// checked by the helper against the signed manifest, so no password.
    fn connect(&mut self) -> Result<&mut Client, String> {
        if self.client.is_none() {
            if !installed() {
                return Err("后台助手还没有安装，请重新打开分流开关".into());
            }
            let mut client = Client::connect().map_err(|_| "连不上后台助手，请把分流开关关掉再打开".to_owned())?;
            let version = Self::hello(&mut client)?;
            if version != self.version {
                let dir = bundled_dir().ok_or("找不到新版后台助手的文件，请重新安装 ProxyPilot")?;
                let reply = client.call(&Request::Upgrade { dir: dir.to_string_lossy().into_owned() }).map_err(|e| e.to_string())?;
                if !reply.ok {
                    return Err(format!("后台助手没能自动升级（{}）。请把分流开关关掉再打开，输入密码重新安装", reply.error));
                }
                client = reconnect(Duration::from_secs(15)).ok_or("后台助手升级后没有重新启动，请把分流开关关掉再打开")?;
                let now = Self::hello(&mut client)?;
                if now != self.version {
                    return Err(format!("后台助手版本 {now} 和应用 {} 不一致，请把分流开关关掉再打开", self.version));
                }
            }
            self.client = Some(client);
        }
        Ok(self.client.as_mut().expect("just connected"))
    }

    /// One round trip; a broken connection is dropped so the next call reconnects.
    fn call(&mut self, request: &Request) -> Result<Reply, String> {
        let result = self.connect()?.call(request).map_err(|_| "和后台助手的连接断开了".to_owned());
        if result.is_err() {
            self.client = None;
        }
        result
    }
}

fn reconnect(within: Duration) -> Option<Client> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if let Ok(client) = Client::connect() {
            return Some(client);
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    None
}

fn failed(message: String) -> ApplyOutcome {
    ApplyOutcome { ok: false, message, rolled_back: false, kept_previous: false, adapter_taken: false }
}

impl Backend for HelperBackend {
    fn apply(&mut self, request: &ApplyRequest) -> ApplyOutcome {
        match self.call(&Request::Apply { request: request.clone() }) {
            Ok(reply) if reply.ok => {
                self.state = reply.state;
                reply.outcome.unwrap_or_else(|| failed("后台助手没有返回结果".into()))
            }
            Ok(reply) => failed(reply.error),
            Err(error) => failed(error),
        }
    }

    fn restart(&mut self) -> Result<(), String> {
        let reply = self.call(&Request::Restart)?;
        if !reply.ok {
            return Err(reply.error);
        }
        self.state = reply.state;
        Ok(())
    }

    fn stop(&mut self) {
        if self.client.is_some() {
            let _ = self.call(&Request::Stop);
        }
        self.state = CoreState::default();
    }

    fn running(&mut self) -> bool {
        self.state.running
    }

    fn pid(&self) -> Option<u32> {
        self.state.pid
    }

    /// Also refreshes the cached state the other getters return; the
    /// supervisor calls it on every tick.
    fn poll_exit(&mut self) -> Option<String> {
        if self.client.is_none() && !self.state.running {
            return None;
        }
        match self.call(&Request::Status) {
            Ok(reply) => {
                self.state = reply.state;
                self.state.exited.take()
            }
            Err(error) => {
                let was_running = self.state.running;
                self.state = CoreState::default();
                was_running.then_some(error)
            }
        }
    }

    fn may_own_adapter(&self) -> bool {
        self.state.may_own_adapter
    }

    fn ready(&mut self) -> bool {
        self.connect().is_ok()
    }
}

/// Text for AppleScript's `"..."`.
fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Text for the shell's `'...'`.
pub(crate) fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Run `command` as root after the system password prompt. `Err(None)`: the
/// user cancelled the prompt; `Err(Some(text))`: it failed.
pub(crate) fn admin_shell(command: &str, prompt: &str) -> Result<(), Option<String>> {
    let script = format!(
        "do shell script {} with prompt {} with administrator privileges",
        applescript_string(command),
        applescript_string(prompt)
    );
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| Some(format!("无法请求管理员权限：{error}")))?;
    if output.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&output.stderr);
    if text.contains("-128") {
        return Err(None);
    }
    Err(Some(text.trim().to_owned()))
}

fn run_as_admin(command: &str, prompt: &str) -> Result<(), String> {
    admin_shell(command, prompt).map_err(|error| match error {
        None => "已取消：没有输入密码，分流没有开启".to_owned(),
        Some(text) => format!("安装后台助手失败：{text}"),
    })
}

fn owner_uid() -> u32 {
    unsafe { libc::getuid() }
}

/// Install (or reinstall) the helper from this app bundle. Asks for the password.
pub fn install() -> Result<(), String> {
    let dir = bundled_dir().ok_or("安装不完整，找不到后台助手的文件，请重新安装 ProxyPilot")?;
    let command = format!(
        "{} install --owner {} --from {}",
        shell_quote(&dir.join(helper::HELPER_FILE).to_string_lossy()),
        owner_uid(),
        shell_quote(&dir.to_string_lossy())
    );
    run_as_admin(&command, "ProxyPilot 需要安装一个后台助手来创建分流用的虚拟网卡，只需要这一次。")
}

/// Remove the helper and everything it installed. Asks for the password.
pub fn uninstall() -> Result<(), String> {
    let installed = Path::new(helper::INSTALL_DIR).join(helper::HELPER_FILE);
    let program = if installed.exists() { installed } else { bundled_dir().ok_or("找不到后台助手")?.join(helper::HELPER_FILE) };
    run_as_admin(&format!("{} uninstall", shell_quote(&program.to_string_lossy())), "ProxyPilot 要移除后台助手。")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_survives_quotes_in_paths() {
        assert_eq!(shell_quote("/Apps/Bob's.app"), r"'/Apps/Bob'\''s.app'");
        assert_eq!(applescript_string(r#"a "b" \c"#), r#""a \"b\" \\c""#);
    }
}

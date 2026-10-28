//! The macOS background helper: where it lives, what the app and it say to
//! each other, and how it checks the files it installs. The protocol and the
//! checks are plain data and compile everywhere; only the client needs Unix.
//!
//! The helper runs as root, so everything it accepts is checked here:
//! requests are one bounded JSON line each, rules are validated by
//! `ApplyRequest::validate`, and files it installs must match a manifest
//! signed with the release key built into it.

use crate::backend::ApplyRequest;
use crate::core::ApplyOutcome;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

pub const LABEL: &str = "app.proxypilot.helper";
pub const SOCKET: &str = "/var/run/app.proxypilot.helper.sock";
/// The helper and its sing-box, root-owned. Only these copies ever run as root.
pub const INSTALL_DIR: &str = "/Library/PrivilegedHelperTools/app.proxypilot.helper";
/// Configs and sing-box logs, readable by root only.
pub const DATA_DIR: &str = "/Library/Application Support/app.proxypilot.helper";
pub const PLIST: &str = "/Library/LaunchDaemons/app.proxypilot.helper.plist";
pub const HELPER_FILE: &str = "proxypilot-helper";
pub const SING_BOX_FILE: &str = "sing-box";
pub const MANIFEST_FILE: &str = "manifest.json";
pub const SIGNATURE_FILE: &str = "manifest.json.sig";
/// One request or reply. An apply with thousands of rules fits easily.
pub const MAX_LINE: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Request {
    Hello,
    Apply { request: ApplyRequest },
    Restart,
    Stop,
    Status,
    /// Replace the helper and sing-box with the copies in `dir` (the new
    /// app's `Contents/Resources/helper`), if the signed manifest allows it.
    Upgrade { dir: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreState {
    pub running: bool,
    pub pid: Option<u32>,
    pub may_own_adapter: bool,
    /// Set once when the core exited on its own since the last status.
    pub exited: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub ok: bool,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub outcome: Option<ApplyOutcome>,
    #[serde(default)]
    pub state: CoreState,
}

impl Reply {
    pub fn failed(error: impl Into<String>) -> Reply {
        Reply { ok: false, error: error.into(), ..Reply::default() }
    }
}

pub fn write_message(out: &mut impl Write, message: &impl Serialize) -> io::Result<()> {
    let mut line = serde_json::to_vec(message).map_err(io::Error::other)?;
    line.push(b'\n');
    out.write_all(&line)?;
    out.flush()
}

/// The next line as `T`; `Ok(None)` at a clean end of stream. A line longer
/// than [`MAX_LINE`] is an error, so a caller cannot make us buffer forever.
pub fn read_message<T: for<'de> Deserialize<'de>>(input: &mut impl BufRead) -> io::Result<Option<T>> {
    let mut line = Vec::new();
    let mut limited = io::Read::take(&mut *input, MAX_LINE as u64 + 1);
    let read = limited.read_until(b'\n', &mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if line.last() != Some(&b'\n') {
        let kind = if line.len() > MAX_LINE { io::ErrorKind::InvalidData } else { io::ErrorKind::UnexpectedEof };
        return Err(io::Error::new(kind, "message too long or cut off"));
    }
    serde_json::from_slice(&line).map(Some).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// The update public key from `tauri.conf.json` (a test keeps them equal).
/// CI smoke tests sign with a throwaway key and build with
/// `PROXYPILOT_TEST_RELEASE_KEY`; the release script refuses to build when it
/// is set, so a shipped helper only ever trusts the real key.
pub const RELEASE_KEY: &str = match option_env!("PROXYPILOT_TEST_RELEASE_KEY") {
    Some(key) => key,
    None => "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEVDMDVFQ0M2RkQ1MTgwQjIKUldTeWdGSDl4dXdGN0JpUDdsaDF3d216V0txNjhNWDBFSlQyMVB6MzNFNU5aTk1UMUJNdG0vSVcK",
};

/// What a release ships in `Contents/Resources/helper`: its version and the
/// SHA256 of each file the helper installs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub files: BTreeMap<String, String>,
}

fn decode_text(encoded: &str) -> Result<String, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded.trim()).map_err(|_| "签名格式不对".to_owned())?;
    String::from_utf8(bytes).map_err(|_| "签名格式不对".to_owned())
}

/// Check `manifest` against `signature` (a `.sig` as `tauri signer sign`
/// writes it) with `key` (base64 like `RELEASE_KEY`), then parse it.
pub fn verify_manifest(manifest: &[u8], signature: &str, key: &str) -> Result<Manifest, String> {
    let key = minisign_verify::PublicKey::decode(&decode_text(key)?).map_err(|_| "公钥格式不对".to_owned())?;
    let signature = minisign_verify::Signature::decode(&decode_text(signature)?).map_err(|_| "签名格式不对".to_owned())?;
    key.verify(manifest, &signature, false).map_err(|_| "后台助手的文件没有通过签名校验".to_owned())?;
    serde_json::from_slice(manifest).map_err(|error| format!("文件清单格式不对：{error}"))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `0.3.0` → (0, 3, 0); anything else is not a version.
fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim().split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// Whether `candidate` may replace `installed`: same or newer. Never older,
/// so a signed but outdated helper cannot be put back to reopen an old bug.
pub fn may_replace(installed: &str, candidate: &str) -> bool {
    matches!((parse_version(installed), parse_version(candidate)), (Some(old), Some(new)) if new >= old)
}

/// The bytes of `name`, if the manifest lists it with this hash.
pub fn checked_file(manifest: &Manifest, name: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    match manifest.files.get(name) {
        Some(expected) if expected.eq_ignore_ascii_case(&sha256_hex(&bytes)) => Ok(bytes),
        Some(_) => Err(format!("{name} 和签名清单里的不一致")),
        None => Err(format!("签名清单里没有 {name}")),
    }
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A launchd job: the helper's LaunchDaemon (`keep_alive`, logging to
/// `stderr`) or the app's login LaunchAgent (run once at load).
pub fn launchd_plist(label: &str, program: &[&str], keep_alive: bool, stderr: Option<&str>) -> String {
    let args: String = program.iter().map(|arg| format!("\n    <string>{}</string>", xml_escape(arg))).collect();
    let stderr = stderr.map(|path| format!("\n  <key>StandardErrorPath</key>\n  <string>{}</string>", xml_escape(path))).unwrap_or_default();
    let keep = if keep_alive { "true" } else { "false" };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>{args}
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <{keep}/>{stderr}
</dict>
</plist>
"#,
        xml_escape(label)
    )
}

/// The app's end of the socket.
#[cfg(unix)]
pub struct Client {
    reader: io::BufReader<std::os::unix::net::UnixStream>,
    writer: std::os::unix::net::UnixStream,
}

#[cfg(unix)]
impl Client {
    pub fn connect() -> io::Result<Client> {
        Client::connect_to(std::path::Path::new(SOCKET))
    }

    pub fn connect_to(path: &std::path::Path) -> io::Result<Client> {
        let stream = std::os::unix::net::UnixStream::connect(path)?;
        // An apply can wait for a check, a stop, a start and a rollback.
        stream.set_read_timeout(Some(std::time::Duration::from_secs(90)))?;
        let writer = stream.try_clone()?;
        Ok(Client { reader: io::BufReader::new(stream), writer })
    }

    pub fn call(&mut self, request: &Request) -> io::Result<Reply> {
        write_message(&mut self.writer, request)?;
        read_message(&mut self.reader)?.ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "后台助手断开了连接"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Signed with a throwaway key made for this test; the release key is not involved.
    const TEST_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDNCNjRENTQ1OTY4MzI0RUUKUldUdUpJT1dSZFZrT3pzRjg3d1lnbFk1bmZ1dFo5Y3l4Q1dKODBURkx0S2xZVkdoWW1JVlJFSC8K";
    const MANIFEST: &str = r#"{"version":"0.3.0","files":{"proxypilot-helper":"e81d3b0e9d82feaaf5f6e55bdff24731d7eee08632ffa63801e6397290c5d20a","sing-box":"f83bf032fc716380bca7971daa8c61630c0b419d28874c83f90a45ddde1c5a75"}}"#;
    const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUdUpJT1dSZFZrT3c3REVoV3NUcUlCVUVZRDVjVTJmNFAvcHRHdnl5eFBrUERwK2FUTUhPWWdkTVQwWGtZems5RkpUVm1jSU5qb2cxOSt5YWpobkpnQ0dzaUV0eXIweFFzPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkzMTc2MDMwCWZpbGU6bWFuaWZlc3QuanNvbgo1NVZoRjFGaWtWQjFEbnR3WjBQU0NHOVFJUThKejMzbnNZSUpjUktUZUtxS1oxT3BWOGhuZXVpTTdURXczd0t2Mi9paGg4bmNqanFyRG56TzVja0dEQT09Cg==";

    #[test]
    fn signed_manifest_is_accepted_and_its_files_checked() {
        let manifest = verify_manifest(MANIFEST.as_bytes(), SIGNATURE, TEST_KEY).unwrap();
        assert_eq!(manifest.version, "0.3.0");
        assert!(checked_file(&manifest, HELPER_FILE, b"helper".to_vec()).is_ok());
        assert!(checked_file(&manifest, SING_BOX_FILE, b"sing-box!".to_vec()).is_err());
        assert!(checked_file(&manifest, "other", b"helper".to_vec()).is_err());
    }

    #[test]
    fn tampered_manifest_or_another_key_is_refused() {
        let tampered = MANIFEST.replace("0.3.0", "9.9.9");
        assert!(verify_manifest(tampered.as_bytes(), SIGNATURE, TEST_KEY).is_err());
        assert!(verify_manifest(MANIFEST.as_bytes(), SIGNATURE, RELEASE_KEY).is_err());
        assert!(verify_manifest(MANIFEST.as_bytes(), "not base64", TEST_KEY).is_err());
    }

    #[test]
    fn release_key_matches_the_updater_config() {
        if option_env!("PROXYPILOT_TEST_RELEASE_KEY").is_some() {
            return;
        }
        let conf = include_str!("../../../src-tauri/tauri.conf.json");
        assert!(conf.contains(&format!("\"pubkey\": \"{RELEASE_KEY}\"")), "helper::RELEASE_KEY differs from tauri.conf.json");
    }

    #[test]
    fn only_same_or_newer_versions_replace_the_helper() {
        assert!(may_replace("0.3.0", "0.3.0"));
        assert!(may_replace("0.3.0", "0.3.1"));
        assert!(may_replace("0.3.9", "0.10.0"));
        assert!(!may_replace("0.3.1", "0.3.0"));
        assert!(!may_replace("0.3.0", "0.3.0-beta"));
        assert!(!may_replace("garbage", "0.3.0"));
    }

    #[test]
    fn plists_escape_arguments() {
        let daemon = launchd_plist(LABEL, &["/Library/x/proxypilot-helper", "serve", "--owner", "501"], true, Some("/tmp/a&b.log"));
        assert!(daemon.contains("<string>serve</string>\n    <string>--owner</string>\n    <string>501</string>"));
        assert!(daemon.contains("<key>KeepAlive</key>\n  <true/>"));
        assert!(daemon.contains("<string>/tmp/a&amp;b.log</string>"));
        let agent = launchd_plist("app.proxypilot.desktop", &["/Applications/A<B>.app/Contents/MacOS/proxypilot-app", "--autostart"], false, None);
        assert!(agent.contains("A&lt;B&gt;.app") && agent.contains("<key>KeepAlive</key>\n  <false/>") && !agent.contains("StandardErrorPath"));
    }

    #[test]
    fn messages_round_trip_and_long_lines_are_refused() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, &Request::Upgrade { dir: "/Applications/ProxyPilot.app/Contents/Resources/helper".into() }).unwrap();
        write_message(&mut buffer, &Request::Status).unwrap();
        assert!(String::from_utf8_lossy(&buffer).starts_with(r#"{"op":"upgrade","dir":"#));
        let mut input = io::Cursor::new(buffer);
        assert!(matches!(read_message::<Request>(&mut input).unwrap(), Some(Request::Upgrade { .. })));
        assert_eq!(read_message::<Request>(&mut input).unwrap(), Some(Request::Status));
        assert_eq!(read_message::<Request>(&mut input).unwrap(), None);
        let long = vec![b'x'; MAX_LINE + 10];
        assert!(read_message::<Request>(&mut io::Cursor::new(long)).is_err());
    }
}

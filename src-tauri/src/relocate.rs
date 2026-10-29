//! macOS: keep ProxyPilot in the Applications folder. Opened straight from the
//! disk image, or before it was moved there (Gatekeeper then runs it from a
//! random read-only copy), it cannot update itself, and the login item points
//! at a place that is gone after a restart. So on launch the window offers,
//! with one button, to move it there and reopen it from there.

use serde::Serialize;

/// What the window offers; `None` when ProxyPilot is where it belongs.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Relocation {
    /// Copy this app into /Applications, replacing an older copy there.
    Move,
    /// /Applications already holds this version or a newer one: open that one.
    OpenInstalled,
}

#[cfg(target_os = "macos")]
const TARGET: &str = "/Applications/ProxyPilot.app";

/// In an Applications folder: the system's, or the user's own.
#[cfg(any(target_os = "macos", test))]
fn in_applications(bundle: &str, home: Option<&str>) -> bool {
    bundle.starts_with("/Applications/")
        || home.is_some_and(|home| bundle.starts_with(&format!("{}/Applications/", home.trim_end_matches('/'))))
}

/// `CFBundleShortVersionString` from an XML Info.plist, which is what the bundler writes.
#[cfg(any(target_os = "macos", test))]
fn plist_version(plist: &str) -> Option<String> {
    let after = plist.split_once("<key>CFBundleShortVersionString</key>")?.1;
    let value = after.trim_start().strip_prefix("<string>")?;
    Some(value.split_once("</string>")?.0.trim().to_owned())
}

/// Move, unless /Applications holds the same version or a newer one. A copy
/// there whose version cannot be read is replaced.
#[cfg(any(target_os = "macos", test))]
fn decide(ours: &str, installed: Option<&str>) -> Relocation {
    match installed {
        Some(installed) if proxypilot_core::helper::may_replace(ours, installed) => Relocation::OpenInstalled,
        _ => Relocation::Move,
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use crate::helper_client::{admin_shell, shell_quote};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    /// The bundle this process runs from; `None` outside one (a dev build).
    pub fn running_bundle() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        exe.ancestors().find(|path| path.extension().is_some_and(|ext| ext == "app")).map(Path::to_path_buf)
    }

    /// What to offer, worked out once at launch.
    pub fn check(version: &str) -> Option<Relocation> {
        let bundle = running_bundle()?;
        let home = std::env::var("HOME").ok();
        if in_applications(&bundle.to_string_lossy(), home.as_deref()) {
            return None;
        }
        let installed = std::fs::read_to_string(Path::new(TARGET).join("Contents/Info.plist")).ok();
        Some(decide(version, installed.as_deref().and_then(plist_version).as_deref()))
    }

    /// Copy `from` to `to` through a staging copy beside it, so a failure at
    /// any step leaves the old app in place. The quarantine mark goes, or
    /// Gatekeeper would run the copy from a read-only place again. One line:
    /// it also runs inside AppleScript's `do shell script`.
    pub(super) fn move_script(from: &Path, to: &Path) -> String {
        let q = |path: &Path| shell_quote(&path.to_string_lossy());
        let (from, staged, old, to) = (q(from), q(&to.with_extension("app.moving")), q(&to.with_extension("app.old")), q(to));
        [
            "set -e".to_owned(),
            format!("rm -rf {staged} {old}"),
            format!("/usr/bin/ditto {from} {staged}"),
            format!("/usr/bin/xattr -dr com.apple.quarantine {staged} 2>/dev/null || true"),
            format!("if [ -e {to} ]; then mv {to} {old}; fi"),
            format!("if ! mv {staged} {to}; then if [ -e {old} ]; then mv {old} {to}; fi; exit 1; fi"),
            format!("rm -rf {old}"),
        ]
        .join("; ")
    }

    /// Wait (up to 10 s) for this process to quit, open the app in
    /// /Applications, and eject our disk image if it is still mounted.
    fn reopen_script(pid: u32, target: &Path) -> String {
        let target = shell_quote(&target.to_string_lossy());
        format!(
            "i=0; while kill -0 {pid} 2>/dev/null && [ $i -lt 100 ]; do sleep 0.1; i=$((i+1)); done; \
             /usr/bin/xattr -dr com.apple.quarantine {target} 2>/dev/null; /usr/bin/open {target}; \
             for v in /Volumes/ProxyPilot*; do [ -d \"$v/ProxyPilot.app\" ] && /usr/bin/hdiutil detach -quiet \"$v\"; done; true"
        )
    }

    /// Do what `check` offered and arrange the reopening. The caller then quits.
    pub fn run(offer: Relocation) -> Result<(), String> {
        let target = Path::new(TARGET);
        if offer == Relocation::Move {
            let bundle = running_bundle().ok_or("找不到正在运行的 ProxyPilot")?;
            let script = move_script(&bundle, target);
            // An administrator can write to /Applications without a password;
            // a standard user, or an old copy there owned by root, needs one.
            let done = Command::new("/bin/sh")
                .arg("-c")
                .arg(&script)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !done {
                admin_shell(&script, "ProxyPilot 要把自己移到“应用程序”文件夹。").map_err(|error| match error {
                    None => "已取消，ProxyPilot 还在原来的位置".to_owned(),
                    Some(text) => format!("没能移到“应用程序”文件夹：{text}"),
                })?;
            }
        }
        Command::new("/bin/sh")
            .arg("-c")
            .arg(reopen_script(std::process::id(), target))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("无法重新打开 ProxyPilot：{error}"))?;
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::Relocation;

    pub fn check(_version: &str) -> Option<Relocation> {
        None
    }

    pub fn run(_offer: Relocation) -> Result<(), String> {
        Err("只有 Mac 版需要移到“应用程序”文件夹".into())
    }
}

pub use imp::{check, run};
#[cfg(target_os = "macos")]
pub use imp::running_bundle;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_applications_folder_counts_as_installed() {
        assert!(in_applications("/Applications/ProxyPilot.app", Some("/Users/li")));
        assert!(in_applications("/Users/li/Applications/ProxyPilot.app", Some("/Users/li/")));
        assert!(!in_applications("/Users/li/Desktop/ProxyPilot.app", Some("/Users/li")));
        assert!(!in_applications("/Volumes/ProxyPilot/ProxyPilot.app", Some("/Users/li")));
        assert!(!in_applications("/private/var/folders/x/T/AppTranslocation/1/d/ProxyPilot.app", None));
    }

    #[test]
    fn opens_the_installed_copy_unless_it_is_older() {
        let plist = "<dict>\n\t<key>CFBundleShortVersionString</key>\n\t<string>0.3.2</string>\n</dict>";
        assert_eq!(plist_version(plist).as_deref(), Some("0.3.2"));
        assert_eq!(plist_version("<dict></dict>"), None);
        assert_eq!(decide("0.3.2", None), Relocation::Move);
        assert_eq!(decide("0.3.2", Some("0.3.0")), Relocation::Move);
        assert_eq!(decide("0.3.2", Some("0.3.2")), Relocation::OpenInstalled);
        assert_eq!(decide("0.3.2", Some("0.4.0")), Relocation::OpenInstalled);
        assert_eq!(decide("0.3.2", Some("garbage")), Relocation::Move);
    }
}

/// Runs the real move on the CI Mac, in a temp folder standing in for /Applications.
#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::imp::move_script;
    use super::plist_version;
    use std::path::Path;
    use std::process::Command;

    fn make_bundle(bundle: &Path, version: &str) {
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        let plist = format!("<dict><key>CFBundleShortVersionString</key><string>{version}</string></dict>");
        std::fs::write(bundle.join("Contents/Info.plist"), plist).unwrap();
        std::fs::write(bundle.join("Contents/MacOS/ProxyPilot"), "").unwrap();
    }

    #[test]
    fn moving_replaces_an_older_copy_and_drops_the_quarantine_mark() {
        let root = std::env::temp_dir().join(format!("pp-relocate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // An apostrophe in the path, to exercise the quoting.
        let from = root.join("Bob's disk").join("ProxyPilot.app");
        let to = root.join("Applications").join("ProxyPilot.app");
        make_bundle(&from, "0.3.2");
        make_bundle(&to, "0.3.0");
        std::fs::write(to.join("Contents/stale"), "").unwrap();
        let marked = Command::new("/usr/bin/xattr")
            .args(["-w", "com.apple.quarantine", "0083;00000000;Safari;"])
            .arg(&from)
            .status()
            .unwrap();
        assert!(marked.success());

        let status = Command::new("/bin/sh").arg("-c").arg(move_script(&from, &to)).status().unwrap();
        assert!(status.success());

        let plist = std::fs::read_to_string(to.join("Contents/Info.plist")).unwrap();
        assert_eq!(plist_version(&plist).as_deref(), Some("0.3.2"));
        assert!(!to.join("Contents/stale").exists());
        let attributes = Command::new("/usr/bin/xattr").arg(&to).output().unwrap();
        assert!(!String::from_utf8_lossy(&attributes.stdout).contains("com.apple.quarantine"));
        let left: Vec<_> = std::fs::read_dir(root.join("Applications")).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from("ProxyPilot.app")]);
        // Copied, not taken away: the running app keeps its files until it quits.
        assert!(from.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}

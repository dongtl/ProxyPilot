//! `proxypilot-helper`: the root half of ProxyPilot on macOS.
//!
//! - `install --owner UID` (root, via the app's password prompt): copy this
//!   helper and sing-box from the app bundle into a root-owned folder, after
//!   checking them against the signed manifest, and register the LaunchDaemon.
//! - `serve --owner UID` (launchd): answer the app on the socket and run sing-box.
//! - `uninstall` (root): stop it all and remove every file it installed.
//!
//! See docs/specs/2026-10-28-macos-design.md. Other systems only get a stub,
//! so the workspace still builds and tests everywhere.

#[cfg(target_os = "macos")]
mod macos;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(target_os = "macos")]
    return macos::main(&args);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = args;
        eprintln!("proxypilot-helper 只用于 macOS");
        std::process::ExitCode::FAILURE
    }
}

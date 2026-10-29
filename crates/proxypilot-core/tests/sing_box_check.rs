//! Every config shape we can generate must pass the real `sing-box check` of
//! the pinned version. Needs the sidecar from scripts/fetch-sing-box.ps1 (or
//! PROXYPILOT_SING_BOX); skipped with a note when it is absent.

use proxypilot_core::config::{BuildInput, ClashApi, build_config, write_config};
use proxypilot_core::core::{Runner, SingBox};
use proxypilot_core::platform::Os;
use proxypilot_core::settings::Targets;
use proxypilot_core::{DnsMode, LogLevel, Upstream, UpstreamKind};
use std::path::PathBuf;
use std::process::Command;

fn sing_box() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("PROXYPILOT_SING_BOX") {
        return Some(PathBuf::from(path));
    }
    let fetched = if cfg!(windows) { "binaries/sing-box-x86_64-pc-windows-msvc.exe" } else { "binaries/macos/sing-box" };
    let bundled = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri").join(fetched);
    bundled.exists().then_some(bundled)
}

#[test]
fn generated_configs_pass_sing_box_check() {
    let Some(executable) = sing_box() else {
        eprintln!("skipped: sing-box not found, run scripts/fetch-sing-box.ps1");
        return;
    };
    let core = SingBox::new(executable.clone());
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("sing-box.log");
    let api = ClashApi { port: 49152, secret: "0123456789abcdef".into() };
    let full = Targets {
        processes: vec!["codex.exe".into(), "Zed.exe".into(), "Visual Studio Code.app".into()],
        domains: vec!["openai.com".into(), "github.com".into()],
    };
    let empty = Targets::default();
    let intranet = vec!["corp.example.cn".to_owned()];
    let direct = vec!["Corp.exe".to_owned()];

    let mut case = 0;
    for kind in [UpstreamKind::Socks5, UpstreamKind::Http] {
        for mode in [DnsMode::Encrypted, DnsMode::System] {
            for targets in [&full, &empty] {
                case += 1;
                let upstream = Upstream { kind, host: "127.0.0.1".into(), port: 10808 };
                let input = BuildInput {
                    upstream: &upstream,
                    upstream_owner: Some("xray.exe"),
                    targets,
                    direct_processes: &direct,
                    intranet_domains: &intranet,
                    dns_mode: mode,
                    log_level: LogLevel::Info,
                    log_path: &log,
                    clash_api: &api,
                    os: Os::CURRENT,
                };
                // Windows checks both shapes, so the macOS one is covered
                // there too. Elsewhere only the macOS shape: the Windows one
                // names its TUN, which a darwin sing-box may refuse.
                let shapes: &[Os] = if cfg!(windows) { &[Os::Windows, Os::MacOs] } else { &[Os::MacOs] };
                for &os in shapes {
                    let input = BuildInput { os, ..input };
                    let path = dir.path().join(format!("case-{case}-{os:?}.json"));
                    write_config(&build_config(&input), &path).unwrap();
                    let label = format!("case {case} ({kind:?}, {mode:?}, {} targets, {os:?})", targets.domains.len());
                    if let Err(message) = core.check(&path) {
                        panic!("{label} rejected:\n{message}");
                    }
                    // A deprecation today is a startup failure two releases later;
                    // fail here while the migration note is still current.
                    let output = Command::new(&executable).arg("check").arg("-c").arg(&path).output().unwrap();
                    let text = String::from_utf8_lossy(&output.stderr).to_lowercase();
                    assert!(!text.contains("deprecated"), "{label} uses a deprecated option:\n{text}");
                }
            }
        }
    }
}

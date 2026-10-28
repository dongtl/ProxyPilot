mod fsx;
mod install;
mod serve;

use std::process::ExitCode;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "usage: proxypilot-helper install --owner UID [--from DIR] | serve --owner UID | uninstall | version";

fn owner(args: &[String]) -> Result<u32, String> {
    let at = args.iter().position(|arg| arg == "--owner").ok_or("缺少 --owner")?;
    args.get(at + 1).and_then(|uid| uid.parse().ok()).ok_or_else(|| "--owner 不是有效的用户 ID".into())
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|arg| arg == name)?;
    args.get(at + 1).map(String::as_str)
}

fn require_root() -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 { Ok(()) } else { Err("需要以 root 身份运行".into()) }
}

pub fn main(args: &[String]) -> ExitCode {
    let result = match args.first().map(String::as_str) {
        Some("install") => require_root().and_then(|_| install::install(owner(args)?, option(args, "--from"))),
        Some("uninstall") => require_root().and_then(|_| install::uninstall()),
        Some("serve") => require_root().and_then(|_| serve::serve(owner(args)?)),
        Some("version") => {
            println!("{VERSION}");
            Ok(())
        }
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // The app shows this to the user when an install fails.
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

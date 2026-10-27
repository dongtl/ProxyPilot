//! `proxypilot-cli`: the core logic without the UI, for Phase 0 verification
//! and troubleshooting. Output is JSON so scripts can read it.

use proxypilot_core::config::{BuildInput, ClashApi, build_config, write_config};
use proxypilot_core::detect::{detect, upstream_owner};
use proxypilot_core::envcheck::{EnvInput, evaluate};
use proxypilot_core::probe::{probe_upstream, upstream_alive};
use proxypilot_core::settings::Settings;
use proxypilot_core::{DnsMode, LogLevel, Upstream, UpstreamKind};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "用法：
  proxypilot-cli detect [--full]
  proxypilot-cli env [--upstream socks5://127.0.0.1:10808]
  proxypilot-cli probe --upstream socks5://127.0.0.1:10808
  proxypilot-cli gen-config --upstream URL --out FILE [--log FILE] [--preset ID]... [--site DOMAIN]...
                            [--app EXE]... [--intranet DOMAIN]... [--dns encrypted|system] [--log-level warn|info|debug]
  proxypilot-cli check --core sing-box.exe --config FILE";

fn parse_upstream(text: &str) -> Result<Upstream, String> {
    let (kind, rest) = match text.split_once("://") {
        Some(("socks5" | "socks", rest)) => (UpstreamKind::Socks5, rest),
        Some(("http", rest)) => (UpstreamKind::Http, rest),
        Some((scheme, _)) => return Err(format!("不支持的类型：{scheme}")),
        None => (UpstreamKind::Socks5, text),
    };
    let (host, port) = rest.rsplit_once(':').ok_or("上游格式应为 socks5://127.0.0.1:10808")?;
    let port: u16 = port.trim_end_matches('/').parse().map_err(|_| "端口无效".to_owned())?;
    Upstream { kind, host: host.into(), port }.normalised()
}

struct Args {
    flags: Vec<(String, String)>,
    switches: Vec<String>,
}

impl Args {
    fn parse(raw: &[String]) -> Result<Args, String> {
        let mut flags = Vec::new();
        let mut switches = Vec::new();
        let mut iter = raw.iter();
        while let Some(arg) = iter.next() {
            let Some(name) = arg.strip_prefix("--") else { return Err(format!("多余的参数：{arg}")) };
            if name == "full" {
                switches.push(name.to_owned());
                continue;
            }
            let value = iter.next().ok_or(format!("--{name} 缺少值"))?;
            flags.push((name.to_owned(), value.clone()));
        }
        Ok(Args { flags, switches })
    }

    fn one(&self, name: &str) -> Option<&str> {
        self.flags.iter().rev().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    fn all(&self, name: &str) -> Vec<&str> {
        self.flags.iter().filter(|(key, _)| key == name).map(|(_, value)| value.as_str()).collect()
    }

    fn required(&self, name: &str) -> Result<&str, String> {
        self.one(name).ok_or(format!("缺少 --{name}"))
    }
}

fn print(value: &impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(value).expect("serialisable"));
}

fn run(command: &str, args: &Args) -> Result<(), String> {
    match command {
        "detect" => print(&detect(args.switches.iter().any(|s| s == "full"), &[std::process::id()])),
        "env" => {
            let upstream = args.one("upstream").map(parse_upstream).transpose()?;
            let alive = upstream.as_ref().is_some_and(|up| upstream_alive(up, Duration::from_millis(500)));
            // The CLI runs no core, so an adapter with our name is not its own,
            // and it is a dev tool, not an install whose folder we check.
            print(&evaluate(&EnvInput::gather(true, false, upstream.is_some(), alive, false)));
        }
        "probe" => print(&probe_upstream(&parse_upstream(args.required("upstream")?)?, Duration::from_secs(15))),
        "gen-config" => {
            let upstream = parse_upstream(args.required("upstream")?)?;
            let out = PathBuf::from(args.required("out")?);
            let log = args.one("log").map(PathBuf::from).unwrap_or_else(|| out.with_extension("log"));
            let mut settings = Settings::default();
            for id in args.all("preset") {
                settings.set_profile_enabled(id, true)?;
            }
            for site in args.all("site") {
                settings.add_site(site)?;
            }
            for app in args.all("app") {
                settings.add_app(app)?;
            }
            settings.intranet_domains = args.all("intranet").into_iter().map(str::to_owned).collect();
            settings.intranet_domains = proxypilot_core::domain::normalise_all(&settings.intranet_domains);
            settings.dns_mode = match args.one("dns") {
                None | Some("encrypted") => DnsMode::Encrypted,
                Some("system") => DnsMode::System,
                Some(other) => return Err(format!("未知的 DNS 模式：{other}")),
            };
            settings.log_level = match args.one("log-level") {
                None | Some("warn") => LogLevel::Warn,
                Some("info") => LogLevel::Info,
                Some("debug") => LogLevel::Debug,
                Some(other) => return Err(format!("未知的日志级别：{other}")),
            };
            let owner = upstream_owner(&upstream).and_then(|(_, name)| name);
            let api = ClashApi::generate().map_err(|error| error.to_string())?;
            let targets = settings.targets();
            let input = BuildInput::from_settings(&settings, &upstream, &targets, owner.as_deref(), &log, &api);
            write_config(&build_config(&input), &out).map_err(|error| error.to_string())?;
            print(&serde_json::json!({
                "config": out, "upstreamOwner": owner, "clashApiPort": api.port, "clashApiSecret": api.secret,
                "proxyProcesses": targets.processes, "proxyDomains": targets.domains,
            }));
        }
        "check" => {
            use proxypilot_core::core::{Runner, SingBox};
            let core = SingBox::new(PathBuf::from(args.required("core")?));
            core.check(&PathBuf::from(args.required("config")?))?;
            println!("ok");
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = raw.split_first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = Args::parse(rest).and_then(|args| run(command, &args));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

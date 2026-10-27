//! One-click self-check: make real connections through our own tunnel and
//! read back from sing-box which way each one went. Lets a tester who knows
//! nothing about networking produce a report a developer can act on.
//!
//! The report is meant to be pasted into a chat, so it never contains node
//! details, IP addresses or the user's intranet address.

use crate::config::ClashApi;
use crate::model::{EnvFinding, Route, Upstream};
use crate::probe::{is_certificate_error, probe_upstream};
use crate::traffic::fetch_connections;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

const DIRECT_HOST: &str = "www.baidu.com";
const STEP_TIMEOUT: Duration = Duration::from_secs(10);
/// Well-known hosts that answer HTTPS on the apex; tried first when enabled,
/// so a custom HTTP-only site does not make the proxy look broken.
const PREFERRED_PROXY_HOSTS: &[&str] = &["google.com", "github.com", "openai.com", "claude.ai", "chatgpt.com", "youtube.com"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    pub id: String,
    pub title: String,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    pub items: Vec<CheckItem>,
    /// Plain-text report for copying; also saved as diagnostics.txt.
    pub text: String,
}

pub struct DiagnoseInput<'a> {
    pub upstream: Option<&'a Upstream>,
    /// The core is up; route checks need it.
    pub routing: bool,
    pub clash_api: Option<&'a ClashApi>,
    pub proxy_domains: &'a [String],
    /// What the user typed as a company address, if anything.
    pub intranet: Option<&'a str>,
}

/// Header lines for the text report, filled in by the app.
pub struct ReportContext {
    pub version: String,
    pub time: String,
    pub elevated: bool,
    pub upstream: String,
    pub routing: String,
    pub dns_mode: String,
    pub profiles: Vec<String>,
}

fn item(id: &str, title: &str, status: CheckStatus, detail: impl Into<String>) -> CheckItem {
    CheckItem { id: id.into(), title: title.into(), status, detail: detail.into() }
}

enum Failure {
    Dns,
    Connect,
    Handshake,
    Certificate,
}

struct Trace {
    dns_ms: u64,
    total_ms: u64,
    route: Option<Route>,
}

/// Resolve through the system (so through our DNS hijack), connect, finish a
/// TLS handshake or an HTTP exchange, then look the connection up by its
/// local port while it is still open.
fn trace(host: &str, port: u16, tls: bool, verify: bool, api: Option<&ClashApi>) -> Result<Trace, Failure> {
    let started = Instant::now();
    let address: SocketAddr = (host, port).to_socket_addrs().map_err(|_| Failure::Dns)?.next().ok_or(Failure::Dns)?;
    let dns_ms = started.elapsed().as_millis() as u64;
    let stream = TcpStream::connect_timeout(&address, STEP_TIMEOUT).map_err(|_| Failure::Connect)?;
    stream.set_read_timeout(Some(STEP_TIMEOUT)).ok();
    stream.set_write_timeout(Some(STEP_TIMEOUT)).ok();
    let local_port = stream.local_addr().map(|a| a.port()).unwrap_or(0);
    // With the system stack sing-box completes the TCP handshake locally, so
    // only an answer from the far end proves the path works.
    let _keep_open: Box<dyn std::any::Any> = if tls {
        let connector = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(!verify)
            .build()
            .map_err(|_| Failure::Handshake)?;
        match connector.connect(host, stream) {
            Ok(tls) => Box::new(tls),
            Err(native_tls::HandshakeError::Failure(error)) if is_certificate_error(&error) => return Err(Failure::Certificate),
            Err(_) => return Err(Failure::Handshake),
        }
    } else {
        let mut stream = stream;
        let request = format!("HEAD / HTTP/1.0\r\nHost: {host}\r\n\r\n");
        stream.write_all(request.as_bytes()).map_err(|_| Failure::Handshake)?;
        let mut head = [0u8; 5];
        stream.read_exact(&mut head).map_err(|_| Failure::Handshake)?;
        if &head != b"HTTP/" {
            return Err(Failure::Handshake);
        }
        Box::new(stream)
    };
    let total_ms = started.elapsed().as_millis() as u64;
    let route = api.and_then(|api| {
        let list = fetch_connections(api.port, &api.secret, Duration::from_secs(2)).ok()?;
        list.into_iter().find(|c| c.source_port == local_port && local_port != 0).map(|c| c.route)
    });
    Ok(Trace { dns_ms, total_ms, route })
}

fn failure_text(failure: &Failure) -> &'static str {
    match failure {
        Failure::Dns => "域名解析失败",
        Failure::Connect => "连接失败",
        Failure::Handshake => "连上了，但对方没有正常响应",
        Failure::Certificate => "证书校验失败，线路上可能有拦截，或系统时间不对",
    }
}

fn route_check(id: &str, title: &str, host: &str, expect: Route, api: Option<&ClashApi>) -> CheckItem {
    let wanted = if expect == Route::Proxy { "代理" } else { "直连" };
    match trace(host, 443, true, true, api) {
        Err(failure) => item(id, title, CheckStatus::Fail, format!("{host}：{}", failure_text(&failure))),
        Ok(trace) => match trace.route {
            Some(route) if route == expect => item(
                id,
                title,
                CheckStatus::Ok,
                format!("{host} 走{wanted}，解析 {} 毫秒，连通 {} 毫秒", trace.dns_ms, trace.total_ms),
            ),
            Some(_) => item(
                id,
                title,
                CheckStatus::Fail,
                format!("{host} 应该走{wanted}，实际走了{}", if expect == Route::Proxy { "直连" } else { "代理" }),
            ),
            None => item(id, title, CheckStatus::Warn, format!("{host} 能打开（{} 毫秒），但没能确认走向", trace.total_ms)),
        },
    }
}

/// `oa.corp.cn`, `http://oa.corp.cn:8080/x`, `https://10.1.2.3` → (host, port, tls).
pub fn parse_intranet(input: &str) -> Option<(String, u16, bool)> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let with_scheme = if input.contains("://") { input.to_owned() } else { format!("http://{input}") };
    let url = url::Url::parse(&with_scheme).ok()?;
    let tls = url.scheme() == "https";
    let host = url.host_str()?.trim_start_matches('[').trim_end_matches(']').to_owned();
    let port = url.port_or_known_default()?;
    Some((host, port, tls))
}

fn intranet_check(input: &str, api: Option<&ClashApi>) -> CheckItem {
    const TITLE: &str = "公司内网";
    let Some((host, port, tls)) = parse_intranet(input) else {
        return item("intranet", TITLE, CheckStatus::Fail, "填写的内网地址格式不对");
    };
    // Company certificates are often self-signed; reachability is what matters here.
    match trace(&host, port, tls, false, api) {
        Err(failure) => item("intranet", TITLE, CheckStatus::Fail, format!("填写的内网地址：{}", failure_text(&failure))),
        Ok(trace) => match trace.route {
            Some(Route::Direct) | None => {
                item("intranet", TITLE, CheckStatus::Ok, format!("填写的内网地址可以访问，走直连，{} 毫秒", trace.total_ms))
            }
            Some(Route::Proxy) => item("intranet", TITLE, CheckStatus::Fail, "填写的内网地址走了代理，应该直连"),
        },
    }
}

fn pick_proxy_host(domains: &[String]) -> Option<&str> {
    PREFERRED_PROXY_HOSTS
        .iter()
        .copied()
        .find(|host| domains.iter().any(|d| d == host))
        .or_else(|| domains.first().map(String::as_str))
}

pub fn run(input: &DiagnoseInput) -> Vec<CheckItem> {
    let mut items = Vec::new();
    match input.upstream {
        None => items.push(item("upstream", "代理客户端", CheckStatus::Fail, "还没有选择代理端口")),
        Some(upstream) => {
            let report = probe_upstream(upstream, Duration::from_secs(15));
            let status = if report.ok { CheckStatus::Ok } else { CheckStatus::Fail };
            items.push(item("upstream", "代理客户端", status, report.message));
        }
    }
    if !input.routing {
        let detail = "分流没有开启，跳过走向检查。请先打开首页的分流开关再自检";
        items.push(item("direct", "国内网站直连", CheckStatus::Skip, detail));
        items.push(item("proxy", "选中的网站走代理", CheckStatus::Skip, detail));
    } else {
        items.push(route_check("direct", "国内网站直连", DIRECT_HOST, Route::Direct, input.clash_api));
        match pick_proxy_host(input.proxy_domains) {
            Some(host) => items.push(route_check("proxy", "选中的网站走代理", host, Route::Proxy, input.clash_api)),
            None => items.push(item("proxy", "选中的网站走代理", CheckStatus::Skip, "没有启用任何按网站分流的项目")),
        }
    }
    if let Some(intranet) = input.intranet.filter(|text| !text.trim().is_empty()) {
        if input.routing {
            items.push(intranet_check(intranet, input.clash_api));
        } else {
            items.push(item("intranet", "公司内网", CheckStatus::Skip, "分流没有开启，跳过"));
        }
    }
    items
}

fn status_label(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Ok => "正常",
        CheckStatus::Warn => "注意",
        CheckStatus::Fail => "异常",
        CheckStatus::Skip => "跳过",
    }
}

pub fn render(context: &ReportContext, items: &[CheckItem], env: &[EnvFinding]) -> String {
    let mut lines = vec![
        "ProxyPilot 自检报告".to_owned(),
        format!("版本：{}　时间：{}", context.version, context.time),
        format!("管理员权限：{}", if context.elevated { "有" } else { "没有" }),
        format!("代理客户端：{}", context.upstream),
        format!("分流：{}　DNS：{}", context.routing, context.dns_mode),
        format!(
            "启用的项目：{}",
            if context.profiles.is_empty() { "无".to_owned() } else { context.profiles.join("、") }
        ),
        String::new(),
        "检查结果：".to_owned(),
    ];
    for check in items {
        lines.push(format!("[{}] {}：{}", status_label(check.status), check.title, check.detail));
    }
    if !env.is_empty() {
        lines.push(String::new());
        lines.push("环境检查：".to_owned());
        for finding in env {
            lines.push(format!("- {}：{}", finding.title, finding.detail));
        }
    }
    lines.push(String::new());
    lines.push("（本报告不包含节点、账号或网址信息）".to_owned());
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FindingLevel;

    #[test]
    fn parses_intranet_inputs() {
        assert_eq!(parse_intranet("oa.corp.cn"), Some(("oa.corp.cn".into(), 80, false)));
        assert_eq!(parse_intranet("https://10.1.2.3/x"), Some(("10.1.2.3".into(), 443, true)));
        assert_eq!(parse_intranet(" http://oa.corp.cn:8080/a?b "), Some(("oa.corp.cn".into(), 8080, false)));
        assert_eq!(parse_intranet(""), None);
    }

    #[test]
    fn prefers_well_known_hosts_for_the_proxy_check() {
        let domains = vec!["example.org".to_owned(), "github.com".to_owned()];
        assert_eq!(pick_proxy_host(&domains), Some("github.com"));
        assert_eq!(pick_proxy_host(&["example.org".to_owned()]), Some("example.org"));
        assert_eq!(pick_proxy_host(&[]), None);
    }

    #[test]
    fn routing_off_skips_route_checks_without_touching_the_network() {
        let input = DiagnoseInput { upstream: None, routing: false, clash_api: None, proxy_domains: &[], intranet: Some("oa.corp.cn") };
        let items = run(&input);
        let statuses: Vec<_> = items.iter().map(|i| (i.id.as_str(), i.status)).collect();
        assert_eq!(statuses, vec![
            ("upstream", CheckStatus::Fail),
            ("direct", CheckStatus::Skip),
            ("proxy", CheckStatus::Skip),
            ("intranet", CheckStatus::Skip),
        ]);
    }

    #[test]
    fn report_text_lists_checks_and_findings_without_addresses() {
        let context = ReportContext {
            version: "0.1.0".into(),
            time: "2026-10-27 13:00（北京时间）".into(),
            elevated: true,
            upstream: "Xray 核心（v2rayN 等），本机端口 10808".into(),
            routing: "已开启".into(),
            dns_mode: "加密 DNS".into(),
            profiles: vec!["GitHub".into()],
        };
        let items = vec![item("direct", "国内网站直连", CheckStatus::Ok, "www.baidu.com 走直连")];
        let env = vec![EnvFinding { id: "x".into(), level: FindingLevel::Info, title: "已识别企业 VPN".into(), detail: "aTrust".into() }];
        let text = render(&context, &items, &env);
        assert!(text.contains("[正常] 国内网站直连：www.baidu.com 走直连"));
        assert!(text.contains("- 已识别企业 VPN：aTrust"));
        assert!(text.contains("启用的项目：GitHub"));
    }
}

//! Bounded probes against the upstream. Nothing here sends credentials or HTTP
//! content: the deepest probe stops after a TLS handshake.

use crate::model::{CandidateStatus, ProbeReport, ProbeStage, Upstream, UpstreamKind};
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// Hosts for the end-to-end test. Two, so one site having a bad day is not
/// reported as a broken proxy; both are ones a split-routing user typically
/// sends through the proxy, so a rule-mode client does not answer them direct.
pub const PROBE_TARGETS: &[&str] = &["www.gstatic.com", "www.google.com"];

/// SOCKS5 greeting offering only "no authentication".
///
/// `05 00` means usable. `05 FF` means the server wants a method we did not
/// offer, which in practice is username/password. Anything else is not a
/// SOCKS5 server. Only the local port is contacted.
pub fn socks5_greeting(ip: IpAddr, port: u16, timeout: Duration) -> Option<CandidateStatus> {
    let mut stream = TcpStream::connect_timeout(&SocketAddr::new(ip, port), timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    stream.write_all(&[0x05, 0x01, 0x00]).ok()?;
    let mut reply = [0u8; 2];
    stream.read_exact(&mut reply).ok()?;
    match reply {
        [0x05, 0x00] => Some(CandidateStatus::Usable),
        [0x05, 0xFF] | [0x05, 0x02] => Some(CandidateStatus::NeedsAuth),
        _ => None,
    }
}

/// Is the upstream still there? A greeting for SOCKS5, a plain connect for
/// HTTP (which has nothing safe to say before a request).
pub fn upstream_alive(upstream: &Upstream, timeout: Duration) -> bool {
    let Some(ip) = upstream.ip() else { return false };
    match upstream.kind {
        UpstreamKind::Socks5 => socks5_greeting(ip, upstream.port, timeout) == Some(CandidateStatus::Usable),
        UpstreamKind::Http => TcpStream::connect_timeout(&SocketAddr::new(ip, upstream.port), timeout).is_ok(),
    }
}

#[derive(Debug)]
enum Failure {
    Timeout,
    LocalUnavailable,
    Protocol,
    RemoteRefused,
    Reset,
    TlsFailed,
    TlsCertificate,
}

impl Failure {
    fn code(&self) -> &'static str {
        match self {
            Failure::Timeout => "timeout",
            Failure::LocalUnavailable => "local_unavailable",
            Failure::Protocol => "socks_protocol",
            Failure::RemoteRefused => "remote_refused",
            Failure::Reset => "connection_reset",
            Failure::TlsFailed => "tls_failed",
            Failure::TlsCertificate => "tls_certificate",
        }
    }

    fn message(&self) -> &'static str {
        match self {
            Failure::Timeout => "超时：代理客户端在规定时间内没有连通目标网站，可能是节点不可用或网络很慢",
            Failure::LocalUnavailable => "连不上代理端口：代理客户端没有运行，或端口已变化",
            Failure::Protocol => "端口有响应，但不是可用的代理协议，请确认端口类型（SOCKS5 / HTTP）",
            Failure::RemoteRefused => "代理客户端拒绝了连接：节点不可用，或客户端规则没有放行",
            Failure::Reset => "连接被中断：节点不稳定或被干扰",
            Failure::TlsFailed => "已连到目标，但加密握手失败：线路可能被干扰",
            Failure::TlsCertificate => "证书校验失败：线路上可能有中间人或系统时间不对",
        }
    }
}

struct Budget {
    deadline: Instant,
}

impl Budget {
    fn left(&self) -> Result<Duration, Failure> {
        let now = Instant::now();
        if now >= self.deadline {
            return Err(Failure::Timeout);
        }
        Ok(self.deadline - now)
    }
}

fn io_failure(error: io::Error, local: bool) -> Failure {
    match error.kind() {
        // Windows retries a SYN to a closed loopback port instead of failing
        // at once, so "nothing listening" often arrives as a timeout. Locally
        // either way means the client is not there.
        _ if local => Failure::LocalUnavailable,
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => Failure::Timeout,
        io::ErrorKind::UnexpectedEof => Failure::Protocol,
        _ => Failure::Reset,
    }
}

fn arm(stream: &TcpStream, budget: &Budget) -> Result<(), Failure> {
    let left = budget.left()?;
    stream.set_read_timeout(Some(left)).map_err(|_| Failure::Reset)?;
    stream.set_write_timeout(Some(left)).map_err(|_| Failure::Reset)?;
    Ok(())
}

fn read_exact(stream: &mut TcpStream, budget: &Budget, buf: &mut [u8]) -> Result<(), Failure> {
    arm(stream, budget)?;
    stream.read_exact(buf).map_err(|error| io_failure(error, false))
}

fn socks_connect(stream: &mut TcpStream, budget: &Budget, host: &str, laps: &mut Laps) -> Result<(), Failure> {
    arm(stream, budget)?;
    stream.write_all(&[0x05, 0x01, 0x00]).map_err(|error| io_failure(error, false))?;
    let mut greeting = [0u8; 2];
    read_exact(stream, budget, &mut greeting)?;
    if greeting != [0x05, 0x00] {
        return Err(Failure::Protocol);
    }
    laps.lap("socks_greeting");
    let name = host.as_bytes();
    let mut request = vec![0x05, 0x01, 0x00, 0x03, name.len() as u8];
    request.extend_from_slice(name);
    request.extend_from_slice(&443u16.to_be_bytes());
    arm(stream, budget)?;
    stream.write_all(&request).map_err(|error| io_failure(error, false))?;
    let mut head = [0u8; 4];
    read_exact(stream, budget, &mut head)?;
    if head[0] != 0x05 || head[2] != 0x00 {
        return Err(Failure::Protocol);
    }
    if head[1] != 0x00 {
        return Err(Failure::RemoteRefused);
    }
    let address_len = match head[3] {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut len = [0u8; 1];
            read_exact(stream, budget, &mut len)?;
            len[0] as usize
        }
        _ => return Err(Failure::Protocol),
    };
    let mut rest = vec![0u8; address_len + 2];
    read_exact(stream, budget, &mut rest)?;
    // The first real round trip to the origin: everything before it is
    // answered locally by the client, so this lap carries the WAN cost.
    laps.lap("socks_connect");
    Ok(())
}

fn http_connect(stream: &mut TcpStream, budget: &Budget, host: &str, laps: &mut Laps) -> Result<(), Failure> {
    arm(stream, budget)?;
    let request = format!("CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|error| io_failure(error, false))?;
    let mut response = Vec::new();
    let mut byte = [0u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        if response.len() > 8192 {
            return Err(Failure::Protocol);
        }
        read_exact(stream, budget, &mut byte)?;
        response.push(byte[0]);
    }
    let status_line = String::from_utf8_lossy(&response);
    let status = status_line.split_whitespace().nth(1).unwrap_or("");
    if !status_line.starts_with("HTTP/1.") {
        return Err(Failure::Protocol);
    }
    if status != "200" {
        return Err(Failure::RemoteRefused);
    }
    laps.lap("socks_connect");
    Ok(())
}

struct Laps {
    started: Instant,
    mark: Instant,
    stages: Vec<ProbeStage>,
}

impl Laps {
    fn lap(&mut self, name: &str) {
        let now = Instant::now();
        self.stages.push(ProbeStage { name: name.into(), ms: (now - self.mark).as_millis() as u64 });
        self.mark = now;
    }
}

/// Schannel reports certificate problems through these HRESULTs; matching the
/// code rather than the text keeps this working on non-English Windows.
pub(crate) fn is_certificate_error(error: &native_tls::Error) -> bool {
    const CODES: &[u32] = &[
        0x8009_0325, // SEC_E_UNTRUSTED_ROOT
        0x8009_0328, // SEC_E_CERT_EXPIRED
        0x8009_0322, // SEC_E_WRONG_PRINCIPAL
        0x800B_0101, // CERT_E_EXPIRED
        0x800B_0109, // CERT_E_UNTRUSTEDROOT
        0x800B_010F, // CERT_E_CN_NO_MATCH
        0x800B_0111, // CERT_E_UNTRUSTEDTESTROOT
    ];
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if let Some(io) = current.downcast_ref::<io::Error>()
            && let Some(code) = io.raw_os_error()
                && CODES.contains(&(code as u32)) {
                    return true;
                }
        source = current.source();
    }
    let text = error.to_string().to_lowercase();
    text.contains("certificate") || text.contains("证书")
}

fn probe_once(upstream: &Upstream, host: &str, timeout: Duration) -> (Result<(), Failure>, Laps) {
    let started = Instant::now();
    let mut laps = Laps { started, mark: started, stages: Vec::new() };
    let budget = Budget { deadline: started + timeout };
    let result = (|| {
        let ip = upstream.ip().ok_or(Failure::LocalUnavailable)?;
        let mut stream = TcpStream::connect_timeout(&SocketAddr::new(ip, upstream.port), budget.left()?)
            .map_err(|error| io_failure(error, true))?;
        laps.lap("local");
        match upstream.kind {
            UpstreamKind::Socks5 => socks_connect(&mut stream, &budget, host, &mut laps)?,
            UpstreamKind::Http => http_connect(&mut stream, &budget, host, &mut laps)?,
        }
        arm(&stream, &budget)?;
        let connector = native_tls::TlsConnector::new().map_err(|_| Failure::TlsFailed)?;
        match connector.connect(host, stream) {
            Ok(_tls) => {
                laps.lap("tls");
                Ok(())
            }
            Err(native_tls::HandshakeError::Failure(error)) => {
                if budget.left().is_err() {
                    Err(Failure::Timeout)
                } else if is_certificate_error(&error) {
                    Err(Failure::TlsCertificate)
                } else {
                    Err(Failure::TlsFailed)
                }
            }
            Err(native_tls::HandshakeError::WouldBlock(_)) => Err(Failure::Timeout),
        }
    })();
    (result, laps)
}

/// End-to-end test through the upstream to a real site. The timeout is a
/// ceiling for giving up, not a health threshold: a cross-border path can
/// legitimately need several seconds for a cold handshake.
pub fn probe_upstream(upstream: &Upstream, timeout: Duration) -> ProbeReport {
    let mut last = None;
    for host in PROBE_TARGETS {
        let (result, laps) = probe_once(upstream, host, timeout);
        let elapsed_ms = laps.started.elapsed().as_millis() as u64;
        match result {
            Ok(()) => {
                return ProbeReport {
                    ok: true,
                    code: "ok".into(),
                    message: format!("代理可用，{} 毫秒连通 {host}", elapsed_ms),
                    target: (*host).into(),
                    elapsed_ms,
                    stages: laps.stages,
                };
            }
            // A dead local port fails the same way for every target.
            Err(Failure::LocalUnavailable) => {
                return report(Failure::LocalUnavailable, host, elapsed_ms, laps.stages);
            }
            Err(failure) => last = Some(report(failure, host, elapsed_ms, laps.stages)),
        }
    }
    last.expect("PROBE_TARGETS is not empty")
}

fn report(failure: Failure, host: &str, elapsed_ms: u64, stages: Vec<ProbeStage>) -> ProbeReport {
    ProbeReport {
        ok: false,
        code: failure.code().into(),
        message: failure.message().into(),
        target: host.into(),
        elapsed_ms,
        stages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(reply: &'static [u8]) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut greeting = [0u8; 3];
                let _ = stream.read_exact(&mut greeting);
                let _ = stream.write_all(reply);
            }
        });
        port
    }

    #[test]
    fn greeting_classifies_replies() {
        let localhost = IpAddr::from([127, 0, 0, 1]);
        let timeout = Duration::from_millis(500);
        assert_eq!(socks5_greeting(localhost, serve_once(&[5, 0]), timeout), Some(CandidateStatus::Usable));
        assert_eq!(socks5_greeting(localhost, serve_once(&[5, 0xFF]), timeout), Some(CandidateStatus::NeedsAuth));
        assert_eq!(socks5_greeting(localhost, serve_once(b"HTTP/1.1 400 Bad Request\r\n"), timeout), None);
    }

    #[test]
    fn closed_port_is_reported_as_local_unavailable() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let upstream = Upstream { kind: UpstreamKind::Socks5, host: "127.0.0.1".into(), port };
        let report = probe_upstream(&upstream, Duration::from_millis(500));
        assert!(!report.ok);
        assert_eq!(report.code, "local_unavailable");
        assert!(!upstream_alive(&upstream, Duration::from_millis(200)));
    }

    #[test]
    fn socks_refusal_is_reported_per_target() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for stream in listener.incoming().take(PROBE_TARGETS.len()) {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 3];
                stream.read_exact(&mut buf).unwrap();
                stream.write_all(&[5, 0]).unwrap();
                let mut head = [0u8; 5];
                stream.read_exact(&mut head).unwrap();
                let mut rest = vec![0u8; head[4] as usize + 2];
                stream.read_exact(&mut rest).unwrap();
                stream.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
            }
        });
        let upstream = Upstream { kind: UpstreamKind::Socks5, host: "127.0.0.1".into(), port };
        let report = probe_upstream(&upstream, Duration::from_secs(2));
        assert_eq!(report.code, "remote_refused");
        assert_eq!(report.target, *PROBE_TARGETS.last().unwrap());
        assert_eq!(report.stages[0].name, "local");
        assert_eq!(report.stages[1].name, "socks_greeting");
    }
}

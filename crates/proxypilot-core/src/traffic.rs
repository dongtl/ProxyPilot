//! Recent connections, read from sing-box's Clash API and kept in memory only,
//! and the direct ones that look blocked.

use crate::domain::site_of;
use crate::model::{ConnectionRow, Route};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub id: String,
    /// Local port of the connecting socket; lets the self-check find its own connection.
    pub source_port: u16,
    pub process: String,
    pub host: String,
    pub port: u16,
    pub route: Route,
    /// "tcp" or "udp".
    pub network: String,
    /// Bytes received so far.
    pub download: u64,
    /// When sing-box accepted it, in milliseconds since 1970.
    pub start_ms: Option<u64>,
    /// Direct because no rule matched, rather than kept direct on purpose
    /// (intranet, the proxy client itself).
    pub unmatched: bool,
}

#[derive(Deserialize)]
struct Snapshot {
    #[serde(default)]
    connections: Option<Vec<RawConnection>>,
}

#[derive(Deserialize)]
struct RawConnection {
    #[serde(default)]
    id: String,
    #[serde(default)]
    metadata: HashMap<String, Value>,
    #[serde(default)]
    chains: Vec<String>,
    #[serde(default)]
    rule: String,
    #[serde(default)]
    download: Value,
    #[serde(default)]
    start: String,
}

fn field(metadata: &HashMap<String, Value>, key: &str) -> String {
    match metadata.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

/// The program behind a process path, empty when unknown: its file name, or
/// on macOS the `.app` it runs from, so a row for "Code Helper" offers to
/// route Visual Studio Code rather than one helper process.
fn process_name(path: &str) -> String {
    crate::platform::program_of(path)
}

pub fn parse_connections(body: &str) -> Result<Vec<Connection>, String> {
    let snapshot: Snapshot = serde_json::from_str(body).map_err(|error| format!("连接列表格式不对：{error}"))?;
    let mut out = Vec::new();
    for raw in snapshot.connections.unwrap_or_default() {
        let port: u16 = field(&raw.metadata, "destinationPort").parse().unwrap_or(0);
        let host = match field(&raw.metadata, "host") {
            host if !host.is_empty() => host,
            _ => field(&raw.metadata, "destinationIP"),
        };
        // Hijacked DNS queries show up as port-53 entries; they are our own plumbing.
        if host.is_empty() || port == 53 || raw.id.is_empty() {
            continue;
        }
        let route = if raw.chains.iter().any(|chain| chain == "proxy") { Route::Proxy } else { Route::Direct };
        out.push(Connection {
            id: raw.id,
            source_port: field(&raw.metadata, "sourcePort").parse().unwrap_or(0),
            process: process_name(&field(&raw.metadata, "processPath")),
            host,
            port,
            route,
            network: field(&raw.metadata, "network"),
            download: raw.download.as_u64().unwrap_or(0),
            start_ms: parse_rfc3339_ms(&raw.start),
            // sing-box names the rule that matched; "final" when none did.
            unmatched: raw.rule == "final",
        });
    }
    Ok(out)
}

/// `2026-11-03T15:02:42.1234567+08:00` → milliseconds since 1970. sing-box's
/// own format only, so no date crate for one field.
fn parse_rfc3339_ms(text: &str) -> Option<u64> {
    let (date, rest) = text.split_once('T')?;
    let mut parts = date.split('-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    let zone_at = rest.find(['Z', 'z', '+', '-'])?;
    let (clock, zone) = rest.split_at(zone_at);
    let mut clock_parts = clock.split(':');
    let hour: i64 = clock_parts.next()?.parse().ok()?;
    let minute: i64 = clock_parts.next()?.parse().ok()?;
    let seconds = clock_parts.next()?;
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let second: i64 = whole.parse().ok()?;
    let millis: i64 = format!("{fraction:0<3}").get(..3)?.parse().ok()?;
    let offset_minutes = match zone.as_bytes().first()? {
        b'Z' | b'z' => 0,
        sign => {
            let (h, m) = zone[1..].split_once(':')?;
            let minutes = h.parse::<i64>().ok()? * 60 + m.parse::<i64>().ok()?;
            if *sign == b'-' { -minutes } else { minutes }
        }
    };
    let days = days_from_civil(year, month, day)?;
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second - offset_minutes * 60;
    u64::try_from(seconds * 1_000 + millis).ok()
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

/// One snapshot of `/connections`. HTTP/1.0 so the reply is never chunked and
/// ends when the server closes, which keeps this free of an HTTP client crate.
pub fn fetch_connections(port: u16, secret: &str, timeout: Duration) -> Result<Vec<Connection>, String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, timeout).map_err(|error| format!("连不上 sing-box：{error}"))?;
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();
    let request = format!("GET /connections HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {secret}\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    stream.take(16 * 1024 * 1024).read_to_end(&mut response).map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&response);
    let (head, body) = text.split_once("\r\n\r\n").ok_or("sing-box 返回了不完整的响应")?;
    let status = head.split_whitespace().nth(1).unwrap_or("");
    if status != "200" {
        return Err(format!("sing-box 返回 {status}"));
    }
    parse_connections(body)
}

/// A direct HTTPS connection that has received nothing for this long is
/// blocked: a server answers a TLS hello within one round trip. sing-box
/// gives up dialling after about five seconds and connections are read every
/// two, so a blocked one is still seen in time.
const STALL_AFTER_MS: u64 = 2_500;
/// A stalled connection is put down to the proxied page its program opened
/// shortly before (or just after, within one poll).
const PARENT_BEFORE_MS: u64 = 20_000;
const PARENT_AFTER_MS: u64 = 3_000;
/// Proxied connections are remembered this long after they were last seen.
const PROXIED_KEEP_MS: u64 = 60_000;
const PROXIED_LIMIT: usize = 512;

/// A direct connection that looks blocked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stall {
    pub process: String,
    pub host: String,
    /// The proxied site the same program had just opened, which the blocked
    /// one most likely belongs to. None when there was none.
    pub parent_host: Option<String>,
}

struct Seen {
    first_ms: u64,
    reported: bool,
}

struct Proxied {
    id: String,
    process: String,
    host: String,
    start_ms: u64,
    last_ms: u64,
}

type Key = (String, String, u16, Route);

/// Deduplicated summary across polls: a connection open for a minute is seen
/// thirty times by the poller but counted once.
pub struct TrafficLog {
    rows: HashMap<Key, ConnectionRow>,
    seen: HashMap<String, Seen>,
    seen_order: VecDeque<String>,
    proxied: VecDeque<Proxied>,
    row_limit: usize,
    seen_limit: usize,
}

impl Default for TrafficLog {
    fn default() -> Self {
        TrafficLog::new(300, 4096)
    }
}

impl TrafficLog {
    pub fn new(row_limit: usize, seen_limit: usize) -> Self {
        TrafficLog {
            rows: HashMap::new(),
            seen: HashMap::new(),
            seen_order: VecDeque::new(),
            proxied: VecDeque::new(),
            row_limit,
            seen_limit,
        }
    }

    /// Add one snapshot. Returns the direct connections newly found stalled,
    /// each reported once.
    pub fn ingest(&mut self, connections: &[Connection], now_ms: u64) -> Vec<Stall> {
        // Proxied ones first, so a page and its blocked parts in the same
        // snapshot are linked.
        for connection in connections.iter().filter(|connection| connection.route == Route::Proxy) {
            self.note_proxied(connection, now_ms);
        }
        self.proxied.retain(|proxied| now_ms.saturating_sub(proxied.last_ms) <= PROXIED_KEEP_MS);
        while self.proxied.len() > PROXIED_LIMIT {
            self.proxied.pop_front();
        }
        let mut stalls = Vec::new();
        for connection in connections {
            let key = (connection.process.clone(), connection.host.clone(), connection.port, connection.route);
            let fresh = !self.seen.contains_key(&connection.id);
            if fresh {
                self.seen.insert(connection.id.clone(), Seen { first_ms: now_ms, reported: false });
                self.seen_order.push_back(connection.id.clone());
            }
            let first_ms = self.seen.get(&connection.id).map_or(now_ms, |seen| seen.first_ms);
            let start_ms = connection.start_ms.unwrap_or(first_ms);
            let stalled = looks_blocked(connection, start_ms, now_ms);
            if stalled && let Some(seen) = self.seen.get_mut(&connection.id).filter(|seen| !seen.reported) {
                seen.reported = true;
                stalls.push(Stall {
                    process: connection.process.clone(),
                    host: connection.host.clone(),
                    parent_host: self.parent_of(&connection.process, start_ms),
                });
            }
            let row = self.rows.entry(key).or_insert_with(|| ConnectionRow {
                process: connection.process.clone(),
                host: connection.host.clone(),
                site: site_of(&connection.host),
                port: connection.port,
                route: connection.route,
                count: 0,
                last_seen: now_ms,
                stalled: false,
            });
            if fresh {
                row.count += 1;
            }
            row.last_seen = now_ms;
            // A later connection that got an answer clears the mark.
            if stalled {
                row.stalled = true;
            } else if connection.download > 0 {
                row.stalled = false;
            }
        }
        while self.seen_order.len() > self.seen_limit {
            if let Some(old) = self.seen_order.pop_front() {
                self.seen.remove(&old);
            }
        }
        while self.rows.len() > self.row_limit {
            let oldest = self.rows.iter().min_by_key(|(_, row)| row.last_seen).map(|(key, _)| key.clone());
            match oldest {
                Some(key) => self.rows.remove(&key),
                None => break,
            };
        }
        stalls
    }

    fn note_proxied(&mut self, connection: &Connection, now_ms: u64) {
        if let Some(known) = self.proxied.iter_mut().find(|proxied| proxied.id == connection.id) {
            known.last_ms = now_ms;
            return;
        }
        let first_ms = self.seen.get(&connection.id).map_or(now_ms, |seen| seen.first_ms);
        self.proxied.push_back(Proxied {
            id: connection.id.clone(),
            process: connection.process.clone(),
            host: connection.host.clone(),
            start_ms: connection.start_ms.unwrap_or(first_ms),
            last_ms: now_ms,
        });
    }

    /// The proxied site `process` opened most recently around `start_ms`.
    /// Pages open their parts within seconds of the page itself.
    fn parent_of(&self, process: &str, start_ms: u64) -> Option<String> {
        if process.is_empty() {
            return None;
        }
        self.proxied
            .iter()
            .filter(|proxied| {
                proxied.process == process
                    && proxied.start_ms <= start_ms + PARENT_AFTER_MS
                    && start_ms.saturating_sub(proxied.start_ms) <= PARENT_BEFORE_MS
            })
            .max_by_key(|proxied| proxied.start_ms)
            .map(|proxied| proxied.host.clone())
    }

    /// Newest first.
    pub fn rows(&self) -> Vec<ConnectionRow> {
        let mut rows: Vec<ConnectionRow> = self.rows.values().cloned().collect();
        rows.sort_by(|a, b| b.last_seen.cmp(&a.last_seen).then_with(|| a.host.cmp(&b.host)));
        rows
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.seen.clear();
        self.seen_order.clear();
        self.proxied.clear();
    }
}

/// Direct for want of a rule, HTTPS over TCP to a name (an IP has no site to
/// suggest), and not a byte back after `STALL_AFTER_MS`.
fn looks_blocked(connection: &Connection, start_ms: u64, now_ms: u64) -> bool {
    connection.route == Route::Direct
        && connection.unmatched
        && connection.port == 443
        && connection.network != "udp"
        && connection.download == 0
        && connection.host.parse::<IpAddr>().is_err()
        && now_ms.saturating_sub(start_ms) >= STALL_AFTER_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"downloadTotal":1,"uploadTotal":2,"connections":[
        {"id":"a","metadata":{"network":"tcp","sourcePort":"50123","destinationIP":"104.18.1.1","destinationPort":"443",
          "host":"chatgpt.com","processPath":"C:\\Program Files\\ChatGPT\\ChatGPT.exe"},"chains":["proxy"]},
        {"id":"b","metadata":{"destinationIP":"110.242.68.66","destinationPort":443,"host":"",
          "processPath":"C:\\Windows\\explorer.exe (user)"},"chains":["direct"]},
        {"id":"c","metadata":{"destinationIP":"172.19.0.2","destinationPort":"53","host":""},"chains":[]},
        {"id":"d","metadata":{"destinationIP":"","destinationPort":"443","host":""},"chains":["direct"]},
        {"id":"e","metadata":{"destinationIP":"1.2.3.4","destinationPort":"443","host":"api.example.com"},"chains":["direct"]}
    ]}"#;

    #[test]
    fn parses_clash_api_snapshot_and_skips_dns_plumbing() {
        let list = parse_connections(SAMPLE).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0], Connection {
            id: "a".into(), source_port: 50123, process: "ChatGPT.exe".into(), host: "chatgpt.com".into(), port: 443,
            route: Route::Proxy, network: "tcp".into(), download: 0, start_ms: None, unmatched: false,
        });
        assert_eq!(list[1].process, "explorer.exe");
        assert_eq!(list[1].host, "110.242.68.66");
        assert_eq!(list[1].route, Route::Direct);
        // No process path: left empty so the UI neither shows nor offers a fake name.
        assert_eq!(list[2].process, "");
        assert!(parse_connections(r#"{"connections":null}"#).unwrap().is_empty());
    }

    #[test]
    fn counts_each_connection_once_across_polls() {
        let list = parse_connections(SAMPLE).unwrap();
        let mut log = TrafficLog::default();
        log.ingest(&list, 1_000);
        log.ingest(&list, 3_000);
        let rows = log.rows();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row.count == 1 && row.last_seen == 3_000));
        let mut again = list[0].clone();
        again.id = "a2".into();
        log.ingest(&[again], 5_000);
        let rows = log.rows();
        assert_eq!(rows[0].host, "chatgpt.com");
        assert_eq!(rows[0].count, 2);
    }

    #[test]
    fn rows_carry_the_site_to_offer() {
        let mut log = TrafficLog::default();
        log.ingest(&parse_connections(SAMPLE).unwrap(), 1_000);
        let site = |host: &str| log.rows().into_iter().find(|row| row.host == host).unwrap().site;
        assert_eq!(site("api.example.com").as_deref(), Some("example.com"));
        assert_eq!(site("chatgpt.com"), None);
        assert_eq!(site("110.242.68.66"), None);
    }

    #[test]
    fn evicts_the_stalest_rows() {
        let mut log = TrafficLog::new(2, 10);
        for (index, host) in ["a.com", "b.com", "c.com"].iter().enumerate() {
            log.ingest(&[conn(host, "x.exe", host, Route::Direct, 0, 0)], index as u64);
        }
        let hosts: Vec<String> = log.rows().into_iter().map(|row| row.host).collect();
        assert_eq!(hosts, vec!["c.com", "b.com"]);
    }

    /// An HTTPS connection over TCP; direct ones are direct for want of a rule.
    fn conn(id: &str, process: &str, host: &str, route: Route, start_ms: u64, download: u64) -> Connection {
        Connection {
            id: id.into(),
            source_port: 0,
            process: process.into(),
            host: host.into(),
            port: 443,
            route,
            network: "tcp".into(),
            download,
            start_ms: Some(start_ms),
            unmatched: route == Route::Direct,
        }
    }

    fn row_stalled(log: &TrafficLog, host: &str) -> bool {
        log.rows().into_iter().any(|row| row.host == host && row.stalled)
    }

    #[test]
    fn a_direct_connection_that_gets_nothing_back_is_reported_once_with_its_page() {
        let page = conn("p", "chrome.exe", "x.com", Route::Proxy, 10_000, 9_000);
        let part = conn("s", "chrome.exe", "abs.twimg.com", Route::Direct, 11_000, 0);
        let mut log = TrafficLog::default();
        // Too early to tell.
        assert!(log.ingest(&[page.clone(), part.clone()], 12_000).is_empty());
        let stalls = log.ingest(&[page.clone(), part.clone()], 14_000);
        assert_eq!(stalls, vec![Stall {
            process: "chrome.exe".into(),
            host: "abs.twimg.com".into(),
            parent_host: Some("x.com".into()),
        }]);
        assert!(row_stalled(&log, "abs.twimg.com"));
        // Reported once, however long it hangs on.
        assert!(log.ingest(std::slice::from_ref(&part), 16_000).is_empty());
        // A later connection to the same site that got an answer clears the mark.
        log.ingest(&[conn("s2", "chrome.exe", "abs.twimg.com", Route::Direct, 20_000, 4_000)], 21_000);
        assert!(!row_stalled(&log, "abs.twimg.com"));
    }

    #[test]
    fn only_unmatched_https_to_a_name_counts_and_the_page_must_be_the_same_programs() {
        let mut kept_direct = conn("k", "chrome.exe", "intranet.corp", Route::Direct, 0, 0);
        kept_direct.unmatched = false;
        let mut udp = conn("u", "chrome.exe", "quic.example.com", Route::Direct, 0, 0);
        udp.network = "udp".into();
        let ip = conn("i", "chrome.exe", "203.0.113.5", Route::Direct, 0, 0);
        let mut plain = conn("h", "chrome.exe", "plain.example.com", Route::Direct, 0, 0);
        plain.port = 80;
        let answered = conn("a", "chrome.exe", "fine.example.com", Route::Direct, 0, 10);
        let mut log = TrafficLog::default();
        assert!(log.ingest(&[kept_direct, udp, ip, plain, answered], 10_000).is_empty());

        // Another program's page, and a page opened long before, are not the parent.
        let other = conn("o", "Code.exe", "github.com", Route::Proxy, 30_000, 1);
        let old = conn("old", "chrome.exe", "x.com", Route::Proxy, 1_000, 1);
        let blocked = conn("b", "chrome.exe", "blocked.example.com", Route::Direct, 30_000, 0);
        let stalls = log.ingest(&[other, old, blocked], 34_000);
        assert_eq!(stalls.len(), 1);
        assert_eq!(stalls[0].parent_host, None);
    }

    #[test]
    fn reads_sing_box_start_times() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_ms("2024-02-29T12:00:00Z"), Some(1_709_208_000_000));
        // Seven fraction digits and an offset, as sing-box writes them.
        assert_eq!(parse_rfc3339_ms("2024-02-29T20:00:00.1234567+08:00"), Some(1_709_208_000_123));
        assert_eq!(parse_rfc3339_ms("2024-02-29T07:30:00.5-04:30"), Some(1_709_208_000_500));
        assert_eq!(parse_rfc3339_ms(""), None);
        assert_eq!(parse_rfc3339_ms("2024-13-01T00:00:00Z"), None);
    }

    #[test]
    fn parses_bytes_start_and_rule() {
        let body = r#"{"connections":[{"id":"z","metadata":{"network":"tcp","destinationPort":"443","host":"abs.twimg.com",
            "processPath":"C:\\Apps\\chrome.exe"},"chains":["direct"],"rule":"final","download":0,"upload":0,
            "start":"2024-02-29T20:00:00.1234567+08:00"}]}"#;
        let list = parse_connections(body).unwrap();
        assert!(list[0].unmatched);
        assert_eq!(list[0].start_ms, Some(1_709_208_000_123));
        assert_eq!(list[0].network, "tcp");
    }
}

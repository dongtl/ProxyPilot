//! Recent connections, read from sing-box's Clash API and kept in memory only.

use crate::model::{ConnectionRow, Route};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
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
}

fn field(metadata: &HashMap<String, Value>, key: &str) -> String {
    match metadata.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

/// File name of a process path. sing-box may append " (user)" on some systems.
fn process_name(path: &str) -> String {
    let path = path.split(" (").next().unwrap_or(path).trim();
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    if name.is_empty() { "未知程序".into() } else { name.to_owned() }
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
        });
    }
    Ok(out)
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

type Key = (String, String, u16, Route);

/// Deduplicated summary across polls: a connection open for a minute is seen
/// thirty times by the poller but counted once.
pub struct TrafficLog {
    rows: HashMap<Key, ConnectionRow>,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
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
        TrafficLog { rows: HashMap::new(), seen: HashSet::new(), seen_order: VecDeque::new(), row_limit, seen_limit }
    }

    pub fn ingest(&mut self, connections: &[Connection], now_ms: u64) {
        for connection in connections {
            let key = (connection.process.clone(), connection.host.clone(), connection.port, connection.route);
            let fresh = self.seen.insert(connection.id.clone());
            if fresh {
                self.seen_order.push_back(connection.id.clone());
                while self.seen_order.len() > self.seen_limit {
                    if let Some(old) = self.seen_order.pop_front() {
                        self.seen.remove(&old);
                    }
                }
            }
            let row = self.rows.entry(key).or_insert_with(|| ConnectionRow {
                process: connection.process.clone(),
                host: connection.host.clone(),
                port: connection.port,
                route: connection.route,
                count: 0,
                last_seen: now_ms,
            });
            if fresh {
                row.count += 1;
            }
            row.last_seen = now_ms;
        }
        while self.rows.len() > self.row_limit {
            let oldest = self.rows.iter().min_by_key(|(_, row)| row.last_seen).map(|(key, _)| key.clone());
            match oldest {
                Some(key) => self.rows.remove(&key),
                None => break,
            };
        }
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
    }
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
        {"id":"d","metadata":{"destinationIP":"","destinationPort":"443","host":""},"chains":["direct"]}
    ]}"#;

    #[test]
    fn parses_clash_api_snapshot_and_skips_dns_plumbing() {
        let list = parse_connections(SAMPLE).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], Connection {
            id: "a".into(), source_port: 50123, process: "ChatGPT.exe".into(), host: "chatgpt.com".into(), port: 443, route: Route::Proxy
        });
        assert_eq!(list[1].process, "explorer.exe");
        assert_eq!(list[1].host, "110.242.68.66");
        assert_eq!(list[1].route, Route::Direct);
        assert!(parse_connections(r#"{"connections":null}"#).unwrap().is_empty());
    }

    #[test]
    fn counts_each_connection_once_across_polls() {
        let list = parse_connections(SAMPLE).unwrap();
        let mut log = TrafficLog::default();
        log.ingest(&list, 1_000);
        log.ingest(&list, 3_000);
        let rows = log.rows();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.count == 1 && row.last_seen == 3_000));
        let mut again = list[0].clone();
        again.id = "a2".into();
        log.ingest(&[again], 5_000);
        let rows = log.rows();
        assert_eq!(rows[0].host, "chatgpt.com");
        assert_eq!(rows[0].count, 2);
    }

    #[test]
    fn evicts_the_stalest_rows() {
        let mut log = TrafficLog::new(2, 10);
        for (index, host) in ["a.com", "b.com", "c.com"].iter().enumerate() {
            let connection = Connection { id: host.to_string(), source_port: 0, process: "x.exe".into(), host: host.to_string(), port: 443, route: Route::Direct };
            log.ingest(&[connection], index as u64);
        }
        let hosts: Vec<String> = log.rows().into_iter().map(|row| row.host).collect();
        assert_eq!(hosts, vec!["c.com", "b.com"]);
    }
}

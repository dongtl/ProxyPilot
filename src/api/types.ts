// IPC contract between the Vue frontend and the Rust backend.
// Rust mirrors every type with `#[serde(rename_all = "camelCase")]`; change both sides together.

export type UpstreamKind = "socks5" | "http";

export interface Upstream {
  kind: UpstreamKind;
  host: string;
  port: number;
}

/** A local port found by `detect_upstreams`. */
export interface Candidate {
  port: number;
  host: string;
  /** Executable name owning the port, e.g. "xray.exe"; null when it could not be read. */
  process: string | null;
  pid: number | null;
  /** Human label derived from the process, e.g. "Xray 核心（v2rayN 等）". */
  client: string;
  /** usable: SOCKS5 without auth; needsAuth: SOCKS5 asking for username/password (unsupported). */
  status: "usable" | "needsAuth";
  /** True when the owning process is a known proxy client, used for ordering and the badge. */
  known: boolean;
}

export type CoreStatus = "stopped" | "starting" | "running" | "failed";
export type UpstreamStatus = "unknown" | "ok" | "unreachable";
export type DnsMode = "encrypted" | "system";
export type LogLevel = "warn" | "info" | "debug";

export interface Profile {
  id: string;
  name: string;
  /** Short description shown under the name; empty for custom entries. */
  description: string;
  processes: string[];
  domains: string[];
  enabled: boolean;
  source: "preset" | "custom";
}

export interface EnvFinding {
  /** Stable key, e.g. "not_elevated", "system_proxy", "foreign_tun", "corporate_vpn", "upstream_down", "core_missing". */
  id: string;
  level: "error" | "warning" | "info";
  title: string;
  detail: string;
}

export interface AppState {
  /** The user's switch; the core may still be starting or failed. */
  routingEnabled: boolean;
  coreStatus: CoreStatus;
  /** Last core error, shown when coreStatus is "failed". */
  coreMessage: string;
  upstream: Upstream | null;
  /** Label of the client owning the upstream port, when known. */
  upstreamClient: string | null;
  upstreamStatus: UpstreamStatus;
  profiles: Profile[];
  /** User additions to the permanent direct list (process names). */
  directProcesses: string[];
  /** User additions to the intranet suffix list. */
  intranetDomains: string[];
  dnsMode: DnsMode;
  logLevel: LogLevel;
  autostart: boolean;
  elevated: boolean;
  env: EnvFinding[];
  dataDir: string;
  version: string;
}

export interface ProbeStage {
  name: "local" | "socks_greeting" | "socks_connect" | "tls";
  ms: number;
}

export interface ProbeReport {
  ok: boolean;
  /** "ok" | "timeout" | "local_unavailable" | "socks_protocol" | "remote_refused" | "connection_reset" | "tls_failed" | "tls_certificate" */
  code: string;
  /** Human readable summary in Chinese. */
  message: string;
  target: string;
  elapsedMs: number;
  stages: ProbeStage[];
}

export interface RunningProcess {
  name: string;
  path: string | null;
}

export interface ConnectionRow {
  process: string;
  /** Domain when sniffed or resolved, otherwise the IP. */
  host: string;
  port: number;
  route: "proxy" | "direct";
  count: number;
  /** Unix epoch milliseconds. */
  lastSeen: number;
}

export type CheckStatus = "ok" | "warn" | "fail" | "skip";

/** One line of the self-check. */
export interface CheckItem {
  /** "upstream" | "direct" | "proxy" | "intranet" */
  id: string;
  title: string;
  status: CheckStatus;
  detail: string;
}

export interface DiagnosticReport {
  items: CheckItem[];
  /** Plain text for copying; contains no node details, IPs or the intranet address. */
  text: string;
}

/** Fields omitted from the patch are left unchanged. */
export interface SettingsPatch {
  directProcesses?: string[];
  intranetDomains?: string[];
  dnsMode?: DnsMode;
  logLevel?: LogLevel;
  autostart?: boolean;
}

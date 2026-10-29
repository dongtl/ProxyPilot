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
  /** macOS: the owner was hidden (a root-run core) and `client` is the one proxy client running. */
  guessed: boolean;
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
  /** The label is a guess: see Candidate.guessed. */
  upstreamClientGuessed: boolean;
  upstreamStatus: UpstreamStatus;
  profiles: Profile[];
  /** User additions to the permanent direct list (process names). */
  directProcesses: string[];
  /** User additions to the intranet suffix list. */
  intranetDomains: string[];
  dnsMode: DnsMode;
  logLevel: LogLevel;
  autostart: boolean;
  /** Look for a new release about once a day. */
  checkUpdates: boolean;
  /** A newer release found by the last check, waiting for the user; null when none. */
  update: UpdateInfo | null;
  /** Whether a TUN can be created: administrator rights on Windows, a ready background helper on macOS. */
  elevated: boolean;
  /** Decides wording and macOS-only settings. */
  platform: Platform;
  /** macOS: whether the background helper is installed. Always false on Windows. */
  helperInstalled: boolean;
  env: EnvFinding[];
  dataDir: string;
  version: string;
}

export type Platform = "windows" | "macos";

export interface UpdateInfo {
  version: string;
  /** Release notes as written on the release page; may be empty. */
  notes: string;
}

/** Pushed as "update-progress" while the installer downloads. */
export interface UpdateProgress {
  downloaded: number;
  /** Bytes; null when the server did not say. */
  total: number | null;
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
  /** Executable name; empty when sing-box could not tell. */
  process: string;
  /** Domain when sniffed or resolved, otherwise the IP. */
  host: string;
  /** The wider site `host` belongs to ("openai.com" for "chat.openai.com"); null when that adds nothing or is unsafe. */
  site: string | null;
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
  checkUpdates?: boolean;
}

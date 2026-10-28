import type { UpdateProgress, Upstream } from "./api";
import { t } from "./i18n";

/** "127.0.0.1:10808", with IPv6 hosts bracketed. */
export function formatEndpoint(host: string, port: number): string {
  return host.includes(":") ? `[${host}]:${port}` : `${host}:${port}`;
}

/** Connection target: the port only when it is not the usual 443. */
export function formatTarget(host: string, port: number): string {
  return port === 443 ? host : formatEndpoint(host, port);
}

/** What the update button says while installing. */
export function formatUpdateProgress(progress: UpdateProgress | null): string {
  if (!progress) return t.update.preparing;
  const { downloaded, total } = progress;
  if (total && downloaded >= total) return t.update.launching;
  if (total) return t.update.downloading(Math.floor((downloaded / total) * 100));
  return t.update.downloadingSize((downloaded / 1024 / 1024).toFixed(1));
}

export function formatUpstream(upstream: Upstream): string {
  return `${t.upstream.kind[upstream.kind]} ${formatEndpoint(upstream.host, upstream.port)}`;
}

export function formatRelativeTime(epochMs: number, now: number): string {
  const seconds = Math.max(0, Math.floor((now - epochMs) / 1000));
  if (seconds < 5) return t.time.justNow;
  if (seconds < 60) return t.time.secondsAgo(seconds);
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return t.time.minutesAgo(minutes);
  return t.time.hoursAgo(Math.floor(minutes / 60));
}

export function formatDateTime(epochMs: number): string {
  return new Date(epochMs).toLocaleString("zh-CN");
}

/** File name from a Windows or POSIX path: "C:\\Apps\\Code.exe" -> "Code.exe". */
export function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

const IPV4 = /^\d{1,3}(\.\d{1,3}){3}$/;

/** Connection hosts are either a domain or a bare IP; IPv6 is the only form containing ":". */
export function isIpAddress(host: string): boolean {
  return IPV4.test(host) || host.includes(":");
}

/** Joins the first few items and appends "等 N 个" when the list is longer. */
export function summarizeList(items: string[], max: number): string {
  const shown = items.slice(0, max).join(t.common.listSeparator);
  return items.length > max ? `${shown} ${t.common.andMore(items.length)}` : shown;
}

export function normalizeProcessName(input: string): string {
  return basename(input.trim().replace(/^"|"$/g, ""));
}

/** "*.corp.example.com" / ".corp.example.com" -> "corp.example.com". */
export function normalizeDomainSuffix(input: string): string {
  return input.trim().toLowerCase().replace(/^\*?\.+/, "");
}

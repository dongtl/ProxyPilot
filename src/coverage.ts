// Which parts of a switched-off profile still go through the proxy because
// another, enabled profile covers them. Mirrors the backend's matching:
// domain_suffix rules (the domain and every name under it) and program names
// compared case-insensitively without ".exe".
import type { Profile } from "./api";
import { t } from "./i18n";

/** Program names that apply on this system, without ".exe"/".app": a preset lists both Windows and macOS names. */
export function programsFor(names: string[], mac: boolean): string[] {
  const out: string[] = [];
  for (const name of names) {
    const bundle = /\.app$/i.test(name);
    if (bundle && !mac) continue;
    const shown = name.replace(/\.(exe|app)$/i, "");
    if (!out.some((seen) => seen.toLowerCase() === shown.toLowerCase())) out.push(shown);
  }
  return out;
}

function covers(suffix: string, domain: string): boolean {
  return domain === suffix || domain.endsWith(`.${suffix}`);
}

function programKey(name: string): string {
  return name.replace(/\.exe$/i, "").toLowerCase();
}

/** One enabled profile and what it covers of the profile being looked at. */
export interface CoveredBy {
  by: string;
  /** The user's own entry, whose name is often just the domain itself. */
  custom: boolean;
  items: string[];
}

/** Empty for an enabled profile: everything in it is proxied anyway. */
export function stillProxied(profile: Profile, all: Profile[], mac: boolean): CoveredBy[] {
  if (profile.enabled) return [];
  const out: CoveredBy[] = [];
  for (const other of all) {
    if (!other.enabled || other.id === profile.id) continue;
    const items = profile.domains.filter((domain) => other.domains.some((suffix) => covers(suffix, domain)));
    const theirs = new Set(other.processes.map(programKey));
    for (const name of profile.processes.filter((process) => theirs.has(programKey(process)))) {
      for (const shown of programsFor([name], mac)) if (!items.includes(shown)) items.push(shown);
    }
    if (items.length) out.push({ by: other.name, custom: other.source === "custom", items });
  }
  return out;
}

/** "ggpht.com 仍会走代理，因为“Google / Gemini”已打开", at most three names per line. */
export function coverageLines(covered: CoveredBy[]): string[] {
  return covered.map(({ by, custom, items }) => {
    const shown = items.slice(0, 3).join(t.common.listSeparator);
    const text = items.length > 3 ? `${shown} ${t.common.andMore(items.length)}` : shown;
    return custom ? t.profiles.stillProxiedByCustom(text, by) : t.profiles.stillProxied(text, by);
  });
}

// App-wide state mirrored from the backend. Every mutating command returns the
// full AppState, which simply replaces ours; the backend also pushes changes it
// makes on its own through the state-changed event.
import { computed, ref, shallowRef } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, onStateChanged, type AppState, type SettingsPatch, type Upstream } from "./api";
import { t } from "./i18n";

const state = shallowRef<AppState | null>(null);
const bootError = ref<string | null>(null);

export const appState = computed(() => state.value);
export const loadError = computed(() => bootError.value);

let unlisten: UnlistenFn | null = null;
let pushCount = 0;
let notifyError: (text: string) => void = (text) => console.error(text);

/** Called from inside NMessageProvider so command errors show up as toasts. */
export function setErrorNotifier(fn: (text: string) => void): void {
  notifyError = fn;
}

export function errorText(err: unknown): string {
  if (typeof err === "string" && err) return err;
  if (err instanceof Error && err.message) return err.message;
  return t.common.unknownError;
}

export async function initStore(): Promise<void> {
  bootError.value = null;
  try {
    if (!unlisten) {
      unlisten = await onStateChanged((next) => {
        pushCount += 1;
        state.value = next;
      });
    }
    const pushesBefore = pushCount;
    const initial = await api.getState();
    // A pushed state that arrived while get_state was in flight is at least as new.
    if (pushCount === pushesBefore) state.value = initial;
  } catch (err) {
    bootError.value = errorText(err);
  }
}

export function disposeStore(): void {
  unlisten?.();
  unlisten = null;
}

/** Runs a command that returns AppState. Resolves true on success; errors are toasted. */
async function apply(call: () => Promise<AppState>): Promise<boolean> {
  try {
    state.value = await call();
    return true;
  } catch (err) {
    notifyError(errorText(err));
    return false;
  }
}

/** Runs a read-only command. Resolves null on failure; errors are toasted. */
async function query<T>(call: () => Promise<T>): Promise<T | null> {
  try {
    return await call();
  } catch (err) {
    notifyError(errorText(err));
    return null;
  }
}

export const actions = {
  setRouting: (enabled: boolean) => apply(() => api.setRouting(enabled)),
  setUpstream: (upstream: Upstream) => apply(() => api.setUpstream(upstream)),
  setProfileEnabled: (id: string, enabled: boolean) =>
    apply(() => api.setProfileEnabled(id, enabled)),
  addSite: (input: string) => apply(() => api.addSite(input)),
  addApp: (process: string) => apply(() => api.addApp(process)),
  removeProfile: (id: string) => apply(() => api.removeProfile(id)),
  updateSettings: (patch: SettingsPatch) => apply(() => api.updateSettings(patch)),
  detectUpstreams: (full: boolean) => query(() => api.detectUpstreams(full)),
  testUpstream: () => query(() => api.testUpstream()),
  listProcesses: () => query(() => api.listProcesses()),
  openDataDir: () => query(() => api.openDataDir()),
  runDiagnostics: (intranet: string | null) => query(() => api.runDiagnostics(intranet)),
};

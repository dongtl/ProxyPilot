// App-wide state mirrored from the backend. Every mutating command returns the
// full AppState, which simply replaces ours; the backend also pushes changes it
// makes on its own through the state-changed event.
import { computed, ref, shallowRef } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  api,
  onStateChanged,
  onUpdateProgress,
  type AppState,
  type SettingsPatch,
  type UpdateInfo,
  type UpdateProgress,
  type Upstream,
} from "./api";
import { t } from "./i18n";

const state = shallowRef<AppState | null>(null);
const bootError = ref<string | null>(null);
// Update state is shared so the banner and the settings card show the same thing.
const installing = ref(false);
const progress = ref<UpdateProgress | null>(null);
const dismissedVersion = ref<string | null>(null);

export const appState = computed(() => state.value);
export const loadError = computed(() => bootError.value);
export const updateInstalling = computed(() => installing.value);
export const updateProgress = computed(() => progress.value);
/** The found release, unless the user chose "later" for this version on the banner. */
export const updateBanner = computed(() => {
  const update = state.value?.update ?? null;
  return update && update.version !== dismissedVersion.value ? update : null;
});

let unlisten: UnlistenFn | null = null;
let unlistenProgress: UnlistenFn | null = null;
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
    if (!unlistenProgress) {
      unlistenProgress = await onUpdateProgress((next) => {
        progress.value = next;
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
  unlistenProgress?.();
  unlistenProgress = null;
}

/** "latest" when up to date, the release when one was found, null when the check failed (already toasted). */
async function checkUpdate(): Promise<UpdateInfo | "latest" | null> {
  try {
    const found = await api.checkUpdate();
    if (!found) return "latest";
    // Asking again means the user wants to see it, even after "later".
    dismissedVersion.value = null;
    return found;
  } catch (err) {
    notifyError(errorText(err));
    return null;
  }
}

/** On Windows success never comes back: the app exits and the installer reopens it. */
async function installUpdate(): Promise<void> {
  if (installing.value) return;
  installing.value = true;
  progress.value = null;
  try {
    await api.installUpdate();
  } catch (err) {
    notifyError(errorText(err));
  } finally {
    installing.value = false;
    progress.value = null;
  }
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
  checkUpdate,
  installUpdate,
  dismissUpdate: (version: string) => {
    dismissedVersion.value = version;
  },
  openReleases: () => query(() => api.openReleases()),
};

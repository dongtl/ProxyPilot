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
  type WithState,
} from "./api";
import { t } from "./i18n";

const state = shallowRef<AppState | null>(null);
const bootError = ref<string | null>(null);
// Update state is shared so the banner and the settings card show the same thing.
const installing = ref(false);
const progress = ref<UpdateProgress | null>(null);
/** Why the last "立即更新" failed, kept on screen until the next try. */
const failure = ref<{ version: string; text: string } | null>(null);
const checking = ref(false);
const dismissedVersion = ref<string | null>(null);
const relocating = ref(false);

export const appState = computed(() => state.value);
/** For wording and macOS-only settings. False until the first state arrives. */
export const isMac = computed(() => state.value?.platform === "macos");
export const loadError = computed(() => bootError.value);
export const updateInstalling = computed(() => installing.value);
export const updateProgress = computed(() => progress.value);
export const updateChecking = computed(() => checking.value);
/** Why updating to the offered release failed; null when it did not. */
export const updateFailure = computed(() => {
  const version = state.value?.update?.version;
  return failure.value && failure.value.version === version ? failure.value.text : null;
});
/** Downloading can be stopped; replacing the files cannot. */
export const updateCancellable = computed(() => installing.value && !progress.value?.installing);
export const relocateBusy = computed(() => relocating.value);
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

/**
 * "latest" when up to date, the release when one was found, null when the
 * check failed (already toasted) or was cancelled.
 */
async function checkUpdate(): Promise<UpdateInfo | "latest" | null> {
  if (checking.value) return null;
  checking.value = true;
  try {
    const outcome = await api.checkUpdate();
    if (outcome.result === "cancelled") return null;
    if (outcome.result === "latest") return "latest";
    // Asking again means the user wants to see it, even after "later".
    dismissedVersion.value = null;
    return outcome.update;
  } catch (err) {
    notifyError(errorText(err));
    return null;
  } finally {
    checking.value = false;
  }
}

/**
 * Comes back only when cancelled or failed: on success the app exits and
 * reopens as the new version. A failure stays on the banner and the settings
 * card instead of a toast that is easy to miss.
 */
async function installUpdate(): Promise<void> {
  const version = state.value?.update?.version;
  if (installing.value || !version) return;
  installing.value = true;
  progress.value = null;
  failure.value = null;
  try {
    await api.installUpdate();
  } catch (err) {
    failure.value = { version, text: errorText(err) };
  } finally {
    installing.value = false;
    progress.value = null;
  }
}

/** Stops the check or download in progress; the pending call then comes back. */
async function cancelUpdate(): Promise<void> {
  try {
    await api.cancelUpdate();
  } catch (err) {
    notifyError(errorText(err));
  }
}

/** On success the app quits and the copy in Applications opens, so the button just keeps spinning. */
async function relocate(): Promise<void> {
  if (relocating.value) return;
  relocating.value = true;
  try {
    await api.relocate();
  } catch (err) {
    notifyError(errorText(err));
    relocating.value = false;
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

/** Runs a command that returns AppState plus a result. Resolves with the result, null on failure; errors are toasted. */
async function withResult<T>(call: () => Promise<WithState<T>>): Promise<T | null> {
  try {
    const { state: next, result } = await call();
    state.value = next;
    return result;
  } catch (err) {
    notifyError(errorText(err));
    return null;
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
  /** What adding did, or null when it failed (already toasted). */
  addSite: (input: string) => withResult(() => api.addSite(input)),
  /** The sites that now go through the proxy, or null on failure. */
  acceptSuggestion: (id: string) => withResult(() => api.acceptSuggestion(id)),
  ignoreSuggestion: (id: string) => apply(() => api.ignoreSuggestion(id)),
  dismissSuggestion: (id: string) => apply(() => api.dismissSuggestion(id)),
  removeExtraSite: (id: string, site: string) => apply(() => api.removeExtraSite(id, site)),
  addApp: (process: string) => apply(() => api.addApp(process)),
  removeProfile: (id: string) => apply(() => api.removeProfile(id)),
  updateSettings: (patch: SettingsPatch) => apply(() => api.updateSettings(patch)),
  removeHelper: () => apply(() => api.removeHelper()),
  detectUpstreams: (full: boolean) => query(() => api.detectUpstreams(full)),
  testUpstream: () => query(() => api.testUpstream()),
  listProcesses: () => query(() => api.listProcesses()),
  openDataDir: () => query(() => api.openDataDir()),
  runDiagnostics: (intranet: string | null) => query(() => api.runDiagnostics(intranet)),
  checkUpdate,
  installUpdate,
  cancelUpdate,
  relocate,
  dismissUpdate: (version: string) => {
    dismissedVersion.value = version;
  },
  openReleases: () => query(() => api.openReleases()),
};

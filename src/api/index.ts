// The only place the frontend talks to the backend. Commands reject with a
// Chinese error string the UI can show as-is.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppState,
  Candidate,
  CheckOutcome,
  ConnectionRow,
  DiagnosticReport,
  ProbeReport,
  RunningProcess,
  SettingsPatch,
  UpdateProgress,
  Upstream,
} from "./types";

export * from "./types";

export const api = {
  getState: () => invoke<AppState>("get_state"),
  detectUpstreams: (full: boolean) => invoke<Candidate[]>("detect_upstreams", { full }),
  setUpstream: (upstream: Upstream) => invoke<AppState>("set_upstream", { upstream }),
  testUpstream: () => invoke<ProbeReport>("test_upstream"),
  setRouting: (enabled: boolean) => invoke<AppState>("set_routing", { enabled }),
  setProfileEnabled: (id: string, enabled: boolean) =>
    invoke<AppState>("set_profile_enabled", { id, enabled }),
  addSite: (input: string) => invoke<AppState>("add_site", { input }),
  addApp: (process: string) => invoke<AppState>("add_app", { process }),
  removeProfile: (id: string) => invoke<AppState>("remove_profile", { id }),
  listProcesses: () => invoke<RunningProcess[]>("list_processes"),
  getConnections: () => invoke<ConnectionRow[]>("get_connections"),
  clearConnections: () => invoke<void>("clear_connections"),
  updateSettings: (patch: SettingsPatch) => invoke<AppState>("update_settings", { patch }),
  openDataDir: () => invoke<void>("open_data_dir"),
  /** Takes a few seconds: makes real connections and checks which way each went. */
  runDiagnostics: (intranet: string | null) => invoke<DiagnosticReport>("run_diagnostics", { intranet }),
  /** A found release also lands in AppState.update; the outcome also lands in AppState.lastCheck. */
  checkUpdate: () => invoke<CheckOutcome>("check_update"),
  /**
   * Downloads and installs the found release. Resolves only when cancelled: on
   * success the app exits and reopens as the new version.
   */
  installUpdate: () => invoke<void>("install_update"),
  /** Stops the check or download the user started; rejects once installing has begun. */
  cancelUpdate: () => invoke<void>("cancel_update"),
  openReleases: () => invoke<void>("open_releases"),
  /** macOS: stops routing and removes the background helper; the system asks for the password. */
  removeHelper: () => invoke<AppState>("remove_helper"),
  /** macOS: moves the app into Applications (or opens the copy there), then the app quits and reopens. */
  relocate: () => invoke<void>("relocate"),
};

/** Backend pushes the full state whenever it changes on its own (core restart, upstream lost). */
export function onStateChanged(handler: (state: AppState) => void): Promise<UnlistenFn> {
  return listen<AppState>("state-changed", (event) => handler(event.payload));
}

export function onUpdateProgress(handler: (progress: UpdateProgress) => void): Promise<UnlistenFn> {
  return listen<UpdateProgress>("update-progress", (event) => handler(event.payload));
}

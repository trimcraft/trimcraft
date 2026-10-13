// Connection to the Rust side. Inside Tauri this uses IPC; in a plain browser (vite dev) it
// falls back to a small in-page demo so the UI can be developed and checked without the app.

import type {
  Action,
  AppMeta,
  ButtonPair,
  Connections,
  ConnectorId,
  ControlRef,
  DetectorConfig,
  DiscordSetup,
  DeviceId,
  DiagEntry,
  EngineSnapshot,
  FunctionDef,
  KeyInfo,
  MonitorInfo,
  MonitorRef,
  Movement,
  OverlayConfig,
  OverlayItem,
  RadioCommand,
  RecordKind,
  RecorderConfig,
  Sound,
  VirtualControlDef,
  WidgetPosition,
} from "./types";
import { createDemo } from "./demo";

export interface Backend {
  meta(): Promise<AppMeta>;
  subscribe(onSnapshot: (s: EngineSnapshot) => void): Promise<() => void>;
  setDeviceLabel(device: DeviceId, label: string | null): Promise<void>;
  addEncoder(name: string, device: DeviceId, decrement: number, increment: number): Promise<string>;
  renameEncoder(id: string, name: string): Promise<void>;
  swapEncoder(id: string): Promise<void>;
  removeEncoder(id: string): Promise<void>;
  dismissSuggestion(pair: ButtonPair): Promise<void>;
  saveFunction(fn: FunctionDef): Promise<string>;
  removeFunction(id: string): Promise<void>;
  setFunctionValue(id: string, value: number): Promise<void>;
  toggleOverlayControl(device: DeviceId, control: ControlRef): Promise<boolean>;
  toggleOverlayFunction(id: string): Promise<boolean>;
  setOverlayLayout(layout: Pick<OverlayConfig, "anchor" | "margin" | "scale">): Promise<void>;
  setOverlayVisible(visible: boolean): Promise<void>;
  setHotkey(hotkey: string): Promise<void>;
  setDetectorConfig(config: DetectorConfig): Promise<void>;
  keys(): Promise<KeyInfo[]>;
  saveVirtualControl(control: VirtualControlDef): Promise<string>;
  removeVirtualControl(id: string): Promise<void>;
  /** Sets the active state without running actions. */
  setRadioActive(id: string, state: string | null): Promise<void>;
  /** Runs a command like a trigger would, including actions. */
  commandRadio(id: string, command: RadioCommand): Promise<void>;
  toggleOverlayVirtualControl(id: string): Promise<boolean>;
  startLearn(): Promise<void>;
  stopLearn(): Promise<void>;
  listMonitors(): Promise<MonitorInfo[]>;
  /** Null: primary monitor. */
  setOverlayMonitor(monitor: MonitorRef | null): Promise<void>;
  setOverlayWidget(index: number, item: OverlayItem): Promise<void>;
  setOverlayPositions(positions: (WidgetPosition | null)[]): Promise<void>;
  setOverlayEditing(editing: boolean): Promise<void>;
  setConnections(connections: Connections): Promise<void>;
  /** Checks a connector now; resolves with a message, rejects with the problem. */
  testConnection(id: ConnectorId): Promise<string>;
  /** Saves the Discord client ID and, if given, the client secret (to the credential store). */
  saveDiscordCredentials(clientId: string, clientSecret: string | null): Promise<void>;
  /** Deletes the Discord OAuth tokens; the next test asks in Discord again. */
  forgetDiscordAuthorization(): Promise<void>;
  /** Deletes client secret and tokens and clears the client ID. */
  removeDiscordCredentials(): Promise<void>;
  /** Redirect URI, scopes and portal link for the setup guide. */
  discordSetup(): Promise<DiscordSetup>;
  /** Opens the Discord Developer Portal in the default browser. */
  openDiscordPortal(): Promise<void>;
  /** Soundpad's live sound list; fails when Soundpad is not reachable. */
  soundpadSounds(): Promise<Sound[]>;
  /** Records into the snapshot's `recorder.result` after a countdown. */
  startRecording(kind: RecordKind, movement: Movement): Promise<void>;
  stopRecording(): Promise<void>;
  cancelRecording(): Promise<void>;
  clearRecording(): Promise<void>;
  setRecorderConfig(config: RecorderConfig): Promise<void>;
  /** Plays the action after the countdown. */
  testAction(action: Action): Promise<void>;
  /** Calls `onBatch` with the buffered entries first, then with new ones about every 100 ms. Entries may repeat; drop them by `seq`. */
  subscribeDiagnostics(onBatch: (entries: DiagEntry[]) => void): Promise<() => void>;
  clearDiagnostics(): Promise<void>;
  /** Opens the log folder in the file manager. */
  openLogFolder(): Promise<void>;
}

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauriBackend(): Promise<Backend> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  return {
    meta: () => invoke("get_meta"),
    async subscribe(onSnapshot) {
      const unlisten = await listen<EngineSnapshot>("detent://snapshot", (e) => onSnapshot(e.payload));
      onSnapshot(await invoke<EngineSnapshot>("get_snapshot"));
      return unlisten;
    },
    setDeviceLabel: (device, label) => invoke("set_device_label", { device, label }),
    addEncoder: (name, device, decrement, increment) =>
      invoke("add_encoder", { name, device, decrement, increment }),
    renameEncoder: (id, name) => invoke("rename_encoder", { id, name }),
    swapEncoder: (id) => invoke("swap_encoder", { id }),
    removeEncoder: (id) => invoke("remove_encoder", { id }),
    dismissSuggestion: (pair) => invoke("dismiss_suggestion", { pair }),
    saveFunction: (fn) => invoke("save_function", { function: fn }),
    removeFunction: (id) => invoke("remove_function", { id }),
    setFunctionValue: (id, value) => invoke("set_function_value", { id, value }),
    toggleOverlayControl: (device, control) => invoke("toggle_overlay_control", { device, control }),
    toggleOverlayFunction: (id) => invoke("toggle_overlay_function", { id }),
    setOverlayLayout: (layout) => invoke("set_overlay_layout", { layout }),
    setOverlayVisible: (visible) => invoke("set_overlay_visible", { visible }),
    setHotkey: (hotkey) => invoke("set_hotkey", { hotkey }),
    setDetectorConfig: (config) => invoke("set_detector_config", { config }),
    keys: () => invoke("get_keys"),
    saveVirtualControl: (control) => invoke("save_virtual_control", { control }),
    removeVirtualControl: (id) => invoke("remove_virtual_control", { id }),
    setRadioActive: (id, state) => invoke("set_radio_active", { id, state }),
    commandRadio: (id, command) => invoke("command_radio", { id, command }),
    toggleOverlayVirtualControl: (id) => invoke("toggle_overlay_virtual_control", { id }),
    startLearn: () => invoke("start_learn"),
    stopLearn: () => invoke("stop_learn"),
    listMonitors: () => invoke("list_monitors"),
    setOverlayMonitor: (monitor) => invoke("set_overlay_monitor", { monitor }),
    setOverlayWidget: (index, item) => invoke("set_overlay_widget", { index, item }),
    setOverlayPositions: (positions) => invoke("set_overlay_positions", { positions }),
    setOverlayEditing: (editing) => invoke("set_overlay_editing", { editing }),
    setConnections: (connections) => invoke("set_connections", { connections }),
    testConnection: (id) => invoke("test_connection", { id }),
    saveDiscordCredentials: (clientId, clientSecret) => invoke("save_discord_credentials", { clientId, clientSecret }),
    forgetDiscordAuthorization: () => invoke("forget_discord_authorization"),
    removeDiscordCredentials: () => invoke("remove_discord_credentials"),
    discordSetup: () => invoke("discord_setup"),
    openDiscordPortal: () => invoke("open_discord_portal"),
    soundpadSounds: () => invoke("soundpad_sounds"),
    startRecording: (kind, movement) => invoke("start_recording", { kind, movement }),
    stopRecording: () => invoke("stop_recording"),
    cancelRecording: () => invoke("cancel_recording"),
    clearRecording: () => invoke("clear_recording"),
    setRecorderConfig: (config) => invoke("set_recorder_config", { config }),
    testAction: (action) => invoke("test_action", { action }),
    async subscribeDiagnostics(onBatch) {
      // Listen first, then fetch the buffer: nothing is missed, a few entries may arrive twice.
      const unlisten = await listen<DiagEntry[]>("detent://diagnostics", (e) => onBatch(e.payload));
      onBatch(await invoke<DiagEntry[]>("get_diagnostics"));
      return unlisten;
    },
    clearDiagnostics: () => invoke("clear_diagnostics"),
    openLogFolder: () => invoke("open_log_folder"),
  };
}

let instance: Promise<Backend> | null = null;

export function backend(): Promise<Backend> {
  instance ??= isTauri ? tauriBackend() : Promise.resolve(createDemo());
  return instance;
}

/** Error text from a rejected invoke (Rust errors arrive as strings). */
export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

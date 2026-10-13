// Connection to the Rust side. Inside Tauri this uses IPC; in a plain browser (vite dev) it
// falls back to a small in-page demo so the UI can be developed and checked without the app.

import type {
  Action,
  AppMeta,
  AppSettings,
  ButtonPair,
  Connections,
  ConnectorId,
  ControlRef,
  DetectorConfig,
  DiscordSetup,
  DeviceId,
  Appearance,
  BackupInfo,
  DiagEntry,
  EngineSnapshot,
  ImportMode,
  ImportResult,
  ImportSummary,
  Toast,
  ToastSettings,
  FunctionDef,
  KeyInfo,
  MonitorInfo,
  MonitorRef,
  Movement,
  OverlayConfig,
  OverlayItem,
  ProcessWindow,
  ProfileSwitching,
  VjoyMapping,
  ControlCommand,
  RecordKind,
  RecorderConfig,
  SecretRef,
  Sound,
  VirtualControlDef,
  VoiceRef,
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
  /** Sets the state without running actions: radio/mode state, toggle on ("on") or off (null). */
  setRadioActive(id: string, state: string | null): Promise<void>;
  /** Runs a command like a trigger would, including actions. */
  commandRadio(id: string, command: ControlCommand): Promise<void>;
  /** Sets a counter control's value without running actions. */
  setControlValue(id: string, value: number): Promise<void>;
  /** Replaces the active profile's vJoy mappings. */
  setVjoyMappings(mappings: VjoyMapping[]): Promise<void>;
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
  /** Plays the action (macros after the countdown). */
  testAction(action: Action): Promise<void>;
  /** Installed Windows voices. */
  listVoices(): Promise<VoiceRef[]>;
  /** Names of the sound output devices. */
  listSoundDevices(): Promise<string[]>;
  /** Executable names of programs that have an audio session now. */
  listAudioPrograms(): Promise<string[]>;
  /** Stores a secret HTTP header value in the credential store; resolves with its reference. */
  storeHttpSecret(value: string): Promise<SecretRef>;
  /** Calls `onBatch` with the buffered entries first, then with new ones about every 100 ms. Entries may repeat; drop them by `seq`. */
  subscribeDiagnostics(onBatch: (entries: DiagEntry[]) => void): Promise<() => void>;
  clearDiagnostics(): Promise<void>;
  /** Opens the log folder in the file manager. */
  openLogFolder(): Promise<void>;
  /** Saves the configuration, or the given profiles, to a file chosen in a dialog; credentials are left out. Resolves with the path, null if cancelled. */
  exportSetup(profiles: string[] | null): Promise<string | null>;
  /** Opens a file dialog and checks the file; null if cancelled, rejects with the reason when the file cannot be imported. */
  pickImportFile(): Promise<ImportSummary | null>;
  cancelImport(): Promise<void>;
  /** Applies the picked file; the current configuration is backed up first. */
  applyImport(mode: ImportMode): Promise<ImportResult>;
  listBackups(): Promise<BackupInfo[]>;
  backupNow(): Promise<BackupInfo>;
  /** Restores a backup after backing up the current configuration; resolves with a message. */
  restoreBackup(name: string): Promise<string>;
  openBackupFolder(): Promise<void>;
  setOverlayAppearance(appearance: Appearance): Promise<void>;
  setToastSettings(settings: ToastSettings): Promise<void>;
  /** Places the toast area of the active profile; null: the default place. */
  setToastPosition(position: WidgetPosition | null): Promise<void>;
  /** Calls `onToasts` with the toasts on screen now and again whenever they change. */
  subscribeToasts(onToasts: (toasts: Toast[]) => void): Promise<() => void>;
  /** Shows a sample toast. */
  testToast(): Promise<void>;
  /** Saves the Settings page and registers or removes autostart to match. */
  setSettings(settings: AppSettings): Promise<void>;
  /** Starts Trimcraft as administrator (UAC prompt) and quits this instance. */
  restartElevated(): Promise<void>;
  /** Makes a profile active (no actions run). */
  selectProfile(id: string): Promise<void>;
  /** Creates an empty profile and makes it active; resolves with its id. */
  createProfile(name: string): Promise<string>;
  /** Copies a profile (without its executables) and makes the copy active. */
  duplicateProfile(id: string, name: string): Promise<string>;
  renameProfile(id: string, name: string): Promise<void>;
  /** Fails for the last profile. */
  deleteProfile(id: string): Promise<void>;
  /** Moves a profile to the front: the default profile. */
  makeDefaultProfile(id: string): Promise<void>;
  setProfileExecutables(id: string, executables: string[]): Promise<void>;
  setProfileSwitching(switching: ProfileSwitching): Promise<void>;
  toggleOverlayProfileName(): Promise<boolean>;
  /** Programs with a window, for the executable picker. */
  listProcessWindows(): Promise<ProcessWindow[]>;
}

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauriBackend(): Promise<Backend> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  return {
    meta: () => invoke("get_meta"),
    async subscribe(onSnapshot) {
      const unlisten = await listen<EngineSnapshot>("trimcraft://snapshot", (e) => onSnapshot(e.payload));
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
    setControlValue: (id, value) => invoke("set_control_value", { id, value }),
    setVjoyMappings: (mappings) => invoke("set_vjoy_mappings", { mappings }),
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
    listVoices: () => invoke("list_voices"),
    listSoundDevices: () => invoke("list_sound_devices"),
    listAudioPrograms: () => invoke("list_audio_programs"),
    storeHttpSecret: (value) => invoke("store_http_secret", { value }),
    async subscribeDiagnostics(onBatch) {
      // Listen first, then fetch the buffer: nothing is missed, a few entries may arrive twice.
      const unlisten = await listen<DiagEntry[]>("trimcraft://diagnostics", (e) => onBatch(e.payload));
      onBatch(await invoke<DiagEntry[]>("get_diagnostics"));
      return unlisten;
    },
    clearDiagnostics: () => invoke("clear_diagnostics"),
    openLogFolder: () => invoke("open_log_folder"),
    exportSetup: (profiles) => invoke("export_setup", { profiles }),
    pickImportFile: () => invoke("pick_import_file"),
    cancelImport: () => invoke("cancel_import"),
    applyImport: (mode) => invoke("apply_import", { mode }),
    listBackups: () => invoke("list_backups"),
    backupNow: () => invoke("backup_now"),
    restoreBackup: (name) => invoke("restore_backup", { name }),
    openBackupFolder: () => invoke("open_backup_folder"),
    setOverlayAppearance: (appearance) => invoke("set_overlay_appearance", { appearance }),
    setToastSettings: (settings) => invoke("set_toast_settings", { settings }),
    setToastPosition: (position) => invoke("set_toast_position", { position }),
    async subscribeToasts(onToasts) {
      const unlisten = await listen<Toast[]>("trimcraft://toasts", (e) => onToasts(e.payload));
      onToasts(await invoke<Toast[]>("get_toasts"));
      return unlisten;
    },
    testToast: () => invoke("test_toast"),
    setSettings: (settings) => invoke("set_settings", { settings }),
    restartElevated: () => invoke("restart_elevated"),
    selectProfile: (id) => invoke("select_profile", { id }),
    createProfile: (name) => invoke("create_profile", { name }),
    duplicateProfile: (id, name) => invoke("duplicate_profile", { id, name }),
    renameProfile: (id, name) => invoke("rename_profile", { id, name }),
    deleteProfile: (id) => invoke("delete_profile", { id }),
    makeDefaultProfile: (id) => invoke("make_default_profile", { id }),
    setProfileExecutables: (id, executables) => invoke("set_profile_executables", { id, executables }),
    setProfileSwitching: (switching) => invoke("set_profile_switching", { switching }),
    toggleOverlayProfileName: () => invoke("toggle_overlay_profile_name"),
    listProcessWindows: () => invoke("list_process_windows"),
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

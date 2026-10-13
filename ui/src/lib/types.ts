// Mirrors the serialised Rust types in detent-core (camelCase JSON).

export type DeviceId = string;

export type DeviceKind = { type: "physical" } | { type: "virtual"; driver: "vJoy" };

export type IdentityConfidence = "serial" | "path" | "onlyCandidate" | "new" | "ambiguous";

export type ControlRef =
  | { type: "axis"; index: number }
  | { type: "button"; index: number }
  | { type: "hat"; index: number };

export interface Fingerprint {
  vendorId: number;
  productId: number;
  serial: string | null;
  path: string | null;
  name: string;
}

export interface DeviceState {
  axes: number[];
  buttons: boolean[];
  /** Bitmask: 1 up, 2 right, 4 down, 8 left. */
  hats: number[];
}

export interface DeviceSnapshot {
  id: DeviceId;
  name: string;
  label: string | null;
  kind: DeviceKind;
  fingerprint: Fingerprint;
  backendGuid: string;
  confidence: IdentityConfidence;
  connected: boolean;
  state: DeviceState;
}

export interface Activity {
  device: DeviceId;
  control: ControlRef;
  timeNs: number;
}

export interface PulseStats {
  presses: number;
  pulses: number;
  minPulseMs: number | null;
  maxPulseMs: number | null;
  avgPulseMs: number | null;
  longestRun: number;
  overlaps: number;
  held: boolean;
}

export interface EncoderView {
  id: string;
  name: string;
  device: DeviceId;
  decrement: number;
  increment: number;
  decrementStats: PulseStats | null;
  incrementStats: PulseStats | null;
  stepsDown: number;
  stepsUp: number;
}

export interface ButtonPair {
  device: DeviceId;
  decrement: number;
  increment: number;
}

export type EndBehavior = "wrap" | "clamp";

export interface CounterSpec {
  min: number;
  max: number;
  step: number;
  initial: number;
  end: EndBehavior;
  decimals: number;
}

export type CounterInput = { type: "encoder"; encoder: string } | { type: "none" };

export interface FunctionDef {
  id: string;
  name: string;
  kind: { type: "counter"; spec: CounterSpec; input: CounterInput };
}

export interface FunctionView extends FunctionDef {
  value: number;
  display: string;
}

export type Anchor = "topLeft" | "topRight" | "bottomLeft" | "bottomRight";

export type OverlayWidget =
  | { type: "control"; device: DeviceId; control: ControlRef; label: string | null }
  | { type: "function"; function: string }
  | { type: "radio"; control: string; layout: RadioLayout }
  /** The active profile's name. */
  | { type: "profileName" };

export type RadioLayout = "vertical" | "horizontal";

/** Top-left corner as fractions (0..1) of the overlay monitor. */
export interface WidgetPosition {
  x: number;
  y: number;
}

/** A widget plus where it is; `position: null` stacks it in the anchored corner. */
export type OverlayItem = OverlayWidget & { position: WidgetPosition | null };

export interface MonitorRef {
  name: string | null;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface MonitorInfo extends MonitorRef {
  primary: boolean;
  scale: number;
}

export interface OverlayConfig {
  visible: boolean;
  hotkey: string;
  /** Null: primary monitor. */
  monitor: MonitorRef | null;
  anchor: Anchor;
  margin: number;
  scale: number;
  widgets: OverlayItem[];
}

export interface DetectorConfig {
  maxPulseMs: number;
  maxRunGapMs: number;
  minRunLength: number;
}

export interface ButtonStats {
  device: DeviceId;
  button: number;
  stats: PulseStats;
}

export interface EngineSnapshot {
  devices: DeviceSnapshot[];
  lastActivity: Activity | null;
  backendError: string | null;
  encoders: EncoderView[];
  suggestions: ButtonPair[];
  halfDetected: ButtonPair[];
  buttonStats: ButtonStats[];
  functions: FunctionView[];
  /** All profiles in order; the first is the default profile. */
  profiles: ProfileView[];
  activeProfile: string;
  profileSwitching: ProfileSwitching;
  /** The program in the foreground (not Detent), as auto-switching sees it. */
  foreground: string | null;
  /** The active profile's virtual controls. */
  virtualControls: VirtualControlView[];
  /** Null unless learn mode is on or just captured something. */
  learn: LearnView | null;
  overlayEditing: boolean;
  recorder: RecorderView;
  recorderConfig: RecorderConfig;
  connections: Connections;
  /** Every connector with its live status. */
  connectors: ConnectorView[];
  /** Global settings with the active profile's widgets. */
  overlay: OverlayConfig;
  detector: DetectorConfig;
  /** App-wide settings (Settings page). */
  settings: AppSettings;
  shell: ShellStatus;
}

export interface AppMeta {
  version: string;
  backend: string;
  dataDir: string;
  logDir: string;
  platform: string;
  /** Detent runs as administrator. */
  elevated: boolean;
}

// --- Virtual controls (detent-core: virtual_control.rs, action.rs) ---------------------------

export type HatDirection = "up" | "upRight" | "right" | "downRight" | "down" | "downLeft" | "left" | "upLeft";

export type StepDirection = "decrement" | "increment";

/** What a trigger listens to (detent-core trigger.rs). */
export type TriggerInput =
  | { type: "button"; device: DeviceId; index: number }
  /** Pressed while the hat points that way. */
  | { type: "hat"; device: DeviceId; index: number; direction: HatDirection }
  | { type: "encoderStep"; encoder: string; direction: StepDirection }
  /** A range of an axis in percent (0..100); "pressed" while inside. */
  | { type: "axisZone"; device: DeviceId; index: number; min: number; max: number };

/** Press: on press (a short press on release if the input also has long or double triggers).
 *  For axis zones press = entering, release = leaving; encoder detents only press. */
export type PressKind = { kind: "press" } | { kind: "release" } | { kind: "long"; ms: number } | { kind: "double"; ms: number };

/** A button that must be held as a modifier. */
export interface ButtonRef {
  device: DeviceId;
  index: number;
}

/** Only when mode `mode` is in state `state` (ids). */
export interface Condition {
  mode: string;
  state: string;
}

export type Trigger = TriggerInput & {
  press: PressKind;
  /** All must be held; the most specific trigger on an input wins. */
  modifiers: ButtonRef[];
  /** All must hold. */
  conditions: Condition[];
};

export type KeyStep =
  | { type: "keyDown"; key: string }
  | { type: "keyUp"; key: string }
  | { type: "keyTap"; key: string; holdMs: number | null }
  | { type: "delay"; ms: number };

export type MouseButton = "left" | "right" | "middle" | "x1" | "x2";

export type MouseStep =
  | { type: "buttonDown"; button: MouseButton }
  | { type: "buttonUp"; button: MouseButton }
  | { type: "click"; button: MouseButton; holdMs: number | null }
  | { type: "wheel"; delta: number }
  | { type: "moveRelative"; dx: number; dy: number }
  /** Jump to a position, as fractions of the macro's monitor. */
  | { type: "moveTo"; x: number; y: number }
  /** Recorded absolute movement. */
  | { type: "movePath"; points: PathPoint[] }
  /** Recorded relative movement (mouse-look). */
  | { type: "moveRelativePath"; points: RelPoint[] }
  | { type: "delay"; ms: number };

/** Fractions of the macro's monitor; `t` in ms after the path starts. */
export interface PathPoint {
  x: number;
  y: number;
  t: number;
}

/** Offset in mickeys from where the path starts; `t` in ms after the path starts. */
export interface RelPoint {
  dx: number;
  dy: number;
  t: number;
}

/** How a mouse macro records movement. */
export type Movement = "absolute" | "relative";

export interface Playback {
  /** 100 = recorded speed; scales waits and paths, not hold times. */
  speedPercent: number;
  /** Waits shorter than this are skipped. */
  minDelayMs: number;
  /** Absolute paths jump straight to their end. */
  endpointOnly: boolean;
}

export type KeyboardMacro = { type: "keyboardMacro"; steps: KeyStep[]; playback: Playback };
export type MouseMacro = {
  type: "mouseMacro";
  steps: MouseStep[];
  playback: Playback;
  movement: Movement;
  /** Monitor absolute positions refer to; null = primary. */
  monitor: MonitorRef | null;
};

export type Action =
  | KeyboardMacro
  | MouseMacro
  | { type: "soundpad"; command: SoundpadCommand }
  | { type: "discord"; command: DiscordCommand }
  | { type: "switchProfile"; target: ProfileTarget }
  | SpeakAction
  | PlaySoundAction
  | { type: "volume"; target: VolumeTarget; command: VolumeCommand }
  | { type: "voicemeeter"; target: VmTarget; command: VmCommand }
  | { type: "mediaKey"; key: MediaKey }
  | LaunchAction
  | HttpAction;

// --- More actions (detent-core: audio.rs, voicemeeter.rs, launch.rs, webhook.rs) -------------

/** An installed Windows voice; looked up by id. */
export interface VoiceRef {
  id: string;
  name: string;
}

/** Text may contain {control}, {state}, {profile}, {value}. A new speech interrupts the last. */
export interface SpeakAction {
  type: "speak";
  text: string;
  /** Null: the Windows default voice. */
  voice: VoiceRef | null;
  /** -10..10, 0 is normal. */
  rate: number;
  /** 0..100. */
  volume: number;
  /** The queue waits until the text is spoken. */
  wait: boolean;
}

export interface PlaySoundAction {
  type: "playSound";
  path: string;
  /** 0..100. */
  volume: number;
  /** Output device name; null: the Windows default. */
  device: string | null;
  stopPrevious: boolean;
  wait: boolean;
}

export type VolumeTarget = { type: "master" } | { type: "app"; process: string };

export type VolumeCommand =
  | { type: "set"; percent: number }
  | { type: "change"; delta: number }
  | { type: "mute" }
  | { type: "unmute" }
  | { type: "toggleMute" };

export type VmKind = "strip" | "bus";

/** Index from 0; the UI shows it from 1. */
export interface VmTarget {
  kind: VmKind;
  index: number;
}

export type VmCommand =
  | { type: "setGain"; db: number }
  | { type: "changeGain"; deltaDb: number }
  | { type: "mute" }
  | { type: "unmute" }
  | { type: "toggleMute" };

export type MediaKey = "playPause" | "next" | "previous" | "stop" | "volumeUp" | "volumeDown" | "mute";

export type LaunchTarget =
  | { type: "program"; path: string; arguments: string; workingDir: string; onlyIfNotRunning: boolean }
  | { type: "url"; url: string }
  | { type: "open"; path: string };

export interface LaunchAction {
  type: "launch";
  target: LaunchTarget;
  /** Programs: the queue waits until the program has exited. */
  wait: boolean;
}

export type HttpMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

export interface HttpHeader {
  name: string;
  /** Empty when the value is secret. */
  value: string;
  /** Where a secret value is stored; null for a plain value. */
  secret: SecretRef | null;
}

export interface HttpAction {
  type: "http";
  method: HttpMethod;
  /** Placeholders in the URL are URL-encoded. */
  url: string;
  headers: HttpHeader[];
  /** Not sent for GET and DELETE. */
  body: string;
  timeoutMs: number;
  wait: boolean;
}

// --- Macro recorder (detent-core: recording.rs) ----------------------------------------------

export type RecordKind = "keyboard" | "mouse";

export interface RecorderConfig {
  stopHotkey: string;
  /** Pixels (mickeys for relative movement) a simplified path may deviate. */
  tolerance: number;
  sampleIntervalMs: number;
}

export interface RecordingResult {
  session: number;
  action: KeyboardMacro | MouseMacro;
  notes: string[];
}

export interface RecorderView {
  session: number;
  phase: "idle" | "countdown" | "recording";
  kind: RecordKind | null;
  movement: Movement;
  result: RecordingResult | null;
  error: string | null;
}

// --- Soundpad (detent-core: soundpad.rs) ------------------------------------------------------

/** A sound as stored in actions: by title and path, never by index (indices shift). */
export interface SoundRef {
  title: string;
  path: string;
}

/** Speakers (you hear it), mic (others hear it) or both. */
export type SoundOutput = "both" | "speakers" | "mic";

export type SoundpadCommand =
  | { type: "play"; sound: SoundRef; output: SoundOutput }
  | { type: "playRandom"; output: SoundOutput }
  | { type: "playNext" }
  | { type: "playPrevious" }
  | { type: "stop" }
  | { type: "togglePause" };

/** An entry of Soundpad's live sound list. */
export interface Sound {
  index: number;
  title: string;
  path: string;
  duration: string;
}

// --- Discord (detent-core: discord.rs) --------------------------------------------------------

/** Mute/deafen set the state (skipped when it already holds); toggles use the live state. */
export type DiscordCommand = "mute" | "unmute" | "deafen" | "undeafen" | "toggleMute" | "toggleDeafen";

/** The user's own mute and deaf state in Discord. */
/** The setup guide step that fixes a problem; the UI numbers the steps. */
export type GuideStep = "clientId" | "clientSecret" | "redirect" | "discordRunning" | "authorize" | "appTesters";

/** Values the setup guide shows, from the constants the connector uses. */
export interface DiscordSetup {
  redirectUri: string;
  scopes: string[];
  portalUrl: string;
}

export interface VoiceState {
  mute: boolean;
  deaf: boolean;
}

// --- Connections (detent-core: connection.rs) -------------------------------------------------

export type ConnectorId = "soundpad" | "discord" | "voicemeeter";

export type ConnectorStatus =
  | { state: "notConfigured" }
  | { state: "connecting" }
  /** Discord shows a consent request the user has to approve. */
  | { state: "waitingForApproval" }
  | { state: "connected" }
  | { state: "error"; message: string };

/** Which credentials are in the credential store (never the values). */
export interface CredentialState {
  secretSaved: boolean;
  /** OAuth tokens are stored. */
  authorized: boolean;
}

export interface ConnectorView {
  id: ConnectorId;
  name: string;
  enabled: boolean;
  status: ConnectorStatus;
  /** E.g. "26 sounds", "Connected as Sam". */
  detail: string | null;
  /** Last problem running one of its actions. */
  warning: string | null;
  /** Live mute/deaf state (Discord, while connected). */
  voice: VoiceState | null;
  /** For connectors with credentials (Discord). */
  credentials: CredentialState | null;
  /** The setup guide step that fixes the last problem (Discord), also while disabled. */
  guide: GuideStep | null;
  /** The program the connector needs is not installed (Voicemeeter); its actions are hidden. */
  missing: string | null;
}

export interface SoundpadSettings {
  enabled: boolean;
  timeoutMs: number;
  listMaxAgeMs: number;
}

/** Where a secret is stored, never the secret. */
export type SecretRef = { store: "windowsCredentialManager"; target: string };

export interface DiscordSettings {
  enabled: boolean;
  clientId: string;
  /** Null until a client secret was saved. */
  clientSecret: SecretRef | null;
}

export interface VoicemeeterSettings {
  enabled: boolean;
}

/** Connector settings as stored in setup.json. Secrets never go here. */
export interface Connections {
  soundpad: SoundpadSettings;
  discord: DiscordSettings;
  voicemeeter: VoicemeeterSettings;
}

export interface RadioState {
  id: string;
  name: string;
  select: Trigger[];
  onActivate: Action[];
  onDeactivate: Action[];
}

export type Startup = "restoreLast" | "empty";

export interface Radio {
  states: RadioState[];
  increment: Trigger[];
  decrement: Trigger[];
  clear: Trigger[];
  /** Off if a state is active, otherwise back to the most recently active state. */
  toggleOff: Trigger[];
  end: EndBehavior;
  toggleOnReselect: boolean;
  startup: Startup;
}

export interface Toggle {
  toggle: Trigger[];
  on: Trigger[];
  off: Trigger[];
  onOn: Action[];
  onOff: Action[];
  /** "empty": off at startup. */
  startup: Startup;
}

export interface Momentary {
  /** Active while any of them is held. */
  triggers: Trigger[];
  onPress: Action[];
  onRelease: Action[];
  /** Active while nothing is held instead. */
  invert: boolean;
}

export interface CounterControl {
  spec: CounterSpec;
  increment: Trigger[];
  decrement: Trigger[];
  reset: Trigger[];
  /** Actions can use the new value as {value}. */
  onChange: Action[];
  /** "empty": the initial value at startup. */
  startup: Startup;
}

export interface ModeState {
  id: string;
  name: string;
  select: Trigger[];
  /** Active while held; back to the previous state on release. */
  shift: Trigger[];
  onActivate: Action[];
  onDeactivate: Action[];
}

export interface Mode {
  /** Exactly one is active; the first by default. */
  states: ModeState[];
  next: Trigger[];
  previous: Trigger[];
  end: EndBehavior;
  /** "empty": the first state at startup. */
  startup: Startup;
}

export type VirtualControlKind =
  | ({ type: "radio" } & Radio)
  | ({ type: "toggle" } & Toggle)
  | ({ type: "momentary" } & Momentary)
  | ({ type: "counter" } & CounterControl)
  | ({ type: "mode" } & Mode);

export type ControlType = VirtualControlKind["type"];

export interface VirtualControlDef<K extends VirtualControlKind = VirtualControlKind> {
  id: string;
  name: string;
  kind: K;
}

export type VirtualControlView = VirtualControlDef & {
  /** Id of the active state (radio, mode; "on" for a toggle that is on). */
  active: string | null;
  activeName: string | null;
  /** Lit: a radio state active, a toggle or momentary control on; always for a mode. */
  on: boolean;
  /** A counter's value, and formatted. */
  value: number | null;
  display: string | null;
  /** A mode held in a state by a shift trigger. */
  shifted: boolean;
};

export interface LearnView {
  captured: Trigger | null;
}

export interface KeyInfo {
  name: string;
  scan: number;
  extended: boolean;
}

/** Commands the UI can run on a control; they fire actions like a trigger would. Next/previous
 *  (radio, mode, counter), clear and toggle off (radio), toggle/on/off (toggle), reset (counter). */
export type ControlCommand = "increment" | "decrement" | "clear" | "toggleOff" | "toggle" | "on" | "off" | "reset";

// --- Diagnostics event log (detent-core diagnostics::Entry) ------------------------------------

export type DiagCategory = "input" | "trigger" | "control" | "action" | "connector" | "log";
export type DiagLevel = "info" | "warn" | "error";

export interface DiagEntry {
  /** Increases by one per entry; used to drop entries received twice. */
  seq: number;
  /** Milliseconds since the Unix epoch. */
  timeMs: number;
  category: DiagCategory;
  level: DiagLevel;
  /** Stable device id, when the entry belongs to a device. */
  device: string | null;
  deviceName: string | null;
  text: string;
  detail: string | null;
}

// --- Settings page (detent-core: settings.rs) ------------------------------------------------

export interface AppSettings {
  /** The close button hides the window; Detent keeps running in the tray. */
  closeToTray: boolean;
  startWithWindows: boolean;
  /** Start in the tray, also when started by hand (a start by Windows always does). */
  startMinimized: boolean;
  /** Relaunch elevated (UAC prompt) when started without administrator rights. */
  runAsAdmin: boolean;
  /** Set by the app; ignored when saving. */
  trayNoticeShown: boolean;
}

/** How Detent is started at logon: Run registry value or elevated Task Scheduler task. */
export type AutostartKind = "off" | "runKey" | "task";

export interface ShellStatus {
  elevated: boolean;
  /** What is registered right now. */
  autostart: AutostartKind;
  /** Why autostart differs from the settings, or why changing it failed. */
  autostartNote: string | null;
  /** The foreground program runs as administrator and Detent does not: its name. */
  elevatedForeground: string | null;
}

// --- Profiles (detent-core: profile.rs) -------------------------------------------------------

/** A profile without its controls and widgets (the snapshot carries the active profile's). */
export interface ProfileView {
  id: string;
  name: string;
  /** Executable names that make the profile active, e.g. "DCS.exe". */
  executables: string[];
}

/** When no profile lists the foreground program: keep the profile, or go to the default (first) one. */
export type NoMatch = "keepCurrent" | "switchToDefault";

export interface ProfileSwitching {
  autoSwitch: boolean;
  noMatch: NoMatch;
}

/** Which profile a "Switch profile" action makes active. */
export type ProfileTarget = { type: "profile"; profile: string } | { type: "next" } | { type: "previous" };

/** A program with a window, for "Pick from running processes". */
export interface ProcessWindow {
  exe: string;
  title: string;
}

// --- Backup and transfer (detent-core: backup.rs, transfer.rs) --------------------------------

export interface BackupInfo {
  /** File name; the id for restoring. */
  name: string;
  /** When it was written (UTC seconds since the Unix epoch). */
  unixSecs: number;
  size: number;
  /** Null if the file cannot be read. */
  profiles: number | null;
}

export type ExportKind = "all" | "profiles" | "backup";

export interface ImportProfileSummary {
  id: string;
  name: string;
  controls: number;
  executables: string[];
}

/** What an import file holds, as shown before importing. */
export interface ImportSummary {
  kind: ExportKind;
  exportedAt: string | null;
  detentVersion: string | null;
  schemaVersion: number;
  profiles: ImportProfileSummary[];
  /** Fields left out when exporting (credentials); they have to be entered again. */
  strippedFields: string[];
}

export type ImportMode = { type: "replaceAll" } | { type: "addProfiles"; profiles: string[] };

export interface AddedProfile {
  id: string;
  name: string;
  /** The name in the file, when it had to change. */
  renamedFrom: string | null;
}

export interface ImportResult {
  message: string;
  report: { added: AddedProfile[]; droppedExecutables: string[]; addedEncoders: number; addedFunctions: number } | null;
}

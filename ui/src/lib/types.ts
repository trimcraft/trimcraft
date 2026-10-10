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
  | { type: "radio"; control: string; layout: RadioLayout };

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
  virtualControls: VirtualControlView[];
  /** Null unless learn mode is on or just captured something. */
  learn: LearnView | null;
  overlayEditing: boolean;
  overlay: OverlayConfig;
  detector: DetectorConfig;
}

export interface AppMeta {
  version: string;
  backend: string;
  dataDir: string;
  logDir: string;
  platform: string;
}

// --- Virtual controls (detent-core: virtual_control.rs, action.rs) ---------------------------

export type HatDirection = "up" | "upRight" | "right" | "downRight" | "down" | "downLeft" | "left" | "upLeft";

export type StepDirection = "decrement" | "increment";

export type Trigger =
  | { type: "button"; device: DeviceId; index: number }
  | { type: "hat"; device: DeviceId; index: number; direction: HatDirection }
  | { type: "encoderStep"; encoder: string; direction: StepDirection };

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
  | { type: "delay"; ms: number };

export type Action =
  | { type: "keyboardMacro"; steps: KeyStep[] }
  | { type: "mouseMacro"; steps: MouseStep[] };

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

export interface VirtualControlDef {
  id: string;
  name: string;
  kind: { type: "radio" } & Radio;
}

export interface VirtualControlView extends VirtualControlDef {
  /** Id of the active state. */
  active: string | null;
  activeName: string | null;
}

export interface LearnView {
  captured: Trigger | null;
}

export interface KeyInfo {
  name: string;
  scan: number;
  extended: boolean;
}

/** Radio commands the UI can run; they fire actions like a trigger would. */
export type RadioCommand = "increment" | "decrement" | "clear" | "toggleOff";

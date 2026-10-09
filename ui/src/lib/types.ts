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
  | { type: "function"; function: string };

export interface OverlayConfig {
  visible: boolean;
  hotkey: string;
  anchor: Anchor;
  margin: number;
  scale: number;
  widgets: OverlayWidget[];
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

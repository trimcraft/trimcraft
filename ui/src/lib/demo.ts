// In-browser stand-in for the Rust engine, used only when the UI runs outside Tauri
// (`npm run dev` in a browser). It is deliberately simple: enough to exercise every screen.

import type { Backend } from "./backend";
import type { DeviceSnapshot, EngineSnapshot, FunctionDef, FunctionView, PulseStats } from "./types";

function device(id: string, name: string, vid: number, pid: number, axes: number, buttons: number, hats: number, virt = false): DeviceSnapshot {
  return {
    id,
    name,
    label: null,
    kind: virt ? { type: "virtual", driver: "vJoy" } : { type: "physical" },
    fingerprint: { vendorId: vid, productId: pid, serial: null, path: `demo://${id}`, name },
    backendGuid: "demo",
    confidence: "path",
    connected: true,
    state: { axes: Array(axes).fill(0), buttons: Array(buttons).fill(false), hats: Array(hats).fill(0) },
  };
}

const stats = (pulses: number): PulseStats => ({
  presses: pulses, pulses, minPulseMs: 28.4, maxPulseMs: 34.9, avgPulseMs: 31.2, longestRun: 5, overlaps: 0, held: false,
});

export function createDemo(): Backend {
  const s: EngineSnapshot = {
    devices: [
      device("f00d:0001#1", "Demo Stick", 0xf00d, 1, 4, 32, 2),
      device("f00d:0002#1", "Demo Throttle", 0xf00d, 2, 3, 24, 1),
      device("f00d:0002#2", "Demo Throttle", 0xf00d, 2, 3, 24, 1),
      device("f00d:0003#1", "Demo Panel", 0xf00d, 3, 0, 16, 0),
      device("1234:bead#1", "vJoy Device", 0x1234, 0xbead, 8, 32, 1, true),
    ],
    lastActivity: null,
    backendError: null,
    encoders: [],
    suggestions: [{ device: "f00d:0003#1", decrement: 10, increment: 11 }],
    halfDetected: [],
    buttonStats: [],
    functions: [],
    overlay: { visible: true, hotkey: "Ctrl+Shift+F10", anchor: "bottomLeft", margin: 32, scale: 1, widgets: [] },
    detector: { maxPulseMs: 200, maxRunGapMs: 600, minRunLength: 3 },
  };
  const listeners = new Set<(s: EngineSnapshot) => void>();
  const positions = new Map<string, number>();
  let tick = 0;

  const emit = () => listeners.forEach((l) => l(structuredClone(s)));

  const counterValue = (f: FunctionDef): number => {
    const { spec } = f.kind;
    return spec.min + (positions.get(f.id) ?? 0) * spec.step;
  };
  const view = (f: FunctionDef): FunctionView => {
    const value = counterValue(f);
    return { ...f, value, display: value.toFixed(f.kind.spec.decimals) };
  };
  const refreshFunctions = () => {
    s.functions = s.functions.map(view);
  };
  const step = (encoderId: string, dir: number) => {
    for (const f of s.functions) {
      if (f.kind.input.type !== "encoder" || f.kind.input.encoder !== encoderId) continue;
      const { spec } = f.kind;
      const last = Math.floor((spec.max - spec.min) / spec.step + 1e-9);
      const target = (positions.get(f.id) ?? 0) + dir;
      positions.set(f.id, spec.end === "wrap" ? ((target % (last + 1)) + last + 1) % (last + 1) : Math.min(Math.max(target, 0), last));
    }
    refreshFunctions();
  };

  setInterval(() => {
    tick++;
    const t = tick / 30;
    const [stick, throttle1, throttle2, panel] = s.devices;
    stick!.state.axes[0] = Math.sin(t * 0.9) * 0.8;
    stick!.state.axes[1] = Math.cos(t * 1.3) * 0.6;
    throttle1!.state.axes[2] = Math.sin(t * 0.2);
    throttle2!.state.axes[2] = Math.cos(t * 0.25);
    stick!.state.buttons[0] = Math.floor(tick / 30) % 2 === 1;
    stick!.state.hats[0] = [0, 1, 2, 4, 8][Math.floor(tick / 45) % 5]!;
    // Encoder: a run of 5 detents every 4 s, alternating direction.
    const phase = tick % 120;
    const pulse = phase < 15 && phase % 3 === 0;
    const up = Math.floor(tick / 120) % 2 === 0;
    panel!.state.buttons[10] = pulse && !up;
    panel!.state.buttons[11] = pulse && up;
    if (pulse) {
      for (const e of s.encoders) {
        if (e.device !== panel!.id) continue;
        const button = up ? 11 : 10;
        if (button === e.increment) { e.stepsUp++; step(e.id, 1); }
        if (button === e.decrement) { e.stepsDown++; step(e.id, -1); }
      }
      s.lastActivity = { device: panel!.id, control: { type: "button", index: up ? 11 : 10 }, timeNs: tick };
    }
    emit();
  }, 33);

  let nextEnc = 1;
  let nextFn = 1;
  const ok = async () => emit();

  return {
    meta: async () => ({ version: "demo", backend: "browser demo", dataDir: "–", logDir: "–", platform: "browser" }),
    async subscribe(cb) {
      listeners.add(cb);
      cb(structuredClone(s));
      return () => listeners.delete(cb);
    },
    async setDeviceLabel(id, label) {
      const d = s.devices.find((x) => x.id === id);
      if (d) d.label = label?.trim() || null;
      emit();
    },
    async addEncoder(name, device, decrement, increment) {
      if (decrement === increment) throw new Error("both directions need different buttons");
      const id = `enc-${nextEnc++}`;
      s.encoders.push({ id, name: name.trim() || `Encoder ${nextEnc - 1}`, device, decrement, increment, decrementStats: stats(3), incrementStats: stats(4), stepsDown: 0, stepsUp: 0 });
      s.suggestions = s.suggestions.filter((p) => !(p.device === device && p.decrement === decrement));
      emit();
      return id;
    },
    async renameEncoder(id, name) {
      const e = s.encoders.find((x) => x.id === id);
      if (e && name.trim()) e.name = name.trim();
      emit();
    },
    async swapEncoder(id) {
      const e = s.encoders.find((x) => x.id === id);
      if (e) [e.decrement, e.increment] = [e.increment, e.decrement];
      emit();
    },
    async removeEncoder(id) {
      s.encoders = s.encoders.filter((e) => e.id !== id);
      for (const f of s.functions) if (f.kind.input.type === "encoder" && f.kind.input.encoder === id) f.kind.input = { type: "none" };
      emit();
    },
    async dismissSuggestion(pair) {
      s.suggestions = s.suggestions.filter((p) => !(p.device === pair.device && p.decrement === pair.decrement));
      emit();
    },
    async saveFunction(fn) {
      const { spec } = fn.kind;
      if (!(spec.step > 0)) throw new Error("step must be greater than zero");
      if (!(spec.max > spec.min)) throw new Error("maximum must be greater than minimum");
      const f: FunctionDef = JSON.parse(JSON.stringify({ ...fn, id: fn.id || `fn-${nextFn++}` }));
      if (!fn.id) positions.set(f.id, Math.round((spec.initial - spec.min) / spec.step));
      const i = s.functions.findIndex((x) => x.id === f.id);
      if (i >= 0) s.functions[i] = view(f); else s.functions.push(view(f));
      emit();
      return f.id;
    },
    async removeFunction(id) {
      s.functions = s.functions.filter((f) => f.id !== id);
      s.overlay.widgets = s.overlay.widgets.filter((w) => !(w.type === "function" && w.function === id));
      emit();
    },
    async setFunctionValue(id, value) {
      const f = s.functions.find((x) => x.id === id);
      if (f) positions.set(id, Math.round((value - f.kind.spec.min) / f.kind.spec.step));
      refreshFunctions();
      emit();
    },
    async toggleOverlayControl(device, control) {
      const i = s.overlay.widgets.findIndex((w) => w.type === "control" && w.device === device && w.control.type === control.type && w.control.index === control.index);
      if (i >= 0) s.overlay.widgets.splice(i, 1); else s.overlay.widgets.push({ type: "control", device, control, label: null });
      emit();
      return i < 0;
    },
    async toggleOverlayFunction(id) {
      const i = s.overlay.widgets.findIndex((w) => w.type === "function" && w.function === id);
      if (i >= 0) s.overlay.widgets.splice(i, 1); else s.overlay.widgets.push({ type: "function", function: id });
      emit();
      return i < 0;
    },
    setOverlayLayout: async (layout) => { Object.assign(s.overlay, layout); await ok(); },
    setOverlayVisible: async (visible) => { s.overlay.visible = visible; await ok(); },
    setHotkey: async (hotkey) => { s.overlay.hotkey = hotkey; await ok(); },
    setDetectorConfig: async (config) => { s.detector = config; await ok(); },
  };
}

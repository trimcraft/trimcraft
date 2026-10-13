import type {
  DiagEntry,
  Action,
  ControlRef,
  DiscordCommand,
  DeviceSnapshot,
  EngineSnapshot,
  HatDirection,
  IdentityConfidence,
  KeyStep,
  MouseStep,
  ProfileTarget,
  ProfileView,
  SoundOutput,
  SoundpadCommand,
  Trigger,
  TriggerInput,
  ButtonRef,
  Condition,
  ControlType,
  VirtualControlKind,
  VjoyAxis,
  VjoyCommand,
  AxisCurve,
} from "./types";

/** Buttons, axes and hats are 0-based internally and shown 1-based, like Windows does. */
export function controlName(c: ControlRef): string {
  const n = c.index + 1;
  switch (c.type) {
    case "axis":
      return `Axis ${n}`;
    case "button":
      return `Button ${n}`;
    case "hat":
      return `Hat ${n}`;
  }
}

export function deviceTitle(d: DeviceSnapshot | undefined, fallbackId = "?"): string {
  if (!d) return `Missing device ${fallbackId}`;
  return d.label ?? d.name;
}

export function hex4(n: number): string {
  return n.toString(16).padStart(4, "0");
}

export const confidenceText: Record<IdentityConfidence, string> = {
  serial: "Recognised by serial number",
  path: "Recognised by USB port",
  onlyCandidate: "Recognised as the only missing device of this model",
  new: "New device",
  ambiguous: "Identity uncertain: identical devices changed ports. Check the label.",
};

export function findDevice(s: EngineSnapshot, id: string): DeviceSnapshot | undefined {
  return s.devices.find((d) => d.id === id);
}

export function ms(v: number | null): string {
  return v === null ? "–" : `${v.toFixed(1)} ms`;
}

const HAT_ARROWS: Record<number, string> = {
  0: "·", 1: "↑", 2: "→", 3: "↗", 4: "↓", 6: "↘", 8: "←", 9: "↖", 12: "↙",
};

export function hatArrow(mask: number): string {
  return HAT_ARROWS[mask] ?? "?";
}

export function sameControl(a: ControlRef, b: ControlRef): boolean {
  return a.type === b.type && a.index === b.index;
}

export const HAT_DIRECTIONS: [HatDirection, string][] = [
  ["up", "↑ up"],
  ["upRight", "↗ up-right"],
  ["right", "→ right"],
  ["downRight", "↘ down-right"],
  ["down", "↓ down"],
  ["downLeft", "↙ down-left"],
  ["left", "← left"],
  ["upLeft", "↖ up-left"],
];

const hatDirectionText = (d: HatDirection) => HAT_DIRECTIONS.find(([v]) => v === d)?.[1] ?? d;

/** E.g. "Demo Stick · Button 4", "Radio knob +", "Demo Stick · Hat 1 ↑ up", "Demo Throttle · Axis 3 80–100 %". */
export function inputText(s: EngineSnapshot, t: TriggerInput): string {
  if (t.type === "encoderStep") {
    const e = s.encoders.find((e) => e.id === t.encoder);
    return `${e?.name ?? `Missing encoder ${t.encoder}`} ${t.direction === "increment" ? "+" : "−"}`;
  }
  const device = deviceTitle(findDevice(s, t.device), t.device);
  if (t.type === "button") return `${device} · ${controlName({ type: "button", index: t.index })}`;
  if (t.type === "axisZone") return `${device} · ${controlName({ type: "axis", index: t.index })} ${t.min}–${t.max} %`;
  return `${device} · ${controlName({ type: "hat", index: t.index })} ${hatDirectionText(t.direction)}`;
}

export function buttonRefText(s: EngineSnapshot, b: ButtonRef): string {
  return `${deviceTitle(findDevice(s, b.device), b.device)} · ${controlName({ type: "button", index: b.index })}`;
}

/** E.g. " (long 500 ms)"; nothing for a plain press. */
export function pressText(t: Trigger): string {
  const zone = t.type === "axisZone";
  switch (t.press.kind) {
    case "press":
      return "";
    case "release":
      return zone ? " (leaving)" : " (release)";
    case "long":
      return ` (long ${t.press.ms} ms)`;
    case "double":
      return ` (double ${t.press.ms} ms)`;
  }
}

/** "Layer = Combat"; a mode or state that is gone says so. */
export function conditionText(s: EngineSnapshot, c: Condition): string {
  const mode = s.virtualControls.find((v) => v.id === c.mode);
  if (!mode || mode.kind.type !== "mode") return "missing mode";
  const state = mode.kind.states.find((st) => st.id === c.state);
  return `${mode.name} = ${state?.name ?? "missing state"}`;
}

/** Input, press kind, modifiers and conditions, e.g. "Stick · Button 2 + Stick · Button 4 (long 500 ms) · when Layer = Combat". */
export function triggerText(s: EngineSnapshot, t: Trigger): string {
  const mods = t.modifiers.map((m) => `${buttonRefText(s, m)} + `).join("");
  const conds = t.conditions.length ? ` · when ${t.conditions.map((c) => conditionText(s, c)).join(" and ")}` : "";
  return `${mods}${inputText(s, t)}${pressText(t)}${conds}`;
}

/** A trigger with defaults: plain press, no modifiers, no conditions. */
export function newTrigger(input: TriggerInput): Trigger {
  return { ...input, press: { kind: "press" }, modifiers: [], conditions: [] };
}

/** Whether the trigger's input is a button or hat direction (it has press kinds). */
export const isDigital = (t: TriggerInput) => t.type === "button" || t.type === "hat";

/** Same button or hat direction. */
export function sameInput(a: TriggerInput, b: TriggerInput): boolean {
  if (a.type !== b.type) return false;
  if (a.type === "button" && b.type === "button") return a.device === b.device && a.index === b.index;
  if (a.type === "hat" && b.type === "hat") return a.device === b.device && a.index === b.index && a.direction === b.direction;
  return JSON.stringify(a) === JSON.stringify(b);
}

export const CONTROL_TYPES: [ControlType, string, string][] = [
  ["radio", "Radio", "several states, at most one active"],
  ["toggle", "Toggle", "on or off"],
  ["momentary", "Momentary", "on while held"],
  ["counter", "Counter", "a value with range and step"],
  ["mode", "Mode", "a layer: exactly one state; triggers can depend on it"],
];

export function sameTrigger(a: Trigger, b: Trigger): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

const hold = (ms: number | null) => (ms === null ? "" : ` ${ms} ms`);

export function keyStepText(s: KeyStep): string {
  switch (s.type) {
    case "keyDown":
      return `↓ ${s.key}`;
    case "keyUp":
      return `↑ ${s.key}`;
    case "keyTap":
      return `${s.key}${hold(s.holdMs)}`;
    case "delay":
      return `wait ${s.ms} ms`;
  }
}

/** A fraction as a percentage, e.g. "50.0%". */
export const pct = (v: number) => `${(v * 100).toFixed(1)}%`;

export function mouseStepText(s: MouseStep): string {
  switch (s.type) {
    case "buttonDown":
      return `↓ ${s.button}`;
    case "buttonUp":
      return `↑ ${s.button}`;
    case "click":
      return `click ${s.button}${hold(s.holdMs)}`;
    case "wheel":
      return `wheel ${s.delta > 0 ? "+" : ""}${s.delta}`;
    case "moveRelative":
      return `move ${s.dx}, ${s.dy}`;
    case "moveTo":
      return `to ${pct(s.x)}, ${pct(s.y)}`;
    case "movePath":
      return `path (${s.points.length} points, ${s.points.at(-1)?.t ?? 0} ms)`;
    case "moveRelativePath":
      return `relative path (${s.points.length} points, ${s.points.at(-1)?.t ?? 0} ms)`;
    case "delay":
      return `wait ${s.ms} ms`;
  }
}

const OUTPUT_TEXT: Record<SoundOutput, string> = { both: "", speakers: " (speakers)", mic: " (mic)" };

export function soundpadText(c: SoundpadCommand): string {
  switch (c.type) {
    case "play":
      return `play “${c.sound.title || "no sound chosen"}”${OUTPUT_TEXT[c.output]}`;
    case "playRandom":
      return `play random${OUTPUT_TEXT[c.output]}`;
    case "playNext":
      return "play next";
    case "playPrevious":
      return "play previous";
    case "stop":
      return "stop";
    case "togglePause":
      return "pause / resume";
  }
}

export const DISCORD_COMMANDS: [DiscordCommand, string][] = [
  ["mute", "Mute"],
  ["unmute", "Unmute"],
  ["toggleMute", "Toggle mute"],
  ["deafen", "Deafen"],
  ["undeafen", "Undeafen"],
  ["toggleDeafen", "Toggle deafen"],
];

export function discordText(c: DiscordCommand): string {
  return (DISCORD_COMMANDS.find(([k]) => k === c)?.[1] ?? c).toLowerCase();
}

/** One-line summary, e.g. "Keys: ↓ LeftCtrl, F1, ↑ LeftCtrl". */
/** E.g. "next profile" or "\"DCS\"" (by name; a deleted target says so). */
export function profileTargetText(t: ProfileTarget, profiles: ProfileView[] = []): string {
  switch (t.type) {
    case "next":
      return "next profile";
    case "previous":
      return "previous profile";
    case "profile": {
      const p = profiles.find((x) => x.id === t.profile);
      return p ? `"${p.name}"` : "deleted profile";
    }
  }
}

/** vJoy axis names as the driver configuration shows them. */
export const VJOY_AXES: [VjoyAxis, string][] = [
  ["x", "X"],
  ["y", "Y"],
  ["z", "Z"],
  ["rx", "Rx"],
  ["ry", "Ry"],
  ["rz", "Rz"],
  ["slider0", "Slider 1"],
  ["slider1", "Slider 2"],
];

export const vjoyAxisName = (a: VjoyAxis) => VJOY_AXES.find(([v]) => v === a)?.[1] ?? a;

/** E.g. "pulse vJoy 1 button 3 (100 ms)", "vJoy 1 axis X to 50 %". */
export function vjoyCommandText(c: VjoyCommand): string {
  switch (c.type) {
    case "button": {
      const b = `vJoy ${c.button.device} button ${c.button.button}`;
      return c.op.type === "pulse" ? `pulse ${b} (${c.op.ms} ms)` : `${c.op.type} ${b}`;
    }
    case "axis":
      return `vJoy ${c.axis.device} axis ${vjoyAxisName(c.axis.axis)} to ${c.percent} %`;
    case "pov":
      return `vJoy ${c.pov.device} POV ${c.pov.pov} ${c.direction ? hatDirectionText(c.direction) : "centre"}`;
  }
}

export function actionText(a: Action, profiles: ProfileView[] = []): string {
  if (a.type === "vjoy") return `vJoy: ${vjoyCommandText(a.command)}`;
  if (a.type === "switchProfile") return `Switch profile: ${profileTargetText(a.target, profiles)}`;
  if (a.type === "soundpad") return `Soundpad: ${soundpadText(a.command)}`;
  if (a.type === "discord") return `Discord: ${discordText(a.command)}`;
  const steps = a.type === "keyboardMacro" ? a.steps.map(keyStepText) : a.steps.map(mouseStepText);
  const speed = a.playback.speedPercent === 100 ? "" : ` (${a.playback.speedPercent}%)`;
  return `${a.type === "keyboardMacro" ? "Keys" : "Mouse"}${speed}: ${steps.join(", ") || "no steps"}`;
}

// --- Event log ---------------------------------------------------------------------------------

/** Local time with milliseconds, e.g. "14:03:07.042". */
export function diagTime(timeMs: number): string {
  const d = new Date(timeMs);
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

/** One entry as a line of plain text (for copying). */
export function diagLine(e: DiagEntry): string {
  const device = e.deviceName ? ` ${e.deviceName}:` : "";
  const level = e.level === "info" ? "" : ` ${e.level.toUpperCase()}`;
  const detail = e.detail ? `  (${e.detail})` : "";
  return `${diagTime(e.timeMs)} [${e.category}]${level}${device} ${e.text}${detail}`;
}

/** All trigger lists of a control. */
export function triggerLists(kind: VirtualControlKind): Trigger[][] {
  switch (kind.type) {
    case "radio":
      return [kind.increment, kind.decrement, kind.clear, kind.toggleOff, ...kind.states.map((s) => s.select)];
    case "toggle":
      return [kind.toggle, kind.on, kind.off];
    case "momentary":
      return [kind.triggers];
    case "counter":
      return [kind.increment, kind.decrement, kind.reset];
    case "mode":
      return [kind.next, kind.previous, ...kind.states.flatMap((s) => [s.select, s.shift])];
  }
}

/** All actions of a control. */
export function controlActions(kind: VirtualControlKind): Action[] {
  switch (kind.type) {
    case "radio":
    case "mode":
      return kind.states.flatMap((s) => [...s.onActivate, ...s.onDeactivate]);
    case "toggle":
      return [...kind.onOn, ...kind.onOff];
    case "momentary":
      return [...kind.onPress, ...kind.onRelease];
    case "counter":
      return kind.onChange;
  }
}

/** Same as detent-core's AxisCurve::apply: shapes an axis value (-1..1) on its way to vJoy. */
export function applyCurve(c: AxisCurve, value: number): number {
  let v = Number.isFinite(value) ? Math.max(-1, Math.min(1, value)) : 0;
  if (c.invert) v = -v;
  const lo = -1 + (2 * c.deadzoneLow) / 100;
  const hi = 1 - (2 * c.deadzoneHigh) / 100;
  if ((c.deadzoneLow > 0 || c.deadzoneHigh > 0) && hi > lo) v = Math.max(-1, Math.min(1, ((v - lo) / (hi - lo)) * 2 - 1));
  const dc = Math.max(0, Math.min(0.99, c.deadzoneCenter / 100));
  v = Math.abs(v) <= dc ? 0 : (Math.sign(v) * (Math.abs(v) - dc)) / (1 - dc);
  if (c.curve.type === "expo") {
    const k = Math.max(0, Math.min(1, c.curve.factor));
    v = (1 - k) * v + k * v * v * v;
  }
  return Math.max(-1, Math.min(1, v * Math.max(0, Math.min(1, c.saturation / 100))));
}

export const defaultCurve = (): AxisCurve => ({ invert: false, deadzoneCenter: 0, deadzoneLow: 0, deadzoneHigh: 0, saturation: 100, curve: { type: "linear" } });

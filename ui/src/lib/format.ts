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
  SoundOutput,
  SoundpadCommand,
  Trigger,
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

/** E.g. "Demo Stick · Button 4", "Radio knob +", "Demo Stick · Hat 1 ↑ up". */
export function triggerText(s: EngineSnapshot, t: Trigger): string {
  if (t.type === "encoderStep") {
    const e = s.encoders.find((e) => e.id === t.encoder);
    return `${e?.name ?? `Missing encoder ${t.encoder}`} ${t.direction === "increment" ? "+" : "−"}`;
  }
  const device = deviceTitle(findDevice(s, t.device), t.device);
  if (t.type === "button") return `${device} · ${controlName({ type: "button", index: t.index })}`;
  return `${device} · ${controlName({ type: "hat", index: t.index })} ${hatDirectionText(t.direction)}`;
}

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
export function actionText(a: Action): string {
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

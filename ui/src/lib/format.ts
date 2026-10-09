import type { ControlRef, DeviceSnapshot, EngineSnapshot, IdentityConfidence } from "./types";

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

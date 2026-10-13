// Turns the overlay appearance settings into CSS custom properties. The overlay window, the
// toasts and the preview in Settings all take their colors and sizes from these, so a theme
// change needs no other code. Nothing here runs per frame: the style string changes only when
// the settings do.

import type { Appearance, Theme } from "./types";

interface Palette {
  /** Background of widgets, r, g, b. */
  bg: [number, number, number];
  text: string;
  muted: string;
  accent: string;
  lampOff: string;
  /** Opacity of the accent-colored borders. */
  borderAlpha: number;
}

const PALETTES: Record<Theme, Palette> = {
  dark: { bg: [12, 15, 19], text: "#e6e9ee", muted: "#8b95a5", accent: "#f0a030", lampOff: "#2a313b", borderAlpha: 0.35 },
  light: { bg: [245, 247, 250], text: "#1b2330", muted: "#566173", accent: "#b85f00", lampOff: "#c5ccd6", borderAlpha: 0.45 },
  highContrast: { bg: [0, 0, 0], text: "#ffffff", muted: "#d4d4d4", accent: "#ffee00", lampOff: "#4a4a4a", borderAlpha: 1 },
  hudGreen: { bg: [2, 12, 6], text: "#b9ffcf", muted: "#5fae78", accent: "#39ff6a", lampOff: "#12381f", borderAlpha: 0.5 },
};

export const THEMES: [Theme, string][] = [
  ["dark", "Dark"],
  ["light", "Light"],
  ["highContrast", "High contrast"],
  ["hudGreen", "HUD green"],
];

export function parseHex(color: string): [number, number, number] | null {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(color);
  return m ? [parseInt(m[1]!, 16), parseInt(m[2]!, 16), parseInt(m[3]!, 16)] : null;
}

/** Black or white, whichever reads better on `color` (the highlight of the active state). */
export function textOn(color: string): string {
  const rgb = parseHex(color) ?? [255, 255, 255];
  const lin = rgb.map((v) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  const luminance = 0.2126 * lin[0]! + 0.7152 * lin[1]! + 0.0722 * lin[2]!;
  return luminance > 0.4 ? "#111111" : "#ffffff";
}

const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(Number.isFinite(v) ? v : lo, lo), hi);

/** The CSS custom properties for `appearance`, as a `style` attribute value. */
export function overlayStyle(appearance: Appearance): string {
  const palette = PALETTES[appearance.theme] ?? PALETTES.dark;
  const accent = appearance.accent && parseHex(appearance.accent) ? appearance.accent : palette.accent;
  const [r, g, b] = palette.bg;
  const [ar, ag, ab] = parseHex(accent)!;
  const radius = clamp(appearance.cornerRadius, 0, 32);
  const vars: Record<string, string | number> = {
    "--accent": accent,
    "--accent-text": textOn(accent),
    "--text": palette.text,
    "--muted": palette.muted,
    "--lamp-on": accent,
    "--lamp-off": palette.lampOff,
    "--ov-bg": `rgba(${r}, ${g}, ${b}, ${clamp(appearance.backgroundOpacity, 0, 100) / 100})`,
    "--ov-border": `rgba(${ar}, ${ag}, ${ab}, ${palette.borderAlpha})`,
    "--ov-line": `rgba(${ar}, ${ag}, ${ab}, ${Math.min(1, palette.borderAlpha * 0.7)})`,
    "--ov-radius": `${radius}px`,
    "--ov-radius-sm": `${Math.round(radius / 2)}px`,
    "--text-scale": clamp(appearance.textScale, 0.5, 2.5),
  };
  return Object.entries(vars)
    .map(([k, v]) => `${k}:${v}`)
    .join(";");
}

// Shared state of the macro recorder in this window: which editor started the recording,
// and countdowns (the backend does the real timing; these only display it).

import type { MonitorRef, MacroStep } from "./types";

/** Same as detent-core's recording::COUNTDOWN_MS. */
export const COUNTDOWN_MS = 3000;

export type RecordMode = "append" | "replace";

/** The macro editor that started the running recording, and what to do with the result. */
export const recordingOwner = $state<{ id: number | null; mode: RecordMode }>({ id: null, mode: "append" });

let nextEditorId = 1;
export const newEditorId = () => nextEditorId++;

/**
 * Seconds left while `active()` is true, counted down from COUNTDOWN_MS. Call during
 * component setup.
 */
export function countdown(active: () => boolean): { left: number } {
  const state = $state({ left: 0 });
  $effect(() => {
    if (!active()) {
      state.left = 0;
      return;
    }
    const start = Date.now();
    const tick = () => (state.left = Math.max(0, Math.ceil((COUNTDOWN_MS - (Date.now() - start)) / 1000)));
    tick();
    const timer = setInterval(tick, 100);
    return () => clearInterval(timer);
  });
  return state;
}

/**
 * Re-expresses absolute positions recorded on monitor `from` as fractions of monitor `to`,
 * so steps recorded on another monitor can be appended to a macro.
 */
export function reproject(steps: MacroStep[], from: MonitorRef, to: MonitorRef): MacroStep[] {
  const fx = (x: number) => (from.x + x * from.width - to.x) / to.width;
  const fy = (y: number) => (from.y + y * from.height - to.y) / to.height;
  return steps.map((s) => {
    if (s.type === "moveTo") return { ...s, x: fx(s.x), y: fy(s.y) };
    if (s.type === "movePath") return { ...s, points: s.points.map((p) => ({ x: fx(p.x), y: fy(p.y), t: p.t })) };
    return s;
  });
}

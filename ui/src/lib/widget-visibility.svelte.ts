// "Only after a change" widgets: a widget with this mode is hidden until what it shows changes,
// stays for its number of seconds, then fades out (the fade is a CSS transition). One timer per
// widget that is currently shown; no loops, nothing runs while nothing changes.

import type { EngineSnapshot, OverlayItem } from "./types";

/** Identifies what a widget shows, so the same widget keeps its state when others change. */
export function widgetKey(w: OverlayItem): string {
  switch (w.type) {
    case "profileName":
      return "profile";
    case "function":
      return `fn:${w.function}`;
    case "radio":
      return `vc:${w.control}`;
    case "control":
      return `in:${w.device}:${w.control.type}:${w.control.index}`;
  }
}

/** A string that changes when what the widget shows changes (axes in 2 % steps, so noise does not count). */
export function widgetSignature(s: EngineSnapshot, w: OverlayItem): string {
  switch (w.type) {
    case "profileName":
      return s.profiles.find((p) => p.id === s.activeProfile)?.name ?? "";
    case "function":
      return s.functions.find((f) => f.id === w.function)?.display ?? "";
    case "radio": {
      const c = s.virtualControls.find((c) => c.id === w.control);
      if (!c) return "";
      if (c.kind.type === "counter") return c.display ?? "";
      if (c.kind.type === "toggle" || c.kind.type === "momentary") return String(c.on);
      return `${c.active ?? ""}|${c.shifted}`;
    }
    case "control": {
      const d = s.devices.find((d) => d.id === w.device);
      if (!d) return "gone";
      const i = w.control.index;
      switch (w.control.type) {
        case "axis":
          return `${d.connected}:${Math.round((d.state.axes[i] ?? 0) * 50)}`;
        case "button":
          return `${d.connected}:${d.state.buttons[i] ?? false}`;
        case "hat":
          return `${d.connected}:${d.state.hats[i] ?? 0}`;
      }
    }
  }
}

export class AfterChange {
  /** Widgets (by key) currently shown because they changed. */
  shown = $state<Record<string, boolean>>({});
  #signatures = new Map<string, string>();
  #timers = new Map<string, ReturnType<typeof setTimeout>>();
  #profile: string | null = null;

  /** Call with every snapshot. */
  update(s: EngineSnapshot) {
    if (s.activeProfile !== this.#profile) {
      // Another profile's widgets: their first look is a baseline, not a change.
      this.#profile = s.activeProfile;
      this.reset();
    }
    const live = new Set<string>();
    for (const w of s.overlay.widgets) {
      if (w.visibility?.mode !== "afterChange") continue;
      const key = widgetKey(w);
      live.add(key);
      const signature = widgetSignature(s, w);
      const before = this.#signatures.get(key);
      this.#signatures.set(key, signature);
      if (before === undefined || before === signature) continue;
      clearTimeout(this.#timers.get(key));
      this.shown[key] = true;
      this.#timers.set(
        key,
        setTimeout(() => {
          this.shown[key] = false;
          this.#timers.delete(key);
        }, Math.max(1, w.visibility.seconds) * 1000),
      );
    }
    for (const key of [...this.#signatures.keys()]) {
      if (live.has(key)) continue;
      this.#signatures.delete(key);
      clearTimeout(this.#timers.get(key));
      this.#timers.delete(key);
      delete this.shown[key];
    }
  }

  reset() {
    this.#timers.forEach((t) => clearTimeout(t));
    this.#timers.clear();
    this.#signatures.clear();
    this.shown = {};
  }
}

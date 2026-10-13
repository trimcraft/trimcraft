// The main window's current section; components can switch it (e.g. "enable it under
// Connections"). Remembered across restarts as a convenience.

export type Section = "controls" | "connections" | "hardware";

export const SECTIONS: [Section, string][] = [
  ["controls", "Virtual Controls"],
  ["connections", "Connections"],
  ["hardware", "Hardware Test"],
];

const KEY = "detent.section";
const DEFAULT: Section = "controls";

function load(): Section {
  try {
    const saved = localStorage.getItem(KEY);
    return SECTIONS.some(([s]) => s === saved) ? (saved as Section) : DEFAULT;
  } catch {
    return DEFAULT;
  }
}

export const nav = $state<{ section: Section }>({ section: load() });

export function show(section: Section) {
  nav.section = section;
  try {
    localStorage.setItem(KEY, section);
  } catch {
    // Only a convenience; the app works without it.
  }
}

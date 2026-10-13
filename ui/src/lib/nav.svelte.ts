// The main window's current section; components can switch it (e.g. "enable it under
// Connections"). Remembered across restarts as a convenience.

// "settings" is opened with the gear in the header, not from the navigation.
export type Section = "controls" | "connections" | "hardware" | "settings";

export const SECTIONS: [Section, string][] = [
  ["controls", "Virtual Controls"],
  ["connections", "Connections"],
  ["hardware", "Hardware Test"],
];

const KEY = "trimcraft.section";
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

/** The Settings page's section; the header's update badge opens "updates". */
export const settingsNav = $state<{ section: string }>({ section: "general" });

export function showSettings(section: string) {
  settingsNav.section = section;
  show("settings");
}

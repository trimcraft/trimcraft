// Settings > About and the Credits page.
import type { AboutLink } from "./types";

/** The URLs behind AboutLink, for the browser demo only. The app opens them in Rust
 *  (src-tauri/src/commands.rs, AboutLink and SUPPORT_URLS, the one place to set the donation
 *  pages); keep these in step. */
export const ABOUT_URLS: Record<AboutLink, string> = {
  repository: "https://github.com/trimcraft/trimcraft",
  issues: "https://github.com/trimcraft/trimcraft/issues/new/choose",
  license: "https://www.gnu.org/licenses/gpl-3.0.html",
  githubSponsors: "",
  koFi: "",
};

/** Button texts of the donation pages. */
export const SUPPORT_LABELS: Partial<Record<AboutLink, string>> = {
  githubSponsors: "GitHub Sponsors",
  koFi: "Ko-fi",
};

/** Open source software Trimcraft is built on, for the Credits page (all of it, with
 *  licences, is in THIRD_PARTY_LICENSES). */
export const LIBRARIES: [name: string, what: string][] = [
  ["Tauri", "desktop app framework, windows and tray"],
  ["SDL 3", "joystick and HID input"],
  ["Svelte, Vite", "user interface"],
  ["serde", "settings files and messages"],
  ["windows-rs", "Windows APIs: input, audio, foreground window"],
  ["keyring", "Windows Credential Manager"],
  ["rodio", "sound files"],
  ["ureq", "HTTP requests and Discord sign-in"],
];

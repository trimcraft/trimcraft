// Soundpad reachability, polled while at least one component shows it.

import { backend, errorText } from "./backend";
import type { SoundpadStatus } from "./types";

const POLL_MS = 3000;

export const soundpad = $state<{ status: SoundpadStatus | null }>({ status: null });

let watchers = 0;
let timer: ReturnType<typeof setInterval> | undefined;

export async function refreshSoundpad() {
  try {
    soundpad.status = await (await backend()).soundpadStatus();
  } catch (e) {
    soundpad.status = { alive: false, error: errorText(e), lastWarning: null };
  }
}

/** Starts polling; returns the function that stops it (usable as an `$effect` cleanup). */
export function watchSoundpad(): () => void {
  if (watchers++ === 0) {
    refreshSoundpad();
    timer = setInterval(refreshSoundpad, POLL_MS);
  }
  return () => {
    if (--watchers === 0) clearInterval(timer);
  };
}

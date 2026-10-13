// "Learn" (Assign) in the control editors: one trigger slot or modifier list at a time asks
// the engine for the next input; VirtualControlsView hands the captured trigger to it.

import type { Trigger } from "./types";
import { backend } from "./backend";

/** Which slot is learning (a key the slot chooses), and what to do with the capture. */
export const learning = $state<{ key: string | null }>({ key: null });
let deliver: ((t: Trigger) => void) | null = null;

export async function startLearn(key: string, onCapture: (t: Trigger) => void) {
  learning.key = key;
  deliver = onCapture;
  await (await backend()).startLearn();
}

export async function stopLearn() {
  learning.key = null;
  deliver = null;
  await (await backend()).stopLearn();
}

/** Called with the engine's captured trigger: gives it to the slot and leaves learn mode. */
export function captured(t: Trigger) {
  const target = deliver;
  stopLearn();
  target?.(t);
}

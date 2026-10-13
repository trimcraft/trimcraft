// State of the "Event log" tab. It lives outside the component, so the filters, the pause and
// the entries survive switching between the tabs; the backend keeps the entries meanwhile.

import { backend } from "./backend";
import type { DiagCategory, DiagEntry } from "./types";

/** Same as the backend's buffer. */
const CAPACITY = 5000;

export const CATEGORIES: [DiagCategory, string][] = [
  ["input", "Input"],
  ["trigger", "Triggers"],
  ["control", "Controls"],
  ["action", "Actions"],
  ["connector", "Connectors"],
  ["output", "vJoy output"],
  ["log", "Log"],
];

class DiagnosticsLog {
  /** Oldest first; replaced (not mutated) per batch. */
  entries = $state.raw<DiagEntry[]>([]);
  /** Categories switched off. */
  hidden = $state<DiagCategory[]>([]);
  /** Device id, or "" for all. */
  device = $state("");
  search = $state("");
  /** Entries after this sequence number stay out of the view while paused. */
  frozenAt = $state<number | null>(null);

  #started = false;
  #lastSeq = 0;

  /** Subscribes once; the subscription lasts as long as the window. */
  async start() {
    if (this.#started) return;
    this.#started = true;
    const b = await backend();
    await b.subscribeDiagnostics((batch) => this.#ingest(batch));
  }

  #ingest(batch: DiagEntry[]) {
    // The buffer and the live batches overlap right after subscribing.
    const fresh = batch.filter((e) => e.seq > this.#lastSeq);
    if (!fresh.length) return;
    this.#lastSeq = fresh.at(-1)!.seq;
    const all = this.entries.length ? this.entries.concat(fresh) : fresh;
    this.entries = all.length > CAPACITY ? all.slice(all.length - CAPACITY) : all;
  }

  get paused() {
    return this.frozenAt !== null;
  }

  /** Entries that arrived after the pause began. */
  get pending() {
    return this.frozenAt === null ? 0 : this.entries.filter((e) => e.seq > this.frozenAt!).length;
  }

  pause() {
    this.frozenAt = this.#lastSeq;
  }

  resume() {
    this.frozenAt = null;
  }

  async clear() {
    this.entries = [];
    if (this.frozenAt !== null) this.frozenAt = this.#lastSeq;
    await (await backend()).clearDiagnostics();
  }

  toggleCategory(c: DiagCategory) {
    this.hidden = this.hidden.includes(c) ? this.hidden.filter((x) => x !== c) : [...this.hidden, c];
  }

  /** What the filters and the pause leave of the entries. */
  get visible(): DiagEntry[] {
    const q = this.search.trim().toLowerCase();
    const frozen = this.frozenAt;
    return this.entries.filter((e) => {
      if (frozen !== null && e.seq > frozen) return false;
      if (this.hidden.includes(e.category)) return false;
      if (this.device && e.device !== this.device) return false;
      if (!q) return true;
      return `${e.category} ${e.deviceName ?? ""} ${e.text} ${e.detail ?? ""}`.toLowerCase().includes(q);
    });
  }

  /** Devices that appear in the log, for the filter. */
  get devices(): [string, string][] {
    const seen = new Map<string, string>();
    for (const e of this.entries) if (e.device && !seen.has(e.device)) seen.set(e.device, e.deviceName ?? e.device);
    return [...seen].sort((a, b) => a[1].localeCompare(b[1]));
  }
}

export const diagnostics = new DiagnosticsLog();

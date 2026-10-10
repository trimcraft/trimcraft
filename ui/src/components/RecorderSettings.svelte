<script lang="ts">
  // Settings of the macro recorder: stop hotkey and how mouse movement is simplified.
  import type { EngineSnapshot, RecorderConfig } from "../lib/types";
  import { backend, errorText } from "../lib/backend";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let draft = $state<RecorderConfig>({ stopHotkey: "", tolerance: 2, sampleIntervalMs: 8 });
  let error = $state("");
  let saved = $state(false);

  // Follow the engine's settings until the user changes something.
  let dirty = $state(false);
  $effect(() => {
    const config = snapshot.recorderConfig;
    if (!dirty) draft = { ...config };
  });

  async function save() {
    error = "";
    try {
      await (await backend()).setRecorderConfig($state.snapshot(draft));
      dirty = false;
      saved = true;
      setTimeout(() => (saved = false), 1500);
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

<section class="panel">
  <h2>Macro recorder</h2>
  <p class="muted small">
    Record a keyboard or mouse macro with <b>● Record</b> in its editor. After a 3 s countdown everything you type (or
    click and move) is recorded until you press the stop hotkey or Stop; Esc cancels. Absolute mouse paths are thinned
    to the sample interval and simplified so they deviate at most by the tolerance.
  </p>
  <div class="row" oninput={() => (dirty = true)}>
    <label>Stop hotkey <input class="mono" bind:value={draft.stopHotkey} placeholder="Ctrl+Shift+F9" /></label>
    <label>Tolerance <input type="number" min="0" max="100" step="0.5" bind:value={draft.tolerance} /> px</label>
    <label>Sample at most every <input type="number" min="1" max="100" bind:value={draft.sampleIntervalMs} /> ms</label>
    <button class="primary" disabled={!dirty} onclick={save}>Save</button>
    {#if saved}<span class="muted small">saved</span>{/if}
  </div>
  {#if error}<p class="error small">{error}</p>{/if}
</section>

<style>
  label { display: inline-flex; gap: 6px; align-items: center; color: var(--muted); font-size: 12px; }
  .small { font-size: 12px; }
  button:disabled { opacity: 0.5; cursor: default; }
</style>

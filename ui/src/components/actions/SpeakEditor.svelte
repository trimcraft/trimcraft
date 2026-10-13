<script lang="ts">
  // Edits a "Speak" action: text with placeholders, voice, rate and volume.
  import type { SpeakAction, VoiceRef } from "../../lib/types";
  import { backend, errorText } from "../../lib/backend";
  import { PLACEHOLDERS } from "../../lib/format";
  import RunOptions from "./RunOptions.svelte";

  let { action = $bindable() }: { action: SpeakAction } = $props();

  let voices = $state<VoiceRef[]>([]);
  let loadError = $state("");

  async function loadVoices() {
    loadError = "";
    try {
      voices = await (await backend()).listVoices();
    } catch (e) {
      loadError = errorText(e);
    }
  }
  loadVoices();

  // The stored voice stays selectable even when it is not installed (any more).
  let options = $derived(action.voice && !voices.some((v) => v.id === action.voice?.id) ? [action.voice, ...voices] : voices);
  let voiceId = $derived(action.voice?.id ?? "");

  function setVoice(id: string) {
    action.voice = options.find((v) => v.id === id) ?? null;
  }
</script>

<div class="editor">
  <textarea rows="2" bind:value={action.text} placeholder="e.g. Channel {'{state}'} on {'{control}'}"></textarea>
  <p class="muted small">Placeholders: {PLACEHOLDERS.join(", ")}. A new speech interrupts the one still speaking.</p>
  <div class="row">
    <label>Voice
      <select value={voiceId} onchange={(e) => setVoice(e.currentTarget.value)}>
        <option value="">Windows default</option>
        {#each options as v (v.id)}<option value={v.id}>{v.name}{voices.includes(v) ? "" : " (not installed)"}</option>{/each}
      </select>
    </label>
    <label>Rate <input type="range" min="-10" max="10" step="1" bind:value={action.rate} /> <span class="mono">{action.rate}</span></label>
    <label>Volume <input type="number" min="0" max="100" bind:value={action.volume} /> %</label>
  </div>
  {#if loadError}<p class="error small">{loadError}</p>{/if}
  <RunOptions {action} bind:wait={action.wait} />
</div>

<style>
  .editor { display: flex; flex-direction: column; gap: 6px; font-size: 12px; width: 100%; }
  textarea { width: 100%; font: inherit; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; padding: 4px 6px; }
  label { display: inline-flex; gap: 4px; align-items: center; color: var(--muted); }
  .small { font-size: 11px; margin: 0; }
  select { max-width: 24em; }
</style>

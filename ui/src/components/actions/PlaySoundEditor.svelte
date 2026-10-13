<script lang="ts">
  // Edits a "Play sound file" action: file, volume, output device, stop previous.
  import type { PlaySoundAction } from "../../lib/types";
  import { backend, errorText } from "../../lib/backend";
  import RunOptions from "./RunOptions.svelte";

  let { action = $bindable() }: { action: PlaySoundAction } = $props();

  let devices = $state<string[]>([]);
  let loadError = $state("");

  async function loadDevices() {
    loadError = "";
    try {
      devices = await (await backend()).listSoundDevices();
    } catch (e) {
      loadError = errorText(e);
    }
  }
  loadDevices();

  let options = $derived(action.device && !devices.includes(action.device) ? [action.device, ...devices] : devices);
</script>

<div class="editor">
  <label class="wide">File
    <input class="mono path" bind:value={action.path} placeholder="C:\Sounds\alert.wav (wav, mp3, ogg, flac)" />
  </label>
  <p class="muted small">Paste the full path, e.g. with Shift + right click on the file, "Copy as path".</p>
  <div class="row">
    <label>Volume <input type="number" min="0" max="100" bind:value={action.volume} /> %</label>
    <label>Output
      <select value={action.device ?? ""} onchange={(e) => (action.device = e.currentTarget.value || null)}>
        <option value="">Windows default</option>
        {#each options as d (d)}<option value={d}>{d}{devices.includes(d) ? "" : " (not connected)"}</option>{/each}
      </select>
    </label>
    <label class="check"><input type="checkbox" bind:checked={action.stopPrevious} /> Stop sounds still playing</label>
  </div>
  {#if loadError}<p class="error small">{loadError}</p>{/if}
  <RunOptions {action} bind:wait={action.wait} />
</div>

<style>
  .editor { display: flex; flex-direction: column; gap: 6px; font-size: 12px; width: 100%; }
  label { display: inline-flex; gap: 4px; align-items: center; color: var(--muted); }
  .wide { display: flex; }
  .path { flex: 1; min-width: 20em; }
  .small { font-size: 11px; margin: 0; }
  select { max-width: 24em; }
</style>

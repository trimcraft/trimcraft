<script lang="ts">
  // Shown in the main window's header while a macro recording counts down or runs.
  import type { EngineSnapshot } from "../lib/types";
  import { backend } from "../lib/backend";
  import { countdown } from "../lib/recording.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let phase = $derived(snapshot.recorder.phase);
  const starting = countdown(() => phase === "countdown");
</script>

{#if phase !== "idle"}
  <span class="banner" class:live={phase === "recording"}>
    {#if phase === "countdown"}
      ● Recording starts in {starting.left}…
    {:else}
      ● REC {snapshot.recorder.kind?.keys && snapshot.recorder.kind?.mouse ? "keys + mouse" : snapshot.recorder.kind?.mouse ? "mouse" : "keys"} — {snapshot.recorderConfig.stopHotkey} stops, Esc cancels
    {/if}
    <button class="small" onclick={async () => (await backend()).stopRecording()}>Stop</button>
    <button class="small" onclick={async () => (await backend()).cancelRecording()}>Cancel</button>
  </span>
{/if}

<style>
  .banner { display: inline-flex; gap: 8px; align-items: center; color: var(--bad); font-weight: 600; border: 1px solid var(--bad); border-radius: 12px; padding: 1px 4px 1px 10px; font-size: 12px; }
  .banner.live { background: rgba(229, 83, 75, 0.12); }
  .small { font-size: 11px; padding: 0 6px; }
</style>

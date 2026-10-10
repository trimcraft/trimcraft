<script lang="ts">
  // "Soundpad connected / not reachable", plus the last problem running a Soundpad action.
  import { soundpad, watchSoundpad } from "../lib/soundpad.svelte";

  let { showWarning = true }: { showWarning?: boolean } = $props();

  $effect(() => watchSoundpad());

  let status = $derived(soundpad.status);
</script>

<span class="status">
  {#if !status}
    <span class="badge">Soundpad: checking…</span>
  {:else if status.alive}
    <span class="badge ok">● Soundpad connected</span>
  {:else}
    <span class="badge off" title={status.error ?? ""}>● Soundpad not reachable</span>
    {#if status.error}<span class="muted small">{status.error}</span>{/if}
  {/if}
  {#if showWarning && status?.lastWarning}<span class="warning small">{status.lastWarning}</span>{/if}
</span>

<style>
  .status { display: inline-flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .ok { border-color: var(--ok); color: var(--ok); }
  .small { font-size: 12px; }
  .warning { color: var(--warn); }
</style>

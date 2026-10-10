<script lang="ts">
  // All overlay widgets stacked, as a preview in the main window. The overlay window itself
  // places them (see Overlay.svelte).
  import type { EngineSnapshot } from "../lib/types";
  import OverlayWidget from "./OverlayWidget.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();
</script>

<div class="widgets">
  {#each snapshot.overlay.widgets as w, i (i)}
    <OverlayWidget {snapshot} widget={w} />
  {:else}
    <p class="empty">Nothing on the overlay yet.</p>
  {/each}
</div>

<style>
  .widgets { display: flex; flex-direction: column; gap: 6px; min-width: 260px; align-items: flex-start; }
  .empty { color: var(--muted); font-size: 12px; margin: 0; }
</style>

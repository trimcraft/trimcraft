<script lang="ts">
  // The overlay window: full-screen, transparent and click-through. Only the panel is visible.
  import { onMount } from "svelte";
  import type { EngineSnapshot } from "./lib/types";
  import { backend } from "./lib/backend";
  import OverlayWidgets from "./components/OverlayWidgets.svelte";

  let snapshot = $state<EngineSnapshot | null>(null);

  onMount(() => {
    let stop: (() => void) | undefined;
    backend().then(async (b) => (stop = await b.subscribe((s) => (snapshot = s))));
    return () => stop?.();
  });

  let position = $derived.by(() => {
    if (!snapshot) return "";
    const { anchor, margin } = snapshot.overlay;
    const v = anchor.startsWith("top") ? `top:${margin}px` : `bottom:${margin}px`;
    const h = anchor.endsWith("Left") ? `left:${margin}px` : `right:${margin}px`;
    return `${v};${h}`;
  });
  let origin = $derived(snapshot ? `${snapshot.overlay.anchor.startsWith("top") ? "top" : "bottom"} ${snapshot.overlay.anchor.endsWith("Left") ? "left" : "right"}` : "");
</script>

{#if snapshot && snapshot.overlay.widgets.length}
  <div class="panel" style={position} style:transform="scale({snapshot.overlay.scale})" style:transform-origin={origin}>
    <OverlayWidgets {snapshot} />
  </div>
{/if}

<style>
  :global(body) { background: transparent !important; }
  .panel {
    position: fixed;
    background: rgba(12, 15, 19, 0.72);
    border: 1px solid rgba(240, 160, 48, 0.35);
    border-radius: 8px;
    padding: 10px 14px;
  }
</style>

<script lang="ts">
  // Renders the overlay widgets. Used by the overlay window and as preview in the main window.
  import type { EngineSnapshot, OverlayWidget } from "../lib/types";
  import { controlName, findDevice, hatArrow } from "../lib/format";
  import AxisBar from "./AxisBar.svelte";
  import HatRose from "./HatRose.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  function label(w: Extract<OverlayWidget, { type: "control" }>): string {
    if (w.label) return w.label;
    const d = findDevice(snapshot, w.device);
    return `${d ? (d.label ?? d.name) : w.device} · ${controlName(w.control)}`;
  }
</script>

<div class="widgets">
  {#each snapshot.overlay.widgets as w, i (i)}
    {#if w.type === "function"}
      {@const f = snapshot.functions.find((f) => f.id === w.function)}
      {#if f}<div class="fn"><span class="fname">{f.name}</span> <span class="fvalue">{f.display}</span></div>{/if}
    {:else}
      {@const d = findDevice(snapshot, w.device)}
      {@const offline = !d || !d.connected}
      <div class="ctl" class:offline>
        {#if w.control.type === "axis"}
          <span class="clabel">{label(w)}</span>
          <AxisBar value={d?.state.axes[w.control.index] ?? 0} wide />
        {:else if w.control.type === "button"}
          {@const on = d?.state.buttons[w.control.index] ?? false}
          <span class="lamp" class:on></span><span class="clabel">{label(w)}</span>
        {:else}
          {@const mask = d?.state.hats[w.control.index] ?? 0}
          <HatRose {mask} /><span class="clabel">{label(w)} {hatArrow(mask)}</span>
        {/if}
      </div>
    {/if}
  {:else}
    <p class="empty">Nothing on the overlay yet.</p>
  {/each}
</div>

<style>
  .widgets { display: flex; flex-direction: column; gap: 6px; min-width: 260px; }
  .fn { font-size: 22px; }
  .fname { color: var(--muted); }
  .fvalue { font-weight: 700; color: var(--accent); font-variant-numeric: tabular-nums; }
  .ctl { display: flex; flex-direction: column; gap: 2px; }
  .ctl:has(.lamp), .ctl:has(:global(.rose)) { flex-direction: row; align-items: center; gap: 8px; }
  .ctl.offline { opacity: 0.4; }
  .clabel { font-size: 12px; color: var(--muted); }
  .lamp { width: 14px; height: 14px; border-radius: 50%; background: var(--lamp-off); }
  .lamp.on { background: var(--lamp-on); box-shadow: 0 0 8px var(--lamp-on); }
  .empty { color: var(--muted); font-size: 12px; margin: 0; }
</style>

<script lang="ts">
  // Horizontal bar for a value in -1..1, filled from the left edge.
  let { value, label = "", wide = false }: { value: number; label?: string; wide?: boolean } = $props();
  let pct = $derived(((Math.max(-1, Math.min(1, value)) + 1) / 2) * 100);
</script>

<div class="axis" class:wide class:unlabelled={!label}>
  {#if label}<span class="label">{label}</span>{/if}
  <div class="track"><div class="fill" style:width="{pct}%"></div></div>
  <span class="value">{Math.round(pct)}%</span>
</div>

<style>
  .axis { display: grid; grid-template-columns: auto 1fr 3.5em; gap: 8px; align-items: center; }
  .axis.unlabelled { grid-template-columns: 1fr 3.5em; }
  .label { min-width: 4.5em; color: var(--muted); font-size: 12px; }
  .track { height: 10px; background: var(--lamp-off); border-radius: 5px; overflow: hidden; }
  .wide .track { height: 14px; }
  .fill { height: 100%; background: var(--accent); }
  .value { text-align: right; font-variant-numeric: tabular-nums; font-size: 12px; }
</style>

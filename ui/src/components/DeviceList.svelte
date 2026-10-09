<script lang="ts">
  import type { DeviceSnapshot } from "../lib/types";
  import { hex4 } from "../lib/format";

  let { devices, selected, onselect }: {
    devices: DeviceSnapshot[];
    selected: string | null;
    onselect: (id: string) => void;
  } = $props();
</script>

<div class="list">
  {#each devices as d (d.id)}
    <button class="item" class:selected={d.id === selected} class:offline={!d.connected} onclick={() => onselect(d.id)}>
      <span class="title">{d.label ?? d.name}</span>
      <span class="row">
        <span class="mono muted">{d.id}</span>
        {#if d.kind.type === "virtual"}<span class="badge virtual">{d.kind.driver} · virtual</span>{/if}
        {#if !d.connected}<span class="badge off">disconnected</span>{/if}
        {#if d.confidence === "ambiguous"}<span class="badge warn">check identity</span>{/if}
      </span>
      {#if d.label}<span class="muted small">{d.name}</span>{/if}
      <span class="muted small">{d.state.axes.length} axes · {d.state.buttons.length} buttons · {d.state.hats.length} hats · {hex4(d.fingerprint.vendorId)}:{hex4(d.fingerprint.productId)}</span>
    </button>
  {:else}
    <p class="muted">No game controllers found. Plug one in; it appears here automatically.</p>
  {/each}
</div>

<style>
  .list { display: flex; flex-direction: column; gap: 6px; }
  .item { display: flex; flex-direction: column; align-items: flex-start; gap: 3px; text-align: left; padding: 8px 10px; background: var(--panel-2); }
  .item.selected { border-color: var(--accent); }
  .item.offline { opacity: 0.55; }
  .title { font-weight: 600; }
  .small { font-size: 12px; }
</style>

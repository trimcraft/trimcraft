<script lang="ts">
  // A state output: none, or a vJoy button held while the state is active / pulsed on
  // entering it. For counters: a vJoy axis following the value.
  import type { ButtonOutput, EngineSnapshot, VjoyAxisRef } from "../lib/types";
  import VjoyTarget from "./VjoyTarget.svelte";

  let {
    snapshot,
    label,
    button = $bindable(),
    axis = $bindable(),
    kind = "button",
  }: {
    snapshot: EngineSnapshot;
    label: string;
    button?: ButtonOutput | null;
    axis?: VjoyAxisRef | null;
    kind?: "button" | "axis";
  } = $props();

  const enabled = $derived(snapshot.connections.vjoy.enabled);
  const set = $derived(kind === "button" ? !!button : !!axis);

  function toggle(on: boolean) {
    if (kind === "button") button = on ? { button: { device: 1, button: 1 }, pulseMs: null } : null;
    else axis = on ? { device: 1, axis: "x" } : null;
  }
</script>

{#if enabled || set}
  <div class="output" class:off={!enabled}>
    <span class="label">{label}</span>
    <div class="row">
      <label class="check"><input type="checkbox" checked={set} onchange={(e) => toggle(e.currentTarget.checked)} /> vJoy</label>
      {#if kind === "button" && button}
        <VjoyTarget {snapshot} kind="button" bind:target={button.button} />
        <select value={button.pulseMs === null ? "hold" : "pulse"} onchange={(e) => (button!.pulseMs = e.currentTarget.value === "hold" ? null : 100)}>
          <option value="hold">held while active</option>
          <option value="pulse">pulsed on entering</option>
        </select>
        {#if button.pulseMs !== null}
          <label class="small"><input type="number" min="10" max="5000" step="10" bind:value={button.pulseMs} /> ms</label>
        {/if}
      {:else if kind === "axis" && axis}
        <VjoyTarget {snapshot} kind="axis" bind:target={axis} />
        <span class="muted small">minimum = one end, maximum = the other</span>
      {/if}
      {#if !enabled}<span class="muted small">(vJoy connection disabled: kept, not written)</span>{/if}
    </div>
  </div>
{/if}

<style>
  .output { display: grid; grid-template-columns: 9em 1fr; gap: 4px 8px; align-items: center; padding: 2px 0; }
  .output.off { opacity: 0.6; }
  .label { color: var(--muted); font-size: 12px; }
  .check { display: inline-flex; gap: 4px; align-items: center; font-size: 12px; }
  .small { font-size: 11px; }
  input[type="number"] { width: 5em; }
</style>

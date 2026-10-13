<script lang="ts">
  // Edits one vJoy action: a button (press, release, toggle, pulse), an axis to a value, or a
  // POV direction.
  import type { EngineSnapshot, HatDirection, VjoyCommand } from "../lib/types";
  import { HAT_DIRECTIONS } from "../lib/format";
  import VjoyTarget from "./VjoyTarget.svelte";

  let { command = $bindable(), snapshot }: { command: VjoyCommand; snapshot: EngineSnapshot } = $props();

  function setType(type: VjoyCommand["type"]) {
    const device = command.type === "button" ? command.button.device : command.type === "axis" ? command.axis.device : command.pov.device;
    if (type === "button") command = { type, button: { device, button: 1 }, op: { type: "press" } };
    else if (type === "axis") command = { type, axis: { device, axis: "x" }, percent: 50 };
    else command = { type, pov: { device, pov: 1 }, direction: "up" };
  }

  function setOp(op: string) {
    if (command.type !== "button") return;
    command.op = op === "pulse" ? { type: "pulse", ms: 100 } : { type: op as "press" | "release" | "toggle" };
  }
</script>

<div class="row">
  <select value={command.type} onchange={(e) => setType(e.currentTarget.value as VjoyCommand["type"])}>
    <option value="button">Button</option>
    <option value="axis">Axis</option>
    <option value="pov">POV</option>
  </select>
  {#if command.type === "button"}
    <select value={command.op.type} onchange={(e) => setOp(e.currentTarget.value)}>
      <option value="press">press (hold)</option>
      <option value="release">release</option>
      <option value="toggle">toggle</option>
      <option value="pulse">pulse</option>
    </select>
    <VjoyTarget {snapshot} kind="button" bind:target={command.button} />
    {#if command.op.type === "pulse"}
      <label class="small"><input type="number" min="10" max="5000" step="10" bind:value={command.op.ms} /> ms</label>
    {/if}
  {:else if command.type === "axis"}
    <VjoyTarget {snapshot} kind="axis" bind:target={command.axis} />
    <label class="small">to <input type="number" min="0" max="100" step="1" bind:value={command.percent} /> %</label>
  {:else}
    <VjoyTarget {snapshot} kind="pov" bind:target={command.pov} />
    <select value={command.direction ?? "centre"} onchange={(e) => {
      if (command.type === "pov") command.direction = e.currentTarget.value === "centre" ? null : (e.currentTarget.value as HatDirection);
    }}>
      <option value="centre">centre</option>
      {#each HAT_DIRECTIONS as [value, text] (value)}<option {value}>{text}</option>{/each}
    </select>
  {/if}
</div>
<p class="muted small">
  Takes effect when the action fires (not after earlier macro waits). A pressed button or a set axis stays until another
  action changes it, the profile changes or vJoy is disabled.
</p>

<style>
  .small { font-size: 11px; margin: 0; }
  input[type="number"] { width: 5em; }
  label { display: inline-flex; gap: 4px; align-items: center; color: var(--muted); }
</style>

<script lang="ts">
  // Edits a Voicemeeter action: strip or bus, gain or mute.
  import type { ConnectorView, VmCommand, VmTarget } from "../../lib/types";
  import ConnectorBadge from "../ConnectorBadge.svelte";

  let {
    target = $bindable(),
    command = $bindable(),
    connector,
  }: { target: VmTarget; command: VmCommand; connector: ConnectorView | undefined } = $props();

  const COMMANDS: [VmCommand["type"], string][] = [
    ["setGain", "Set gain to"],
    ["changeGain", "Change gain by"],
    ["mute", "Mute"],
    ["unmute", "Unmute"],
    ["toggleMute", "Toggle mute"],
  ];

  function setCommand(type: VmCommand["type"]) {
    if (type === "setGain") command = { type, db: command.type === "setGain" ? command.db : 0 };
    else if (type === "changeGain") command = { type, deltaDb: command.type === "changeGain" ? command.deltaDb : 3 };
    else command = { type };
  }
</script>

<div class="editor">
  <div class="row">
    <select bind:value={target.kind}>
      <option value="strip">Strip (input)</option>
      <option value="bus">Bus (output)</option>
    </select>
    <label>No. <input type="number" min="1" max="8" value={target.index + 1} oninput={(e) => (target.index = Math.max(0, Math.min(7, (e.currentTarget.valueAsNumber || 1) - 1)))} /></label>
    <select value={command.type} onchange={(e) => setCommand(e.currentTarget.value as VmCommand["type"])}>
      {#each COMMANDS as [value, text] (value)}<option {value}>{text}</option>{/each}
    </select>
    {#if command.type === "setGain"}
      <input type="number" min="-60" max="12" step="0.5" bind:value={command.db} /> dB
    {:else if command.type === "changeGain"}
      <input type="number" min="-72" max="72" step="0.5" bind:value={command.deltaDb} /> dB
    {/if}
    <span class="spacer"></span>
    {#if connector && connector.status.state !== "connected"}<ConnectorBadge {connector} />{/if}
  </div>
  <p class="muted small">Strips and buses are numbered from 1, left to right as in Voicemeeter. Gain goes from -60 to +12 dB.</p>
</div>

<style>
  .editor { display: flex; flex-direction: column; gap: 6px; font-size: 12px; width: 100%; }
  label { display: inline-flex; gap: 4px; align-items: center; color: var(--muted); }
  input[type="number"] { width: 5em; }
  .spacer { flex: 1; }
  .small { font-size: 11px; margin: 0; }
</style>

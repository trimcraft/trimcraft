<script lang="ts">
  // Edits an "Audio volume" action: master or a program's volume; set, change, mute.
  import type { VolumeCommand, VolumeTarget } from "../../lib/types";
  import { backend, errorText } from "../../lib/backend";

  let { target = $bindable(), command = $bindable() }: { target: VolumeTarget; command: VolumeCommand } = $props();

  const COMMANDS: [VolumeCommand["type"], string][] = [
    ["set", "Set to"],
    ["change", "Change by"],
    ["mute", "Mute"],
    ["unmute", "Unmute"],
    ["toggleMute", "Toggle mute"],
  ];

  let programs = $state<string[]>([]);
  let loadError = $state("");
  const listId = `audio-programs-${Math.random().toString(36).slice(2)}`;

  async function loadPrograms() {
    loadError = "";
    try {
      programs = await (await backend()).listAudioPrograms();
    } catch (e) {
      loadError = errorText(e);
    }
  }
  loadPrograms();

  function setTarget(type: VolumeTarget["type"]) {
    target = type === "master" ? { type } : { type, process: target.type === "app" ? target.process : "" };
  }

  function setCommand(type: VolumeCommand["type"]) {
    if (type === "set") command = { type, percent: command.type === "set" ? command.percent : 50 };
    else if (type === "change") command = { type, delta: command.type === "change" ? command.delta : 10 };
    else command = { type };
  }
</script>

<div class="editor">
  <div class="row">
    <select value={target.type} onchange={(e) => setTarget(e.currentTarget.value as VolumeTarget["type"])}>
      <option value="master">Master (default output device)</option>
      <option value="app">Program</option>
    </select>
    {#if target.type === "app"}
      <input class="mono" list={listId} bind:value={target.process} placeholder="Discord.exe" />
      <datalist id={listId}>{#each programs as p (p)}<option value={p}></option>{/each}</datalist>
      <button class="small" onclick={loadPrograms} title="Programs that have an audio session now">Reload list</button>
    {/if}
  </div>
  <div class="row">
    <select value={command.type} onchange={(e) => setCommand(e.currentTarget.value as VolumeCommand["type"])}>
      {#each COMMANDS as [value, text] (value)}<option {value}>{text}</option>{/each}
    </select>
    {#if command.type === "set"}
      <input type="number" min="0" max="100" bind:value={command.percent} /> %
    {:else if command.type === "change"}
      <input type="number" min="-100" max="100" bind:value={command.delta} /> percentage points
    {/if}
  </div>
  {#if target.type === "app"}
    <p class="muted small">Changes every audio session of the program on every output device, like the Windows volume mixer. The program must be playing (or have played) sound.</p>
  {/if}
  {#if loadError}<p class="error small">{loadError}</p>{/if}
</div>

<style>
  .editor { display: flex; flex-direction: column; gap: 6px; font-size: 12px; width: 100%; }
  .small { font-size: 11px; margin: 0; }
</style>

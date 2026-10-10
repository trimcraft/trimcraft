<script lang="ts">
  // Edits one Soundpad action: the command, where to play, and which sound (picked from
  // Soundpad's live list; stored by title and path).
  import type { ConnectorView, Sound, SoundOutput, SoundpadCommand } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import ConnectorBadge from "./ConnectorBadge.svelte";

  let { command = $bindable(), connector }: { command: SoundpadCommand; connector: ConnectorView | undefined } = $props();

  const commandTypes: [SoundpadCommand["type"], string][] = [
    ["play", "Play sound"],
    ["stop", "Stop"],
    ["togglePause", "Pause / resume"],
    ["playRandom", "Play random sound"],
    ["playNext", "Play next sound"],
    ["playPrevious", "Play previous sound"],
  ];
  const outputs: [SoundOutput, string][] = [
    ["both", "speakers and mic"],
    ["speakers", "speakers only (you hear it)"],
    ["mic", "mic only (others hear it)"],
  ];

  let picking = $state(false);
  let sounds = $state<Sound[]>([]);
  let loadError = $state("");
  let loading = $state(false);
  let search = $state("");

  let filtered = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return q ? sounds.filter((s) => s.title.toLowerCase().includes(q) || s.path.toLowerCase().includes(q)) : sounds;
  });

  function setType(type: SoundpadCommand["type"]) {
    const output: SoundOutput = "output" in command ? command.output : "both";
    switch (type) {
      case "play":
        command = { type, sound: command.type === "play" ? command.sound : { title: "", path: "" }, output };
        break;
      case "playRandom":
        command = { type, output };
        break;
      default:
        command = { type };
    }
  }

  async function openPicker() {
    picking = true;
    loading = true;
    loadError = "";
    try {
      sounds = await (await backend()).soundpadSounds();
    } catch (e) {
      loadError = errorText(e);
    } finally {
      loading = false;
    }
  }

  function choose(s: Sound) {
    if (command.type === "play") command.sound = { title: s.title, path: s.path };
    picking = false;
  }
</script>

<div class="soundpad">
  <div class="row">
    <select value={command.type} onchange={(e) => setType(e.currentTarget.value as SoundpadCommand["type"])}>
      {#each commandTypes as [value, text] (value)}<option {value}>{text}</option>{/each}
    </select>
    {#if command.type === "play" || command.type === "playRandom"}
      <label class="muted">on
        <select bind:value={command.output}>
          {#each outputs as [value, text] (value)}<option {value}>{text}</option>{/each}
        </select>
      </label>
    {/if}
    <span class="spacer"></span>
    {#if connector && connector.status.state !== "connected"}<ConnectorBadge {connector} />{/if}
  </div>
  {#if command.type === "play"}
    <div class="row">
      {#if command.sound.title || command.sound.path}
        <span class="chosen">{command.sound.title}</span>
        <span class="muted mono path" title={command.sound.path}>{command.sound.path}</span>
      {:else}
        <span class="error">No sound chosen</span>
      {/if}
      <button class="small" onclick={() => (picking ? (picking = false) : openPicker())}>{picking ? "Close list" : "Choose sound…"}</button>
    </div>
    {#if picking}
      <div class="picker">
        <div class="row">
          <input class="search" placeholder="Search title or file" bind:value={search} />
          <button class="small" onclick={openPicker} disabled={loading}>Reload</button>
          <span class="muted small">{loading ? "loading…" : `${filtered.length} of ${sounds.length}`}</span>
        </div>
        {#if loadError}
          <p class="error small">{loadError}</p>
        {:else}
          <ul>
            {#each filtered as s (s.index)}
              <li>
                <button class="item" class:current={s.path === command.sound.path && s.title === command.sound.title} onclick={() => choose(s)}>
                  <span>{s.title}</span>
                  <span class="muted mono">{s.duration}</span>
                </button>
              </li>
            {:else}
              <li class="muted small">No sounds match.</li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  {/if}
</div>

<style>
  .soundpad { display: flex; flex-direction: column; gap: 6px; font-size: 12px; width: 100%; }
  .spacer { flex: 1; }
  .chosen { font-weight: 600; }
  .path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 40em; }
  .picker { border: 1px solid var(--line); border-radius: 4px; padding: 6px; background: var(--panel); }
  .search { min-width: 18em; }
  ul { list-style: none; margin: 6px 0 0; padding: 0; max-height: 14em; overflow-y: auto; }
  .item { display: flex; justify-content: space-between; width: 100%; border: none; background: none; text-align: left; padding: 2px 6px; }
  .item:hover { background: var(--panel-2); }
  .item.current { color: var(--accent); }
  .small { font-size: 12px; }
  label { display: inline-flex; gap: 4px; align-items: center; }
</style>

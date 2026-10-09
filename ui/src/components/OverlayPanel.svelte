<script lang="ts">
  import type { Anchor, EngineSnapshot } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import OverlayWidgets from "./OverlayWidgets.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let error = $state("");
  let hotkeyDraft = $state<string | null>(null);

  const anchors: [Anchor, string][] = [
    ["topLeft", "Top left"],
    ["topRight", "Top right"],
    ["bottomLeft", "Bottom left"],
    ["bottomRight", "Bottom right"],
  ];

  async function run(action: () => Promise<unknown>) {
    error = "";
    try {
      await action();
    } catch (e) {
      error = errorText(e);
    }
  }

  function layout(change: Partial<{ anchor: Anchor; margin: number; scale: number }>) {
    const { anchor, margin, scale } = snapshot.overlay;
    run(async () => (await backend()).setOverlayLayout({ anchor, margin, scale, ...change }));
  }

  function saveHotkey() {
    const h = hotkeyDraft;
    if (h === null) return;
    run(async () => {
      await (await backend()).setHotkey(h.trim());
      hotkeyDraft = null;
    });
  }
</script>

<section class="panel">
  <h2>Overlay</h2>
  {#if error}<p class="error">{error}</p>{/if}
  <div class="row">
    <button class:primary={snapshot.overlay.visible} onclick={() => run(async () => (await backend()).setOverlayVisible(!snapshot.overlay.visible))}>
      {snapshot.overlay.visible ? "Overlay shown" : "Overlay hidden"}
    </button>
    {#if hotkeyDraft === null}
      <span class="muted">Hotkey <span class="mono">{snapshot.overlay.hotkey}</span></span>
      <button class="small" onclick={() => (hotkeyDraft = snapshot.overlay.hotkey)}>Change</button>
    {:else}
      <form class="row" onsubmit={(e) => { e.preventDefault(); saveHotkey(); }}>
        <input class="mono" bind:value={hotkeyDraft} placeholder="Ctrl+Shift+F10" />
        <button class="primary small" type="submit">Save</button>
        <button class="small" type="button" onclick={() => (hotkeyDraft = null)}>Cancel</button>
      </form>
    {/if}
  </div>
  <div class="row layout">
    <label>Position
      <select value={snapshot.overlay.anchor} onchange={(e) => layout({ anchor: e.currentTarget.value as Anchor })}>
        {#each anchors as [value, text]}<option {value}>{text}</option>{/each}
      </select>
    </label>
    <label>Distance from edge
      <input type="number" min="0" max="2000" value={snapshot.overlay.margin} onchange={(e) => layout({ margin: Number(e.currentTarget.value) })} /> px
    </label>
    <label>Size
      <input type="range" min="0.5" max="2.5" step="0.1" value={snapshot.overlay.scale} onchange={(e) => layout({ scale: Number(e.currentTarget.value) })} />
      {Math.round(snapshot.overlay.scale * 100)}%
    </label>
  </div>
  <h2>Preview</h2>
  <div class="preview"><OverlayWidgets {snapshot} /></div>
</section>

<style>
  .layout { margin-top: 8px; }
  .layout label { display: flex; gap: 6px; align-items: center; color: var(--muted); font-size: 12px; }
  .preview { background: repeating-linear-gradient(45deg, #1c2128, #1c2128 10px, #20262e 10px, #20262e 20px); padding: 12px; border-radius: 6px; }
  h2 { margin-top: 12px; }
</style>

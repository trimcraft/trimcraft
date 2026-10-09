<script lang="ts">
  import type { ControlRef, DeviceSnapshot, EngineSnapshot } from "../lib/types";
  import { confidenceText, controlName, hatArrow, hex4, sameControl } from "../lib/format";
  import { backend } from "../lib/backend";
  import AxisBar from "./AxisBar.svelte";
  import HatRose from "./HatRose.svelte";

  let { device, snapshot }: { device: DeviceSnapshot; snapshot: EngineSnapshot } = $props();

  let labelDraft = $state("");
  let editing = $state(false);

  function onOverlay(c: ControlRef): boolean {
    return snapshot.overlay.widgets.some(
      (w) => w.type === "control" && w.device === device.id && sameControl(w.control, c),
    );
  }

  function recent(c: ControlRef): boolean {
    const a = snapshot.lastActivity;
    return a !== null && a.device === device.id && sameControl(a.control, c);
  }

  function encoderOf(index: number): string | null {
    const e = snapshot.encoders.find((e) => e.device === device.id && (e.decrement === index || e.increment === index));
    if (!e) return null;
    return `${e.name} ${e.increment === index ? "+" : "−"}`;
  }

  async function toggle(c: ControlRef) {
    await (await backend()).toggleOverlayControl(device.id, c);
  }

  async function saveLabel() {
    await (await backend()).setDeviceLabel(device.id, labelDraft.trim() || null);
    editing = false;
  }
</script>

<section class="panel">
  <div class="head">
    {#if editing}
      <form class="row" onsubmit={(e) => { e.preventDefault(); saveLabel(); }}>
        <input bind:value={labelDraft} placeholder={device.name} />
        <button class="primary" type="submit">Save</button>
        <button type="button" onclick={() => (editing = false)}>Cancel</button>
      </form>
    {:else}
      <h3>{device.label ?? device.name}</h3>
      <button class="small" onclick={() => { labelDraft = device.label ?? ""; editing = true; }}>Rename</button>
    {/if}
  </div>
  <dl class="facts">
    <dt>Detent id</dt><dd class="mono">{device.id}</dd>
    <dt>Reported name</dt><dd>{device.name}</dd>
    <dt>Vendor : product</dt><dd class="mono">{hex4(device.fingerprint.vendorId)}:{hex4(device.fingerprint.productId)}</dd>
    <dt>Serial number</dt><dd class="mono">{device.fingerprint.serial ?? "none reported"}</dd>
    <dt>OS path</dt><dd class="mono path">{device.fingerprint.path ?? "–"}</dd>
    <dt>Identity</dt><dd class:warn={device.confidence === "ambiguous"}>{confidenceText[device.confidence]}</dd>
    <dt>Type</dt><dd>{device.kind.type === "virtual" ? `Virtual (${device.kind.driver}), not an input device` : "Physical"}</dd>
  </dl>
  <p class="muted hint">Click a control to show or hide it in the overlay. Highlighted controls are on the overlay.</p>

  {#if device.state.axes.length}
    <h2>Axes</h2>
    <div class="axes">
      {#each device.state.axes as value, index}
        {@const c = { type: "axis", index } as const}
        <button class="ctl" class:pinned={onOverlay(c)} class:recent={recent(c)} onclick={() => toggle(c)}>
          <AxisBar {value} label={controlName(c)} />
        </button>
      {/each}
    </div>
  {/if}

  {#if device.state.buttons.length}
    <h2>Buttons</h2>
    <div class="buttons">
      {#each device.state.buttons as on, index}
        {@const c = { type: "button", index } as const}
        {@const enc = encoderOf(index)}
        <button class="ctl btn" class:on class:pinned={onOverlay(c)} class:recent={recent(c)} class:encoder={enc !== null}
          title={enc ? `${controlName(c)} · encoder ${enc}` : controlName(c)} onclick={() => toggle(c)}>
          {index + 1}
        </button>
      {/each}
    </div>
  {/if}

  {#if device.state.hats.length}
    <h2>Hats</h2>
    <div class="row">
      {#each device.state.hats as mask, index}
        {@const c = { type: "hat", index } as const}
        <button class="ctl hat" class:pinned={onOverlay(c)} class:recent={recent(c)} onclick={() => toggle(c)}>
          <HatRose {mask} /><span>{controlName(c)} {hatArrow(mask)}</span>
        </button>
      {/each}
    </div>
  {/if}
</section>

<style>
  .head { display: flex; gap: 8px; align-items: center; margin-bottom: 8px; }
  .head h3 { margin: 0; }
  .facts { display: grid; grid-template-columns: max-content 1fr; gap: 2px 12px; margin: 0 0 8px; font-size: 12px; }
  .facts dt { color: var(--muted); }
  .facts dd { margin: 0; }
  .path { word-break: break-all; }
  .warn { color: var(--warn); }
  .hint { font-size: 12px; margin: 4px 0 12px; }
  h2 { margin-top: 14px; }
  .axes { display: flex; flex-direction: column; gap: 4px; }
  .ctl { background: transparent; border: 1px solid transparent; text-align: left; }
  .ctl.pinned { border-color: var(--accent-dim); background: #2a2214; }
  .ctl.recent { outline: 2px solid var(--accent); outline-offset: -2px; }
  .buttons { display: grid; grid-template-columns: repeat(auto-fill, minmax(34px, 1fr)); gap: 4px; }
  .btn { height: 28px; padding: 0; text-align: center; background: var(--lamp-off); font-size: 11px; font-variant-numeric: tabular-nums; }
  .btn.on { background: var(--lamp-on); color: #111; font-weight: 700; }
  .btn.encoder { box-shadow: inset 0 -3px 0 #6f8de0; }
  .btn.pinned { border-color: var(--accent); }
  .hat { display: flex; gap: 8px; align-items: center; }
</style>

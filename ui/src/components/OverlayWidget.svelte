<script lang="ts">
  // One overlay widget. Used by the overlay window and the preview in the main window.
  import type { EngineSnapshot, OverlayWidget } from "../lib/types";
  import { controlName, findDevice, hatArrow } from "../lib/format";
  import AxisBar from "./AxisBar.svelte";
  import HatRose from "./HatRose.svelte";

  let { snapshot, widget: w }: { snapshot: EngineSnapshot; widget: OverlayWidget } = $props();

  function label(w: Extract<OverlayWidget, { type: "control" }>): string {
    if (w.label) return w.label;
    const d = findDevice(snapshot, w.device);
    return `${d ? (d.label ?? d.name) : w.device} · ${controlName(w.control)}`;
  }
</script>

{#if w.type === "profileName"}
  <div class="profile">{snapshot.profiles.find((p) => p.id === snapshot.activeProfile)?.name ?? ""}</div>
{:else if w.type === "function"}
  {@const f = snapshot.functions.find((f) => f.id === w.function)}
  {#if f}<div class="fn"><span class="fname">{f.name}</span> <span class="fvalue">{f.display}</span></div>{/if}
{:else if w.type === "radio"}
  {@const c = snapshot.virtualControls.find((c) => c.id === w.control)}
  {#if c}
    <div class="radio" class:horizontal={w.layout === "horizontal"}>
      <span class="rname">{c.name}</span>
      <ul>
        {#each c.kind.states as s (s.id)}
          <li class:on={c.active === s.id} data-label={s.name}>{s.name}</li>
        {/each}
      </ul>
    </div>
  {/if}
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

<style>
  .profile { font-size: 15px; font-weight: 600; color: var(--accent); text-transform: uppercase; letter-spacing: 0.08em; white-space: nowrap; }
  .fn { font-size: 22px; white-space: nowrap; }
  .fname { color: var(--muted); }
  .fvalue { font-weight: 700; color: var(--accent); font-variant-numeric: tabular-nums; }
  .ctl { display: flex; flex-direction: column; gap: 2px; min-width: 240px; }
  .ctl:has(.lamp), .ctl:has(:global(.rose)) { flex-direction: row; align-items: center; gap: 8px; }
  .ctl.offline { opacity: 0.4; }
  .clabel { font-size: 12px; color: var(--muted); }
  .lamp { width: 14px; height: 14px; border-radius: 50%; background: var(--lamp-off); }
  .lamp.on { background: var(--lamp-on); box-shadow: 0 0 8px var(--lamp-on); }
  .radio { display: flex; flex-direction: column; gap: 4px; }
  .radio.horizontal { flex-direction: row; align-items: center; gap: 10px; }
  .rname { color: var(--muted); font-size: 13px; text-transform: uppercase; letter-spacing: 0.06em; white-space: nowrap; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 3px; }
  .horizontal ul { flex-direction: row; }
  /* The highlight must not change the size: every item has the same border, and the width of
     the bold text is reserved on all items by a hidden bold copy. */
  li {
    display: flex;
    flex-direction: column;
    align-items: center;
    font-size: 16px;
    font-variant-numeric: tabular-nums;
    padding: 2px 10px;
    border-radius: 4px;
    border: 1px solid rgba(240, 160, 48, 0.25);
    color: var(--text);
    white-space: nowrap;
  }
  li.on { background: var(--accent); border-color: var(--accent); color: #111; font-weight: 700; }
  li::after { content: attr(data-label); font-weight: 700; visibility: hidden; height: 0; overflow: hidden; }
</style>

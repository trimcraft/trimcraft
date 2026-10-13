<script lang="ts">
  import { onMount } from "svelte";
  import type { Anchor, EngineSnapshot, MonitorInfo, MonitorRef, OverlayItem, RadioLayout } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import { CONTROL_TYPES, controlName, findDevice } from "../lib/format";
  import OverlayPreview from "./OverlayPreview.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let error = $state("");
  let hotkeyDraft = $state<string | null>(null);
  let monitors = $state<MonitorInfo[]>([]);

  const anchors: [Anchor, string][] = [
    ["topLeft", "Top left"],
    ["topRight", "Top right"],
    ["bottomLeft", "Bottom left"],
    ["bottomRight", "Bottom right"],
  ];

  onMount(() => {
    loadMonitors();
  });

  async function loadMonitors() {
    await run(async () => (monitors = await (await backend()).listMonitors()));
  }

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

  const sameMonitor = (a: MonitorRef, b: MonitorRef) => a.name === b.name && a.x === b.x && a.y === b.y;
  const monitorKey = (m: MonitorRef) => `${m.name ?? ""}@${m.x},${m.y}`;
  const monitorText = (m: MonitorInfo) =>
    `${m.name ?? "Monitor"} · ${m.width}×${m.height} at ${m.x},${m.y}${m.primary ? " · primary" : ""}`;

  let selectedMonitor = $derived.by(() => {
    const saved = snapshot.overlay.monitor;
    if (!saved) return "";
    const found = monitors.find((m) => sameMonitor(m, saved));
    return found ? monitorKey(found) : "missing";
  });

  function chooseMonitor(key: string) {
    const m = monitors.find((m) => monitorKey(m) === key);
    const monitor: MonitorRef | null = m ? { name: m.name, x: m.x, y: m.y, width: m.width, height: m.height } : null;
    run(async () => (await backend()).setOverlayMonitor(monitor));
  }

  function widgetName(w: OverlayItem): string {
    switch (w.type) {
      case "profileName":
        return "Profile name";
      case "function":
        return snapshot.functions.find((f) => f.id === w.function)?.name ?? w.function;
      case "radio":
      {
        const c = snapshot.virtualControls.find((c) => c.id === w.control);
        const type = CONTROL_TYPES.find(([t]) => t === c?.kind.type)?.[1] ?? "Control";
        return `${type}: ${c?.name ?? w.control}`;
      }
      case "control": {
        if (w.label) return w.label;
        const d = findDevice(snapshot, w.device);
        return `${d ? (d.label ?? d.name) : w.device} · ${controlName(w.control)}`;
      }
    }
  }

  function update(index: number, change: Partial<OverlayItem>) {
    const item = { ...snapshot.overlay.widgets[index]!, ...change } as OverlayItem;
    run(async () => (await backend()).setOverlayWidget(index, item));
  }

  function setCoord(index: number, axis: "x" | "y", percent: number) {
    const w = snapshot.overlay.widgets[index]!;
    const pos = w.position ?? { x: 0, y: 0 };
    const v = Math.min(Math.max((Number.isFinite(percent) ? percent : 0) / 100, 0), 1);
    update(index, { position: { ...pos, [axis]: v } });
  }

  function setVisibility(index: number, afterChange: boolean) {
    update(index, { visibility: afterChange ? { mode: "afterChange", seconds: 3 } : { mode: "always" } });
  }

  /** An empty field removes the setting. `max` is the largest percent. */
  function setOptional(index: number, key: "scale" | "opacity", text: string, max: number) {
    const item = { ...snapshot.overlay.widgets[index]! } as OverlayItem;
    const percent = Number(text);
    if (text.trim() === "" || !Number.isFinite(percent)) delete item[key];
    else item[key] = Math.min(Math.max(percent, key === "scale" ? 50 : 0), max) / 100;
    run(async () => (await backend()).setOverlayWidget(index, item));
  }

  /** Radios and modes show their states, vertically or horizontally. */
  const hasStates = (id: string) => ["radio", "mode"].includes(snapshot.virtualControls.find((c) => c.id === id)?.kind.type ?? "");
  const pct = (v: number) => Math.round(v * 1000) / 10;
  const hasProfileName = $derived(snapshot.overlay.widgets.some((w) => w.type === "profileName"));
  const profileName = $derived(snapshot.profiles.find((p) => p.id === snapshot.activeProfile)?.name ?? "");
</script>

<section class="panel">
  <h2>Overlay</h2>
  <p class="muted small">Monitor, size, hotkey and visibility apply to all profiles; the widgets and their positions belong to the profile <b>{profileName}</b>.</p>
  {#if error}<p class="error">{error}</p>{/if}
  <div class="row">
    <button class:primary={snapshot.overlay.visible} onclick={() => run(async () => (await backend()).setOverlayVisible(!snapshot.overlay.visible))}>
      {snapshot.overlay.visible ? "Overlay shown" : "Overlay hidden"}
    </button>
    {#if snapshot.overlayEditing}
      <button class="primary" onclick={() => run(async () => (await backend()).setOverlayEditing(false))}>Done editing layout</button>
    {:else}
      <button onclick={() => run(async () => (await backend()).setOverlayEditing(true))}>Edit layout</button>
    {/if}
    <button class:active={hasProfileName} title="Shows the active profile's name on the overlay (set per profile)" onclick={() => run(async () => (await backend()).toggleOverlayProfileName())}>
      {hasProfileName ? "Profile name shown" : "Show profile name"}
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
  {#if snapshot.overlayEditing}
    <p class="muted small">The overlay now takes the mouse: drag the widgets on screen, then press Done (on the overlay, here, or Esc).</p>
  {/if}
  <div class="row layout">
    <label>Monitor
      <select value={selectedMonitor} onchange={(e) => chooseMonitor(e.currentTarget.value)}>
        <option value="">Primary monitor</option>
        {#if selectedMonitor === "missing"}<option value="missing" disabled>{snapshot.overlay.monitor?.name ?? "Saved monitor"} (not connected)</option>{/if}
        {#each monitors as m (monitorKey(m))}<option value={monitorKey(m)}>{monitorText(m)}</option>{/each}
      </select>
      <button class="small" title="Read the monitor list again" onclick={loadMonitors}>↻</button>
    </label>
    <label>Size
      <input type="range" min="0.5" max="2.5" step="0.1" value={snapshot.overlay.scale} onchange={(e) => layout({ scale: Number(e.currentTarget.value) })} />
      {Math.round(snapshot.overlay.scale * 100)}%
    </label>
  </div>

  {#if snapshot.overlay.widgets.length}
    <h2>Widgets</h2>
    <table class="widgets">
      <thead><tr><th>Widget</th><th>X %</th><th>Y %</th><th></th><th>Visible</th><th title="On top of the overlay size">Size %</th><th title="Of the whole widget, on top of the background opacity in Settings">Opacity %</th></tr></thead>
      <tbody>
        {#each snapshot.overlay.widgets as w, i (i)}
          <tr>
            <td>{widgetName(w)}</td>
            {#if w.position}
              <td><input type="number" min="0" max="100" step="0.5" value={pct(w.position.x)} onchange={(e) => setCoord(i, "x", Number(e.currentTarget.value))} /></td>
              <td><input type="number" min="0" max="100" step="0.5" value={pct(w.position.y)} onchange={(e) => setCoord(i, "y", Number(e.currentTarget.value))} /></td>
            {:else}
              <td colspan="2" class="muted small">in the corner stack <button class="small" onclick={() => update(i, { position: { x: 0.05, y: 0.05 } })}>Place freely</button></td>
            {/if}
            <td>
              {#if w.type === "radio" && hasStates(w.control)}
                <select value={w.layout} onchange={(e) => update(i, { layout: e.currentTarget.value as RadioLayout })}>
                  <option value="vertical">vertical</option>
                  <option value="horizontal">horizontal</option>
                </select>
              {/if}
            </td>
            <td class="vis">
              <select value={w.visibility?.mode ?? "always"} onchange={(e) => setVisibility(i, e.currentTarget.value === "afterChange")}>
                <option value="always">Always</option>
                <option value="afterChange">Only after a change</option>
              </select>
              {#if w.visibility?.mode === "afterChange"}
                <input type="number" min="1" max="60" step="1" value={w.visibility.seconds} title="Seconds shown after a change" onchange={(e) => update(i, { visibility: { mode: "afterChange", seconds: Math.min(60, Math.max(1, Math.round(Number(e.currentTarget.value) || 3))) } })} /> s
              {/if}
            </td>
            <td><input type="number" min="50" max="300" step="5" placeholder="100" value={w.scale ? Math.round(w.scale * 100) : ""} onchange={(e) => setOptional(i, "scale", e.currentTarget.value, 300)} /></td>
            <td><input type="number" min="0" max="100" step="5" placeholder="100" value={w.opacity !== undefined ? Math.round(w.opacity * 100) : ""} onchange={(e) => setOptional(i, "opacity", e.currentTarget.value, 100)} /></td>
          </tr>
        {/each}
      </tbody>
    </table>
    {#if snapshot.overlay.widgets.some((w) => !w.position)}
      <div class="row layout">
        <span class="muted small">Corner stack for widgets not placed yet:</span>
        <label>Position
          <select value={snapshot.overlay.anchor} onchange={(e) => layout({ anchor: e.currentTarget.value as Anchor })}>
            {#each anchors as [value, text]}<option {value}>{text}</option>{/each}
          </select>
        </label>
        <label>Distance from edge
          <input type="number" min="0" max="2000" value={snapshot.overlay.margin} onchange={(e) => layout({ margin: Number(e.currentTarget.value) })} /> px
        </label>
      </div>
    {/if}
  {/if}
  <h2>Preview</h2>
  <OverlayPreview {snapshot} />
</section>

<style>
  .layout { margin-top: 8px; }
  .layout label { display: flex; gap: 6px; align-items: center; color: var(--muted); font-size: 12px; }
  h2 { margin-top: 12px; }
  .small { font-size: 12px; }
  .widgets { border-collapse: collapse; font-size: 12px; }
  .widgets th { text-align: left; color: var(--muted); font-weight: normal; padding: 2px 8px 2px 0; }
  .widgets td { padding: 2px 8px 2px 0; border-top: 1px solid var(--line); }
  .widgets input[type="number"] { width: 5em; }
  .vis { white-space: nowrap; }
  .vis input[type="number"] { width: 3.5em; }
</style>

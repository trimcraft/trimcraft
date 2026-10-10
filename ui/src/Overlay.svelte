<script lang="ts">
  // The overlay window: covers one monitor, transparent and click-through. Widgets with a
  // position sit where the user put them; the others are stacked in the anchored corner.
  // In layout edit mode the window takes the mouse and widgets can be dragged.
  import { onMount, tick } from "svelte";
  import type { EngineSnapshot, WidgetPosition } from "./lib/types";
  import { backend, errorText } from "./lib/backend";
  import OverlayWidget from "./components/OverlayWidget.svelte";

  let snapshot = $state<EngineSnapshot | null>(null);
  let error = $state("");
  /** Position of the widget being dragged, until the engine has the new one. */
  let dragging = $state<{ index: number; pos: WidgetPosition; dx: number; dy: number } | null>(null);
  /** Rendered element per widget index, for measuring. */
  let elements: HTMLElement[] = $state([]);
  let fixedForSession = false;

  onMount(() => {
    let stop: (() => void) | undefined;
    backend().then(async (b) => (stop = await b.subscribe((s) => (snapshot = s))));
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && snapshot?.overlayEditing) done();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      stop?.();
      window.removeEventListener("keydown", onKey);
    };
  });

  let editing = $derived(snapshot?.overlayEditing ?? false);
  let items = $derived(snapshot?.overlay.widgets.map((item, index) => ({ item, index })) ?? []);
  let placed = $derived(items.filter(({ item }) => item.position !== null));
  let stacked = $derived(items.filter(({ item }) => item.position === null));

  let stackPosition = $derived.by(() => {
    if (!snapshot) return "";
    const { anchor, margin } = snapshot.overlay;
    const v = anchor.startsWith("top") ? `top:${margin}px` : `bottom:${margin}px`;
    const h = anchor.endsWith("Left") ? `left:${margin}px` : `right:${margin}px`;
    return `${v};${h}`;
  });
  let stackOrigin = $derived(
    snapshot ? `${snapshot.overlay.anchor.startsWith("top") ? "top" : "bottom"} ${snapshot.overlay.anchor.endsWith("Left") ? "left" : "right"}` : "",
  );

  // Entering edit mode turns stacked widgets into placed ones at the spot they are shown,
  // so nothing jumps and every widget can be dragged.
  $effect(() => {
    if (!editing) {
      fixedForSession = false;
      return;
    }
    if (fixedForSession || !snapshot || stacked.length === 0) return;
    fixedForSession = true;
    // Plain data, not reactive proxies: it crosses into IPC (or the demo).
    const s = $state.snapshot(snapshot);
    tick().then(() => {
      const positions = s.overlay.widgets.map((w, i) => {
        if (w.position) return w.position;
        const r = elements[i]?.getBoundingClientRect();
        return r ? { x: r.left / window.innerWidth, y: r.top / window.innerHeight } : null;
      });
      run(async () => (await backend()).setOverlayPositions(positions));
    });
  });

  async function run(action: () => Promise<unknown>) {
    error = "";
    try {
      await action();
    } catch (e) {
      error = errorText(e);
    }
  }

  function done() {
    run(async () => (await backend()).setOverlayEditing(false));
  }

  function position(index: number, pos: WidgetPosition): string {
    const p = dragging?.index === index ? dragging.pos : pos;
    return `left:${p.x * 100}%;top:${p.y * 100}%`;
  }

  function startDrag(e: PointerEvent, index: number, pos: WidgetPosition) {
    if (!editing || e.button !== 0) return;
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const r = el.getBoundingClientRect();
    dragging = { index, pos, dx: e.clientX - r.left, dy: e.clientY - r.top };
  }

  function moveDrag(e: PointerEvent) {
    if (!dragging) return;
    const clamp = (v: number) => Math.min(Math.max(v, 0), 1);
    dragging.pos = {
      x: clamp((e.clientX - dragging.dx) / window.innerWidth),
      y: clamp((e.clientY - dragging.dy) / window.innerHeight),
    };
  }

  function endDrag() {
    const d = dragging;
    const item = d && snapshot?.overlay.widgets[d.index];
    if (!d || !item) {
      dragging = null;
      return;
    }
    run(async () => {
      const position = { x: d.pos.x, y: d.pos.y };
      await (await backend()).setOverlayWidget(d.index, { ...$state.snapshot(item), position });
      dragging = null;
    });
  }
</script>

{#if snapshot}
  <div class="surface" class:editing>
    {#if editing}
      <div class="toolbar">
        <span>Layout edit mode: drag the widgets.</span>
        {#if error}<span class="error">{error}</span>{/if}
        <button class="primary" onclick={done}>Done</button>
      </div>
    {/if}

    {#each placed as { item, index } (index)}
      <div
        class="panel placed"
        class:frame={editing}
        class:moving={dragging?.index === index}
        style={position(index, item.position!)}
        style:transform="scale({snapshot.overlay.scale})"
        bind:this={elements[index]}
        onpointerdown={(e) => startDrag(e, index, item.position!)}
        onpointermove={moveDrag}
        onpointerup={endDrag}
        onpointercancel={endDrag}
        role="presentation"
      >
        <OverlayWidget {snapshot} widget={item} />
      </div>
    {/each}

    {#if stacked.length}
      <div class="stack" class:right={snapshot.overlay.anchor.endsWith("Right")} style={stackPosition} style:transform="scale({snapshot.overlay.scale})" style:transform-origin={stackOrigin}>
        {#each stacked as { item, index } (index)}
          <div class="panel" class:frame={editing} bind:this={elements[index]}><OverlayWidget {snapshot} widget={item} /></div>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  :global(body) { background: transparent !important; margin: 0; overflow: hidden; }
  .surface { position: fixed; inset: 0; }
  /* A faint tint in edit mode shows that the overlay has the mouse now. */
  .surface.editing { background: rgba(12, 15, 19, 0.35); outline: 2px dashed rgba(240, 160, 48, 0.6); outline-offset: -2px; }
  .panel {
    background: rgba(12, 15, 19, 0.72);
    border: 1px solid rgba(240, 160, 48, 0.35);
    border-radius: 8px;
    padding: 10px 14px;
  }
  .placed { position: absolute; transform-origin: top left; }
  .stack { position: absolute; display: flex; flex-direction: column; gap: 6px; align-items: flex-start; }
  .stack.right { align-items: flex-end; }
  .frame { outline: 2px solid var(--accent); outline-offset: 2px; }
  .placed.frame { cursor: grab; user-select: none; }
  .placed.moving { cursor: grabbing; opacity: 0.85; }
  .toolbar {
    position: absolute;
    top: 16px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    gap: 12px;
    align-items: center;
    background: var(--panel);
    border: 1px solid var(--accent);
    border-radius: 6px;
    padding: 8px 12px;
    z-index: 10;
  }
</style>

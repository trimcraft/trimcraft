<script lang="ts">
  // The overlay window: covers one monitor, transparent and click-through. Widgets with a
  // position sit where the user put them; the others are stacked in the anchored corner.
  // In layout edit mode the window takes the mouse and widgets can be dragged.
  import { onMount, tick } from "svelte";
  import type { EngineSnapshot, OverlayItem, Toast, WidgetPosition } from "./lib/types";
  import { backend, errorText } from "./lib/backend";
  import OverlayWidget from "./components/OverlayWidget.svelte";
  import ToastView from "./components/ToastView.svelte";
  import { overlayStyle } from "./lib/overlay-style";
  import { AfterChange, widgetKey } from "./lib/widget-visibility.svelte";
  import { countdown } from "./lib/recording.svelte";

  let snapshot = $state<EngineSnapshot | null>(null);
  let error = $state("");
  /** Position of the widget being dragged, until the engine has the new one. */
  let dragging = $state<{ index: number; pos: WidgetPosition; dx: number; dy: number } | null>(null);
  /** Rendered element per widget index, for measuring. */
  let elements: HTMLElement[] = $state([]);
  /** Profile whose stacked widgets were fixed in this edit session (widgets are per profile). */
  let fixedFor: string | null = null;
  let toasts = $state<Toast[]>([]);
  /** The toast area being dragged (layout edit mode). */
  let toastDrag = $state<{ pos: WidgetPosition; dx: number; dy: number } | null>(null);
  const afterChange = new AfterChange();

  onMount(() => {
    let stop: (() => void) | undefined;
    let stopToasts: (() => void) | undefined;
    backend().then(async (b) => {
      stop = await b.subscribe((s) => (snapshot = s));
      stopToasts = await b.subscribeToasts((t) => (toasts = t));
    });
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && snapshot?.overlayEditing) done();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      stop?.();
      stopToasts?.();
      afterChange.reset();
      window.removeEventListener("keydown", onKey);
    };
  });

  let editing = $derived(snapshot?.overlayEditing ?? false);
  // Widgets set to "only after a change" notice changes as snapshots arrive.
  $effect(() => {
    if (snapshot) afterChange.update(snapshot);
  });
  const faded = (item: OverlayItem) => !editing && item.visibility?.mode === "afterChange" && !afterChange.shown[widgetKey(item)];
  let style = $derived(snapshot ? overlayStyle(snapshot.overlay.appearance) : "");
  let recording = $derived(snapshot?.recorder.phase ?? "idle");
  const recCountdown = countdown(() => recording === "countdown");
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
      fixedFor = null;
      return;
    }
    if (!snapshot || fixedFor === snapshot.activeProfile || stacked.length === 0) return;
    fixedFor = snapshot.activeProfile;
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

  // The toast area: placed by dragging in layout edit mode, stored per profile.
  let toastPlace = $derived.by(() => {
    if (!snapshot) return "";
    const s = snapshot.overlay.scale;
    const pos = toastDrag?.pos ?? snapshot.toastPosition;
    // Default: centred near the top.
    if (!pos) return `left:50%;top:6%;transform:translateX(-50%) scale(${s});transform-origin:top center`;
    return `left:${pos.x * 100}%;top:${pos.y * 100}%;transform:scale(${s});transform-origin:top left`;
  });

  function startToastDrag(e: PointerEvent) {
    if (!editing || e.button !== 0) return;
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const r = el.getBoundingClientRect();
    toastDrag = { pos: { x: r.left / window.innerWidth, y: r.top / window.innerHeight }, dx: e.clientX - r.left, dy: e.clientY - r.top };
  }

  function moveToastDrag(e: PointerEvent) {
    if (!toastDrag) return;
    const clamp = (v: number) => Math.min(Math.max(v, 0), 1);
    toastDrag.pos = { x: clamp((e.clientX - toastDrag.dx) / window.innerWidth), y: clamp((e.clientY - toastDrag.dy) / window.innerHeight) };
  }

  function endToastDrag() {
    const d = toastDrag;
    if (!d) return;
    run(async () => {
      await (await backend()).setToastPosition({ x: d.pos.x, y: d.pos.y });
      toastDrag = null;
    });
  }

  const SAMPLE_TOAST: Toast[] = [{ id: 0, key: "sample", title: "Toast area", text: "drag me", version: 0 }];
</script>

{#if snapshot}
  <div class="surface" class:editing {style}>
    {#if recording !== "idle"}
      <div class="rec" class:live={recording === "recording"}>
        ● {recording === "countdown" ? `REC in ${recCountdown.left}` : "REC"}
      </div>
    {/if}
    {#if editing}
      <div class="toolbar">
        <span>Layout edit mode: drag the widgets and the toast area.</span>
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
        style:transform="scale({snapshot.overlay.scale * (item.scale ?? 1)})"
        style:opacity={faded(item) ? 0 : (item.opacity ?? 1)}
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
          <div
            class="panel"
            class:frame={editing}
            style:transform={item.scale ? `scale(${item.scale})` : undefined}
            style:transform-origin={stackOrigin}
            style:opacity={faded(item) ? 0 : (item.opacity ?? 1)}
            bind:this={elements[index]}
          ><OverlayWidget {snapshot} widget={item} /></div>
        {/each}
      </div>
    {/if}

    {#if editing}
      <div
        class="toast-area frame"
        class:moving={toastDrag !== null}
        style={toastPlace}
        onpointerdown={startToastDrag}
        onpointermove={moveToastDrag}
        onpointerup={endToastDrag}
        onpointercancel={endToastDrag}
        role="presentation"
      >
        <ToastView toasts={SAMPLE_TOAST} />
      </div>
    {:else}
      <div class="toast-area" style={toastPlace}><ToastView {toasts} /></div>
    {/if}
  </div>
{/if}

<style>
  :global(body) { background: transparent !important; margin: 0; overflow: hidden; }
  .surface { position: fixed; inset: 0; color: var(--text); }
  /* A faint tint in edit mode shows that the overlay has the mouse now. */
  .surface.editing { background: rgba(12, 15, 19, 0.35); outline: 2px dashed rgba(240, 160, 48, 0.6); outline-offset: -2px; }
  .panel {
    background: var(--ov-bg);
    border: 1px solid var(--ov-border);
    border-radius: var(--ov-radius);
    padding: 10px 14px;
    color: var(--text);
    /* The fade of "only after a change" widgets is a transition, not an animation loop. */
    transition: opacity 0.35s ease;
  }
  .toast-area { position: absolute; z-index: 15; pointer-events: none; }
  .toast-area.frame { pointer-events: auto; cursor: grab; user-select: none; }
  .toast-area.moving { cursor: grabbing; opacity: 0.85; }
  .placed { position: absolute; transform-origin: top left; }
  .stack { position: absolute; display: flex; flex-direction: column; gap: 6px; align-items: flex-start; }
  .stack.right { align-items: flex-end; }
  .frame { outline: 2px solid var(--accent); outline-offset: 2px; }
  .placed.frame { cursor: grab; user-select: none; }
  .placed.moving { cursor: grabbing; opacity: 0.85; }
  .rec {
    position: absolute;
    top: 12px;
    left: 50%;
    transform: translateX(-50%);
    background: rgba(12, 15, 19, 0.8);
    color: #ff4d42;
    border: 2px solid #ff4d42;
    border-radius: 14px;
    padding: 2px 14px;
    font-weight: 700;
    letter-spacing: 0.08em;
    z-index: 20;
  }
  .rec.live { animation: rec-blink 1s steps(2, start) infinite; }
  @keyframes rec-blink { to { opacity: 0.45; } }
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

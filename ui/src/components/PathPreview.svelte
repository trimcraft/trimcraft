<script lang="ts">
  // Preview of a mouse macro's movement. Absolute moves are drawn on the outline of the
  // macro's monitor; relative moves (mouse-look) are chained from where they start, scaled to
  // fit. Waypoints of paths and jump targets can be dragged.
  import type { MouseStep } from "../lib/types";

  let {
    steps = $bindable(),
    monitor,
    focus = null,
  }: {
    steps: MouseStep[];
    /** Size of the macro's monitor, for the outline's shape. */
    monitor: { width: number; height: number } | null;
    /** Index of the step to highlight. */
    focus?: number | null;
  } = $props();

  const HEIGHT = 260;
  const PAD = 12;
  const HIT = 9;

  let width = $state(520);
  let absCanvas = $state<HTMLCanvasElement | null>(null);
  let relCanvas = $state<HTMLCanvasElement | null>(null);

  let hasAbsolute = $derived(steps.some((s) => s.type === "moveTo" || s.type === "movePath"));
  let hasRelative = $derived(steps.some((s) => s.type === "moveRelativePath" || s.type === "moveRelative"));

  type Pt = { x: number; y: number };
  /** A draggable point: step index and waypoint index (null for a jump target). */
  type Handle = { step: number; point: number | null };

  function colors() {
    const css = getComputedStyle(document.documentElement);
    const v = (name: string) => css.getPropertyValue(name).trim();
    return { line: v("--line"), panel: v("--panel"), muted: v("--muted"), accent: v("--accent"), text: v("--text") };
  }

  function setup(canvas: HTMLCanvasElement): CanvasRenderingContext2D | null {
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(HEIGHT * ratio);
    const ctx = canvas.getContext("2d");
    ctx?.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx?.clearRect(0, 0, width, HEIGHT);
    return ctx;
  }

  // --- Absolute ----------------------------------------------------------------------------

  /** Where the monitor outline sits in the canvas. */
  let frame = $derived.by(() => {
    const aspect = monitor && monitor.height > 0 ? monitor.width / monitor.height : 16 / 9;
    let h = HEIGHT - 2 * PAD;
    let w = h * aspect;
    if (w > width - 2 * PAD) {
      w = width - 2 * PAD;
      h = w / aspect;
    }
    return { x: (width - w) / 2, y: (HEIGHT - h) / 2, w, h };
  });

  const toCanvas = (fx: number, fy: number): Pt => ({ x: frame.x + fx * frame.w, y: frame.y + fy * frame.h });
  const toFraction = (p: Pt): Pt => ({
    x: Math.min(Math.max((p.x - frame.x) / frame.w, 0), 1),
    y: Math.min(Math.max((p.y - frame.y) / frame.h, 0), 1),
  });

  function absHandles(): (Handle & Pt)[] {
    const out: (Handle & Pt)[] = [];
    steps.forEach((s, step) => {
      if (s.type === "moveTo") out.push({ step, point: null, ...toCanvas(s.x, s.y) });
      if (s.type === "movePath") s.points.forEach((p, point) => out.push({ step, point, ...toCanvas(p.x, p.y) }));
    });
    return out;
  }

  function drawAbsolute() {
    if (!absCanvas) return;
    const ctx = setup(absCanvas);
    if (!ctx) return;
    const c = colors();
    ctx.fillStyle = c.panel;
    ctx.strokeStyle = c.line;
    ctx.lineWidth = 1;
    ctx.fillRect(frame.x, frame.y, frame.w, frame.h);
    ctx.strokeRect(frame.x + 0.5, frame.y + 0.5, frame.w - 1, frame.h - 1);
    // Cursor travel from step to step, faint and dashed.
    let last: Pt | null = null;
    ctx.setLineDash([3, 4]);
    ctx.strokeStyle = c.muted;
    for (const s of steps) {
      const first = s.type === "moveTo" ? s : s.type === "movePath" ? s.points[0] : undefined;
      const end = s.type === "moveTo" ? s : s.type === "movePath" ? s.points.at(-1) : undefined;
      if (!first || !end) continue;
      const a = toCanvas(first.x, first.y);
      if (last) {
        ctx.beginPath();
        ctx.moveTo(last.x, last.y);
        ctx.lineTo(a.x, a.y);
        ctx.stroke();
      }
      last = toCanvas(end.x, end.y);
    }
    ctx.setLineDash([]);
    steps.forEach((s, i) => {
      const focused = focus === i;
      ctx.strokeStyle = ctx.fillStyle = focused ? c.accent : c.text;
      if (s.type === "movePath") {
        ctx.lineWidth = focused ? 2 : 1.25;
        ctx.beginPath();
        s.points.forEach((p, k) => {
          const q = toCanvas(p.x, p.y);
          if (k === 0) ctx.moveTo(q.x, q.y);
          else ctx.lineTo(q.x, q.y);
        });
        ctx.stroke();
        for (const p of s.points) {
          const q = toCanvas(p.x, p.y);
          ctx.beginPath();
          ctx.arc(q.x, q.y, focused ? 4 : 2.5, 0, Math.PI * 2);
          ctx.fill();
        }
      } else if (s.type === "moveTo") {
        const q = toCanvas(s.x, s.y);
        ctx.fillRect(q.x - 4, q.y - 4, 8, 8);
        ctx.font = "11px sans-serif";
        ctx.fillText(String(i + 1), q.x + 6, q.y - 6);
      }
    });
  }

  // --- Relative ----------------------------------------------------------------------------

  /** Positions of relative moves, chained from the origin, per step. */
  function relChain(): { step: number; start: Pt; points: Pt[] }[] {
    const out: { step: number; start: Pt; points: Pt[] }[] = [];
    let pos: Pt = { x: 0, y: 0 };
    steps.forEach((s, step) => {
      if (s.type === "moveRelative") {
        const end = { x: pos.x + s.dx, y: pos.y + s.dy };
        out.push({ step, start: pos, points: [end] });
        pos = end;
      } else if (s.type === "moveRelativePath") {
        const start = pos;
        const points = s.points.map((p) => ({ x: start.x + p.dx, y: start.y + p.dy }));
        out.push({ step, start, points });
        pos = points.at(-1) ?? start;
      }
    });
    return out;
  }

  /** Scale and offset that fit all relative positions (fixed while dragging). */
  let relView: { scale: number; ox: number; oy: number } | null = null;
  let relDragging = false;

  function fitRelative(chain: ReturnType<typeof relChain>) {
    const all = [{ x: 0, y: 0 }, ...chain.flatMap((c) => c.points)];
    const xs = all.map((p) => p.x);
    const ys = all.map((p) => p.y);
    const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    const scale = Math.min((width - 4 * PAD) / Math.max(x1 - x0, 1), (HEIGHT - 4 * PAD) / Math.max(y1 - y0, 1), 4);
    return { scale, ox: width / 2 - ((x0 + x1) / 2) * scale, oy: HEIGHT / 2 - ((y0 + y1) / 2) * scale };
  }

  function drawRelative() {
    if (!relCanvas) return;
    const ctx = setup(relCanvas);
    if (!ctx) return;
    const c = colors();
    const chain = relChain();
    if (!relDragging || !relView) relView = fitRelative(chain);
    const v = relView;
    const at = (p: Pt): Pt => ({ x: v.ox + p.x * v.scale, y: v.oy + p.y * v.scale });
    ctx.fillStyle = c.panel;
    ctx.fillRect(0, 0, width, HEIGHT);
    // Origin cross.
    const o = at({ x: 0, y: 0 });
    ctx.strokeStyle = c.line;
    ctx.beginPath();
    ctx.moveTo(o.x - 8, o.y);
    ctx.lineTo(o.x + 8, o.y);
    ctx.moveTo(o.x, o.y - 8);
    ctx.lineTo(o.x, o.y + 8);
    ctx.stroke();
    for (const part of chain) {
      const focused = focus === part.step;
      ctx.strokeStyle = ctx.fillStyle = focused ? c.accent : c.text;
      ctx.lineWidth = focused ? 2 : 1.25;
      ctx.beginPath();
      const s = at(part.start);
      ctx.moveTo(s.x, s.y);
      for (const p of part.points) {
        const q = at(p);
        ctx.lineTo(q.x, q.y);
      }
      ctx.stroke();
      if (steps[part.step]?.type === "moveRelativePath") {
        for (const p of part.points) {
          const q = at(p);
          ctx.beginPath();
          ctx.arc(q.x, q.y, focused ? 4 : 2.5, 0, Math.PI * 2);
          ctx.fill();
        }
      }
    }
    ctx.fillStyle = c.muted;
    ctx.font = "11px sans-serif";
    ctx.fillText(`1 px = ${(1 / v.scale).toFixed(1)} mickeys`, 8, HEIGHT - 8);
  }

  // Redraw whenever the steps, the focus or the size change.
  $effect(() => {
    $state.snapshot(steps);
    void focus;
    void width;
    void frame;
    drawAbsolute();
    drawRelative();
  });

  // --- Dragging ----------------------------------------------------------------------------

  let drag = $state<{ kind: "abs" | "rel"; handle: Handle; start: Pt } | null>(null);

  function local(e: PointerEvent): Pt {
    const r = (e.currentTarget as HTMLCanvasElement).getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  function nearest<T extends Pt>(list: T[], p: Pt): T | null {
    let best: T | null = null;
    let bestD = HIT;
    for (const h of list) {
      const d = Math.hypot(h.x - p.x, h.y - p.y);
      if (d <= bestD) [best, bestD] = [h, d];
    }
    return best;
  }

  function downAbs(e: PointerEvent) {
    const hit = nearest(absHandles(), local(e));
    if (!hit) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { kind: "abs", handle: { step: hit.step, point: hit.point }, start: { x: 0, y: 0 } };
  }

  function moveAbs(e: PointerEvent) {
    if (drag?.kind !== "abs") return;
    const f = toFraction(local(e));
    const s = steps[drag.handle.step];
    if (s?.type === "moveTo") [s.x, s.y] = [f.x, f.y];
    if (s?.type === "movePath" && drag.handle.point !== null) {
      const p = s.points[drag.handle.point];
      if (p) [p.x, p.y] = [f.x, f.y];
    }
  }

  function downRel(e: PointerEvent) {
    const v = relView;
    if (!v) return;
    const at = (p: Pt): Pt => ({ x: v.ox + p.x * v.scale, y: v.oy + p.y * v.scale });
    const handles = relChain()
      .filter((c) => steps[c.step]?.type === "moveRelativePath")
      .flatMap((c) => c.points.map((p, point) => ({ step: c.step, point, start: c.start, ...at(p) })));
    const hit = nearest(handles, local(e));
    if (!hit) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    relDragging = true;
    drag = { kind: "rel", handle: { step: hit.step, point: hit.point }, start: hit.start };
  }

  function moveRel(e: PointerEvent) {
    const v = relView;
    if (drag?.kind !== "rel" || !v || drag.handle.point === null) return;
    const p = local(e);
    const s = steps[drag.handle.step];
    const point = s?.type === "moveRelativePath" ? s.points[drag.handle.point] : undefined;
    if (!point) return;
    point.dx = Math.round((p.x - v.ox) / v.scale - drag.start.x);
    point.dy = Math.round((p.y - v.oy) / v.scale - drag.start.y);
  }

  function up() {
    drag = null;
    relDragging = false;
  }
</script>

<div class="preview" bind:clientWidth={width}>
  {#if hasAbsolute}
    <div class="label muted">Absolute moves on the {monitor ? `${monitor.width}×${monitor.height}` : "primary"} monitor — drag waypoints to adjust</div>
    <canvas
      bind:this={absCanvas}
      style:height="{HEIGHT}px"
      class:dragging={drag?.kind === "abs"}
      onpointerdown={downAbs}
      onpointermove={moveAbs}
      onpointerup={up}
      onpointercancel={up}
    ></canvas>
  {/if}
  {#if hasRelative}
    <div class="label muted">Relative moves from where the macro starts (+ is the start) — drag waypoints to adjust</div>
    <canvas
      bind:this={relCanvas}
      style:height="{HEIGHT}px"
      class:dragging={drag?.kind === "rel"}
      onpointerdown={downRel}
      onpointermove={moveRel}
      onpointerup={up}
      onpointercancel={up}
    ></canvas>
  {/if}
</div>

<style>
  .preview { display: flex; flex-direction: column; gap: 4px; width: 100%; }
  canvas { width: 100%; border: 1px solid var(--line); border-radius: 4px; cursor: crosshair; touch-action: none; }
  canvas.dragging { cursor: grabbing; }
  .label { font-size: 11px; }
</style>

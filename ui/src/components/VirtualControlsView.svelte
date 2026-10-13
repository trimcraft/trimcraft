<script lang="ts">
  import { onMount } from "svelte";
  import type { ControlCommand, ControlType, EngineSnapshot, KeyInfo, VirtualControlDef, VirtualControlKind, VirtualControlView } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import { CONTROL_TYPES } from "../lib/format";
  import { captured } from "../lib/learn.svelte";
  import ControlEditor from "./ControlEditor.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let draft = $state<VirtualControlDef | null>(null);
  let error = $state("");
  let keys = $state<KeyInfo[]>([]);
  let valueDraft = $state<Record<string, string>>({});

  onMount(() => {
    backend()
      .then((b) => b.keys())
      .then((k) => (keys = k))
      .catch((e) => (error = errorText(e)));
  });

  // The engine captured an input for an Assign button: hand it to the slot that asked.
  $effect(() => {
    const t = snapshot.learn?.captured;
    if (t) captured($state.snapshot(t));
  });

  const states = (names: string[]) => names.map((name) => ({ id: "", name, select: [], onActivate: [], onDeactivate: [] }));

  function blankKind(type: ControlType): VirtualControlKind {
    switch (type) {
      case "radio":
        return { type, states: states(["State 1", "State 2", "State 3"]), increment: [], decrement: [], clear: [], toggleOff: [], end: "wrap", toggleOnReselect: false, startup: "restoreLast" };
      case "toggle":
        return { type, toggle: [], on: [], off: [], onOn: [], onOff: [], startup: "restoreLast" };
      case "momentary":
        return { type, triggers: [], onPress: [], onRelease: [], invert: false };
      case "counter":
        return { type, spec: { min: 1, max: 20, step: 1, initial: 1, end: "wrap", decimals: 0 }, increment: [], decrement: [], reset: [], onChange: [], startup: "restoreLast" };
      case "mode":
        return { type, states: states(["Mode 1", "Mode 2"]).map((s) => ({ ...s, shift: [] })), next: [], previous: [], end: "wrap", startup: "restoreLast" };
    }
  }

  const blank = (type: ControlType): VirtualControlDef => ({
    id: "",
    name: CONTROL_TYPES.find(([t]) => t === type)?.[1] ?? "Control",
    kind: blankKind(type),
  });

  async function run(action: () => Promise<unknown>) {
    error = "";
    try {
      await action();
      return true;
    } catch (e) {
      error = errorText(e);
      return false;
    }
  }

  function edit(c: VirtualControlView) {
    const { active: _a, activeName: _n, on: _o, value: _v, display: _d, shifted: _s, ...def } = $state.snapshot(c);
    draft = def;
  }

  async function save() {
    if (!draft) return;
    // Plain object, not the reactive proxy: it crosses into IPC (or the demo's structuredClone).
    const d = $state.snapshot(draft);
    if (await run(async () => (await backend()).saveVirtualControl(d))) draft = null;
  }

  function onOverlay(id: string): boolean {
    return snapshot.overlay.widgets.some((w) => w.type === "radio" && w.control === id);
  }

  const command = (id: string, c: ControlCommand) => run(async () => (await backend()).commandRadio(id, c));
  const setActive = (c: VirtualControlView, state: string | null) =>
    run(async () => (await backend()).setRadioActive(c.id, state));
  const setValue = (c: VirtualControlView) => {
    const v = Number(valueDraft[c.id]);
    if (Number.isFinite(v)) run(async () => (await backend()).setControlValue(c.id, v));
    delete valueDraft[c.id];
  };
  const typeName = (c: VirtualControlView) => CONTROL_TYPES.find(([t]) => t === c.kind.type)?.[1] ?? "";
</script>

<datalist id="detent-keys">
  {#each keys as k (k.name)}<option value={k.name}></option>{/each}
</datalist>

<section class="panel">
  <h2>Virtual controls</h2>
  <p class="muted small">
    Built from buttons, hat directions, axis zones and encoder detents; switching them can press keys or mouse buttons and
    run other actions. Radio: one of several states (or none). Toggle: on/off. Momentary: on while held. Counter: a value.
    Mode: a layer with exactly one state, which other triggers can depend on.
  </p>
  {#if error}<p class="error">{error}</p>{/if}

  {#each snapshot.virtualControls as c (c.id)}
    <div class="control" class:editing={draft?.id === c.id}>
      <div class="row">
        <span class="type muted">{typeName(c)}</span>
        <span class="cname">{c.name}</span>
        {#if c.kind.type === "radio" || c.kind.type === "mode"}
          <span class="active">{c.activeName ?? "–"}</span>
          {#if c.shifted}<span class="badge warn" title="Held in this state by a shift trigger">shift</span>{/if}
        {:else if c.kind.type === "toggle" || c.kind.type === "momentary"}
          <span class="lamp" class:on={c.on}></span><span class="active">{c.on ? "ON" : "OFF"}</span>
        {:else}
          <span class="active">{c.display}</span>
        {/if}
        <span class="spacer"></span>
        {#if c.kind.type === "radio" || c.kind.type === "mode" || c.kind.type === "counter"}
          <button class="small" title="Previous / down (runs actions)" onclick={() => command(c.id, "decrement")}>◀</button>
          <button class="small" title="Next / up (runs actions)" onclick={() => command(c.id, "increment")}>▶</button>
        {/if}
        {#if c.kind.type === "radio"}
          <button class="small" title="All off (runs actions)" onclick={() => command(c.id, "clear")}>Off</button>
          <button class="small" title="Toggle off: off, or back to the last active state (runs actions)" onclick={() => command(c.id, "toggleOff")}>Toggle</button>
        {:else if c.kind.type === "toggle"}
          <button class="small" title="Switch (runs actions)" onclick={() => command(c.id, "toggle")}>Toggle</button>
          <button class="small" title="Set without running actions" onclick={() => setActive(c, c.on ? null : "on")}>Set {c.on ? "off" : "on"} silently</button>
        {:else if c.kind.type === "counter"}
          <button class="small" title="Back to the initial value (runs actions)" onclick={() => command(c.id, "reset")}>Reset</button>
          <form class="row" onsubmit={(e) => { e.preventDefault(); setValue(c); }}>
            <input class="value" type="number" step="any" placeholder={c.display ?? ""} value={valueDraft[c.id] ?? ""} oninput={(e) => (valueDraft[c.id] = e.currentTarget.value)} title="Set the value without running actions" />
          </form>
        {/if}
        <button class="small" class:active={onOverlay(c.id)} onclick={() => run(async () => (await backend()).toggleOverlayVirtualControl(c.id))}>
          {onOverlay(c.id) ? "On overlay" : "Show on overlay"}
        </button>
        <button class="small" onclick={() => edit(c)}>Edit</button>
        <button class="small" onclick={() => run(async () => (await backend()).removeVirtualControl(c.id))}>Delete</button>
      </div>
      {#if c.kind.type === "radio" || c.kind.type === "mode"}
        <div class="states">
          {#each c.kind.states as s (s.id)}
            <button class="state" class:on={c.active === s.id} title="Set without running actions, e.g. to match the game" onclick={() => setActive(c, c.kind.type === "radio" && c.active === s.id ? null : s.id)}>{s.name}</button>
          {/each}
        </div>
      {/if}
    </div>
  {:else}
    <p class="muted">No virtual controls yet.</p>
  {/each}

  {#if draft}
    <ControlEditor {snapshot} bind:draft {keys} onsave={save} oncancel={() => (draft = null)} />
  {:else}
    <div class="row new">
      {#each CONTROL_TYPES as [type, text, hint] (type)}
        <button title={hint} onclick={() => (draft = blank(type))}>New {text.toLowerCase()}</button>
      {/each}
    </div>
    <p class="muted small">◀ ▶ Off Toggle Reset run actions like a trigger; clicking a state sets it silently.</p>
  {/if}
</section>

<style>
  .small { font-size: 12px; }
  .control { border-top: 1px solid var(--line); padding: 8px 0; }
  .control.editing { opacity: 0.6; }
  .type { font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; min-width: 6.5em; }
  .cname { font-size: 16px; min-width: 6em; }
  .active { font-size: 16px; font-weight: 700; color: var(--accent); font-variant-numeric: tabular-nums; }
  .lamp { width: 12px; height: 12px; border-radius: 50%; background: var(--lamp-off); }
  .lamp.on { background: var(--lamp-on); box-shadow: 0 0 6px var(--lamp-on); }
  .spacer { flex: 1; }
  .states { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 6px; }
  .state { font-size: 12px; border-radius: 12px; padding: 2px 10px; }
  .state.on { background: var(--accent); color: #111; border-color: var(--accent); font-weight: 600; }
  .new { margin-top: 8px; }
  input.value { width: 6em; font-size: 12px; padding: 1px 4px; }
</style>

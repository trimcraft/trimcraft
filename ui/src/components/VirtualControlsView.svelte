<script lang="ts">
  import { onMount } from "svelte";
  import type { EngineSnapshot, KeyInfo, RadioCommand, VirtualControlDef, VirtualControlView } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import RadioEditor from "./RadioEditor.svelte";
  import SoundpadStatusBadge from "./SoundpadStatusBadge.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let draft = $state<VirtualControlDef | null>(null);
  let error = $state("");
  let keys = $state<KeyInfo[]>([]);

  let usesSoundpad = $derived(
    snapshot.virtualControls.some((c) => c.kind.states.some((s) => [...s.onActivate, ...s.onDeactivate].some((a) => a.type === "soundpad"))),
  );

  onMount(() => {
    backend()
      .then((b) => b.keys())
      .then((k) => (keys = k))
      .catch((e) => (error = errorText(e)));
  });

  const blank = (): VirtualControlDef => ({
    id: "",
    name: "Radio",
    kind: {
      type: "radio",
      states: [1, 2, 3].map((n) => ({ id: "", name: `State ${n}`, select: [], onActivate: [], onDeactivate: [] })),
      increment: [],
      decrement: [],
      clear: [],
      toggleOff: [],
      end: "wrap",
      toggleOnReselect: false,
      startup: "restoreLast",
    },
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
    const { active: _a, activeName: _n, ...def } = $state.snapshot(c);
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

  const command = (id: string, c: RadioCommand) => run(async () => (await backend()).commandRadio(id, c));
  const setActive = (c: VirtualControlView, state: string) =>
    run(async () => (await backend()).setRadioActive(c.id, c.active === state ? null : state));
</script>

<datalist id="detent-keys">
  {#each keys as k (k.name)}<option value={k.name}></option>{/each}
</datalist>

<section class="panel">
  <h2>Radios</h2>
  <p class="muted small">
    A radio has several states of which at most one is active, like the preset buttons of a car radio. Buttons, hat
    directions or encoder detents switch states; entering and leaving a state can press keys or mouse buttons.
  </p>
  {#if error}<p class="error">{error}</p>{/if}
  {#if usesSoundpad}<div class="row soundpad"><SoundpadStatusBadge /></div>{/if}

  {#each snapshot.virtualControls as c (c.id)}
    <div class="control" class:editing={draft?.id === c.id}>
      <div class="row">
        <span class="cname">{c.name}</span>
        <span class="active">{c.activeName ?? "–"}</span>
        <span class="spacer"></span>
        <button class="small" title="Previous state (runs actions)" onclick={() => command(c.id, "decrement")}>◀</button>
        <button class="small" title="Next state (runs actions)" onclick={() => command(c.id, "increment")}>▶</button>
        <button class="small" title="All off (runs actions)" onclick={() => command(c.id, "clear")}>Off</button>
        <button class="small" title="Toggle off: off, or back to the last active state (runs actions)" onclick={() => command(c.id, "toggleOff")}>Toggle</button>
        <button class="small" class:active={onOverlay(c.id)} onclick={() => run(async () => (await backend()).toggleOverlayVirtualControl(c.id))}>
          {onOverlay(c.id) ? "On overlay" : "Show on overlay"}
        </button>
        <button class="small" onclick={() => edit(c)}>Edit</button>
        <button class="small" onclick={() => run(async () => (await backend()).removeVirtualControl(c.id))}>Delete</button>
      </div>
      <div class="states">
        {#each c.kind.states as s (s.id)}
          <button class="state" class:on={c.active === s.id} title="Set without running actions, e.g. to match the game" onclick={() => setActive(c, s.id)}>{s.name}</button>
        {/each}
      </div>
    </div>
  {:else}
    <p class="muted">No virtual controls yet.</p>
  {/each}

  {#if draft}
    <RadioEditor {snapshot} bind:draft {keys} onsave={save} oncancel={() => (draft = null)} />
  {:else}
    <div class="row new">
      <button onclick={() => (draft = blank())}>New radio</button>
      <span class="muted small">◀ ▶ Off Toggle run actions like a trigger; clicking a state sets it silently.</span>
    </div>
  {/if}
</section>

<style>
  .small { font-size: 12px; }
  .control { border-top: 1px solid var(--line); padding: 8px 0; }
  .control.editing { opacity: 0.6; }
  .cname { font-size: 16px; min-width: 6em; }
  .active { font-size: 16px; font-weight: 700; color: var(--accent); }
  .spacer { flex: 1; }
  .states { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 6px; }
  .state { font-size: 12px; border-radius: 12px; padding: 2px 10px; }
  .state.on { background: var(--accent); color: #111; border-color: var(--accent); font-weight: 600; }
  .new { margin-top: 8px; }
  .soundpad { margin-bottom: 6px; }
</style>

<script lang="ts">
  import type { EngineSnapshot, FunctionDef, FunctionView } from "../lib/types";
  import { backend, errorText } from "../lib/backend";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  const blank = (): FunctionDef => ({
    id: "",
    name: "Channel",
    kind: {
      type: "counter",
      spec: { min: 1, max: 20, step: 1, initial: 1, end: "wrap", decimals: 0 },
      input: snapshot.encoders[0] ? { type: "encoder", encoder: snapshot.encoders[0].id } : { type: "none" },
    },
  });

  let draft = $state<FunctionDef | null>(null);
  let error = $state("");

  function onOverlay(id: string): boolean {
    return snapshot.overlay.widgets.some((w) => w.type === "function" && w.function === id);
  }

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

  function edit(f: FunctionView) {
    const { value: _v, display: _d, ...def } = $state.snapshot(f);
    draft = def;
  }

  async function save() {
    if (!draft) return;
    // Plain object, not the reactive proxy: it crosses into IPC (or the demo's structuredClone).
    const d = $state.snapshot(draft);
    if (await run(async () => (await backend()).saveFunction(d))) draft = null;
  }

  function encoderSelection(): string {
    return draft?.kind.input.type === "encoder" ? draft.kind.input.encoder : "";
  }

  function setEncoder(id: string) {
    if (!draft) return;
    draft.kind.input = id ? { type: "encoder", encoder: id } : { type: "none" };
  }

  function encoderName(f: FunctionView): string {
    if (f.kind.input.type !== "encoder") return "no encoder";
    const id = f.kind.input.encoder;
    return snapshot.encoders.find((e) => e.id === id)?.name ?? id;
  }
</script>

<section class="panel">
  <h2>Counters</h2>
  <p class="muted small">A counter is a value with a range, e.g. a radio channel. Each encoder detent moves it one step.</p>
  {#if error}<p class="error">{error}</p>{/if}

  {#each snapshot.functions as f (f.id)}
    <div class="fn row">
      <span class="value">{f.name} <b>{f.display}</b></span>
      <span class="muted small">
        {f.kind.spec.min}–{f.kind.spec.max}, step {f.kind.spec.step}, {f.kind.spec.end === "wrap" ? "wraps around" : "stops at ends"} · {encoderName(f)}
      </span>
      <button class="small" class:active={onOverlay(f.id)} onclick={() => run(async () => (await backend()).toggleOverlayFunction(f.id))}>
        {onOverlay(f.id) ? "On overlay" : "Show on overlay"}
      </button>
      <button class="small" onclick={() => edit(f)}>Edit</button>
      <button class="small" onclick={() => run(async () => (await backend()).setFunctionValue(f.id, f.kind.spec.initial))}>Reset</button>
      <button class="small" onclick={() => run(async () => (await backend()).removeFunction(f.id))}>Delete</button>
    </div>
  {/each}

  {#if draft}
    <form class="editor" onsubmit={(e) => { e.preventDefault(); save(); }}>
      <label>Name <input bind:value={draft.name} /></label>
      <label>Minimum <input type="number" step="any" bind:value={draft.kind.spec.min} /></label>
      <label>Maximum <input type="number" step="any" bind:value={draft.kind.spec.max} /></label>
      <label>Step <input type="number" step="any" bind:value={draft.kind.spec.step} /></label>
      <label>Start value <input type="number" step="any" bind:value={draft.kind.spec.initial} /></label>
      <label>Decimals <input type="number" min="0" max="6" bind:value={draft.kind.spec.decimals} /></label>
      <label>At the end
        <select bind:value={draft.kind.spec.end}>
          <option value="wrap">wrap around</option>
          <option value="clamp">stop</option>
        </select>
      </label>
      <label>Encoder
        <select value={encoderSelection()} onchange={(e) => setEncoder(e.currentTarget.value)}>
          <option value="">none</option>
          {#each snapshot.encoders as e (e.id)}<option value={e.id}>{e.name}</option>{/each}
        </select>
      </label>
      <div class="row">
        <button class="primary" type="submit">{draft.id ? "Save" : "Create"}</button>
        <button type="button" onclick={() => (draft = null)}>Cancel</button>
      </div>
    </form>
  {:else}
    <button onclick={() => (draft = blank())}>New counter</button>
    {#if !snapshot.encoders.length}<span class="muted small">Set up an encoder first to drive it.</span>{/if}
  {/if}
</section>

<style>
  .small { font-size: 12px; }
  .fn { border-top: 1px solid var(--line); padding: 8px 0; }
  .value { font-size: 16px; min-width: 9em; }
  .editor { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 8px; margin-top: 8px; }
  .editor label { display: flex; flex-direction: column; gap: 2px; font-size: 12px; color: var(--muted); }
  .editor input, .editor select { width: 100%; }
</style>

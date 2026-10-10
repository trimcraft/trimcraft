<script lang="ts">
  // Editor for one radio. Works on a draft; nothing is saved until "Save".
  import type { EngineSnapshot, KeyInfo, RadioState, Trigger, VirtualControlDef } from "../lib/types";
  import { backend } from "../lib/backend";
  import { sameTrigger } from "../lib/format";
  import ActionList from "./ActionList.svelte";
  import TriggerSlot from "./TriggerSlot.svelte";

  let {
    snapshot,
    draft = $bindable(),
    keys,
    onsave,
    oncancel,
  }: {
    snapshot: EngineSnapshot;
    draft: VirtualControlDef;
    keys: KeyInfo[];
    onsave: () => void;
    oncancel: () => void;
  } = $props();

  /** Which trigger slot is learning: "increment", "decrement", "clear", "toggleOff" or "select:<state index>". */
  let learnKey = $state<string | null>(null);

  function slot(key: string): Trigger[] | undefined {
    const r = draft.kind;
    if (key === "increment") return r.increment;
    if (key === "decrement") return r.decrement;
    if (key === "clear") return r.clear;
    if (key === "toggleOff") return r.toggleOff;
    return r.states[Number(key.slice("select:".length))]?.select;
  }

  async function learn(key: string) {
    learnKey = key;
    await (await backend()).startLearn();
  }

  async function cancelLearn() {
    learnKey = null;
    await (await backend()).stopLearn();
  }

  // Put the captured trigger into the slot that asked for it, then leave learn mode.
  $effect(() => {
    const captured = snapshot.learn?.captured;
    if (!captured || learnKey === null) return;
    const target = slot(learnKey);
    if (target && !target.some((t) => sameTrigger(t, captured))) target.push(captured);
    learnKey = null;
    backend().then((b) => b.stopLearn());
  });

  // Leaving the editor must not leave the engine swallowing input.
  $effect(() => () => {
    if (learnKey !== null) backend().then((b) => b.stopLearn());
  });

  function addState() {
    const n = draft.kind.states.length + 1;
    const state: RadioState = { id: "", name: `State ${n}`, select: [], onActivate: [], onDeactivate: [] };
    draft.kind.states.push(state);
  }

  function moveState(i: number, by: number) {
    const s = draft.kind.states;
    const j = i + by;
    if (j < 0 || j >= s.length) return;
    if (learnKey?.startsWith("select:")) cancelLearn();
    [s[i], s[j]] = [s[j]!, s[i]!];
  }

  function removeState(i: number) {
    if (learnKey?.startsWith("select:")) cancelLearn();
    draft.kind.states.splice(i, 1);
  }
</script>

<!-- A div, not a form: the nested editors have many buttons that must not submit. -->
<div class="editor">
  <div class="row">
    <label class="name">Name <input bind:value={draft.name} placeholder="Radio" /></label>
  </div>

  <h3>Behaviour</h3>
  <div class="row settings">
    <label>At the ends
      <select bind:value={draft.kind.end}>
        <option value="wrap">wrap around</option>
        <option value="clamp">stop</option>
      </select>
    </label>
    <label>At startup
      <select bind:value={draft.kind.startup}>
        <option value="restoreLast">restore last state</option>
        <option value="empty">no state active</option>
      </select>
    </label>
    <label class="check"><input type="checkbox" bind:checked={draft.kind.toggleOnReselect} /> Selecting the active state again turns it off</label>
  </div>

  <h3>Triggers</h3>
  <TriggerSlot {snapshot} label="Next state" bind:triggers={draft.kind.increment} learning={learnKey === "increment"} onlearn={() => learn("increment")} oncancel={cancelLearn} />
  <TriggerSlot {snapshot} label="Previous state" bind:triggers={draft.kind.decrement} learning={learnKey === "decrement"} onlearn={() => learn("decrement")} oncancel={cancelLearn} />
  <TriggerSlot {snapshot} label="All off" bind:triggers={draft.kind.clear} learning={learnKey === "clear"} onlearn={() => learn("clear")} oncancel={cancelLearn} />
  <TriggerSlot {snapshot} label="Toggle off" bind:triggers={draft.kind.toggleOff} learning={learnKey === "toggleOff"} onlearn={() => learn("toggleOff")} oncancel={cancelLearn} />
  <p class="hint muted">Toggle off turns the active state off, or brings back the state that was active last.</p>

  <h3>States</h3>
  {#each draft.kind.states as state, i (i)}
    <div class="state">
      <div class="row">
        <span class="muted">{i + 1}</span>
        <input class="sname" bind:value={state.name} placeholder={`State ${i + 1}`} />
        <span class="spacer"></span>
        <button class="small" title="Move up" onclick={() => moveState(i, -1)}>↑</button>
        <button class="small" title="Move down" onclick={() => moveState(i, 1)}>↓</button>
        <button class="small" disabled={draft.kind.states.length <= 1} onclick={() => removeState(i)}>Delete</button>
      </div>
      <TriggerSlot {snapshot} label="Select this state" bind:triggers={state.select} learning={learnKey === `select:${i}`} onlearn={() => learn(`select:${i}`)} oncancel={cancelLearn} />
      <ActionList label="On activate" bind:actions={state.onActivate} {keys} {snapshot} />
      <ActionList label="On deactivate" bind:actions={state.onDeactivate} {keys} {snapshot} />
    </div>
  {/each}
  <div class="row">
    <button onclick={addState}>Add state</button>
  </div>

  <div class="row footer">
    <button class="primary" onclick={onsave}>{draft.id ? "Save" : "Create"}</button>
    <button onclick={oncancel}>Cancel</button>
    <span class="muted small">Editing never runs actions.</span>
  </div>
</div>

<style>
  .editor { display: flex; flex-direction: column; gap: 4px; margin-top: 8px; border-top: 1px solid var(--line); padding-top: 8px; }
  .editor :global(button) { font-size: 12px; }
  h3 { margin: 10px 0 2px; font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); }
  .name { display: flex; gap: 6px; align-items: center; color: var(--muted); }
  .settings label { display: flex; gap: 6px; align-items: center; color: var(--muted); font-size: 12px; }
  .check { cursor: pointer; }
  .state { border: 1px solid var(--line); border-radius: 6px; padding: 6px 8px; margin-bottom: 6px; background: var(--panel-2); }
  .state :global(input), .state :global(select), .state :global(button) { background: var(--panel); }
  .sname { min-width: 14em; font-weight: 600; }
  .spacer { flex: 1; }
  .footer { margin-top: 8px; }
  .hint { font-size: 11px; margin: 0 0 0 9em; padding-left: 8px; }
  .small { font-size: 12px; }
</style>

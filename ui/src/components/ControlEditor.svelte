<script lang="ts">
  // Editor for one virtual control of any kind (radio, toggle, momentary, counter, mode).
  // Works on a draft; nothing is saved until "Save".
  import type { EngineSnapshot, KeyInfo, VirtualControlDef } from "../lib/types";
  import { CONTROL_TYPES, triggerLists } from "../lib/format";
  import ActionList from "./ActionList.svelte";
  import TriggerSlot, { type ModeOption } from "./TriggerSlot.svelte";

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

  /** Modes of this profile for conditions; the draft itself while it is a mode (saved states only). */
  let modes = $derived.by((): ModeOption[] => {
    const list: ModeOption[] = snapshot.virtualControls
      .filter((c) => c.id !== draft.id && c.kind.type === "mode")
      .map((c) => ({ id: c.id, name: c.name, states: c.kind.type === "mode" ? c.kind.states.map((s) => ({ id: s.id, name: s.name })) : [] }));
    if (draft.id && draft.kind.type === "mode") {
      list.push({ id: draft.id, name: `${draft.name} (this mode)`, states: draft.kind.states.filter((s) => s.id).map((s) => ({ id: s.id, name: s.name })) });
    }
    return list;
  });

  /** Every other trigger of the profile, so a slot can tell when a plain press becomes a short press. */
  let others = $derived([
    ...snapshot.virtualControls.filter((c) => c.id !== draft.id).flatMap((c) => triggerLists(c.kind).flat()),
    ...triggerLists(draft.kind).flat(),
  ]);

  function addState() {
    const k = draft.kind;
    if (k.type === "radio") k.states.push({ id: "", name: `State ${k.states.length + 1}`, select: [], onActivate: [], onDeactivate: [] });
    if (k.type === "mode") k.states.push({ id: "", name: `State ${k.states.length + 1}`, select: [], shift: [], onActivate: [], onDeactivate: [] });
  }

  function moveState(i: number, by: number) {
    const k = draft.kind;
    if (k.type !== "radio" && k.type !== "mode") return;
    const s: unknown[] = k.states;
    const j = i + by;
    if (j < 0 || j >= s.length) return;
    [s[i], s[j]] = [s[j], s[i]];
  }

  function removeState(i: number) {
    const k = draft.kind;
    if (k.type === "radio" || k.type === "mode") k.states.splice(i, 1);
  }

  const typeName = $derived(CONTROL_TYPES.find(([t]) => t === draft.kind.type)?.[1] ?? "Control");
</script>

<!-- A div, not a form: the nested editors have many buttons that must not submit. -->
<div class="editor">
  <div class="row">
    <label class="name">{typeName} name <input bind:value={draft.name} placeholder={typeName} /></label>
  </div>

  {#if draft.kind.type === "radio"}
    {@const k = draft.kind}
    <h3>Behaviour</h3>
    <div class="row settings">
      <label>At the ends
        <select bind:value={k.end}>
          <option value="wrap">wrap around</option>
          <option value="clamp">stop</option>
        </select>
      </label>
      <label>At startup
        <select bind:value={k.startup}>
          <option value="restoreLast">restore last state</option>
          <option value="empty">no state active</option>
        </select>
      </label>
      <label class="check"><input type="checkbox" bind:checked={k.toggleOnReselect} /> Selecting the active state again turns it off</label>
    </div>
    <h3>Triggers</h3>
    <TriggerSlot {snapshot} {modes} {others} label="Next state" bind:triggers={k.increment} />
    <TriggerSlot {snapshot} {modes} {others} label="Previous state" bind:triggers={k.decrement} />
    <TriggerSlot {snapshot} {modes} {others} label="All off" bind:triggers={k.clear} />
    <TriggerSlot {snapshot} {modes} {others} label="Toggle off" bind:triggers={k.toggleOff} />
    <p class="hint muted">Toggle off turns the active state off, or brings back the state that was active last.</p>
  {:else if draft.kind.type === "toggle"}
    {@const k = draft.kind}
    <h3>Behaviour</h3>
    <div class="row settings">
      <label>At startup
        <select bind:value={k.startup}>
          <option value="restoreLast">as it was</option>
          <option value="empty">off</option>
        </select>
      </label>
    </div>
    <h3>Triggers</h3>
    <TriggerSlot {snapshot} {modes} {others} label="Toggle" bind:triggers={k.toggle} />
    <TriggerSlot {snapshot} {modes} {others} label="On" bind:triggers={k.on} />
    <TriggerSlot {snapshot} {modes} {others} label="Off" bind:triggers={k.off} />
    <h3>Actions</h3>
    <ActionList label="On switching on" bind:actions={k.onOn} {keys} {snapshot} />
    <ActionList label="On switching off" bind:actions={k.onOff} {keys} {snapshot} />
  {:else if draft.kind.type === "momentary"}
    {@const k = draft.kind}
    <h3>Behaviour</h3>
    <div class="row settings">
      <label class="check"><input type="checkbox" bind:checked={k.invert} /> Invert: on while nothing is held</label>
    </div>
    <h3>Triggers</h3>
    <TriggerSlot {snapshot} {modes} {others} hold label="While held" bind:triggers={k.triggers} />
    <p class="hint muted">On while any of these is held (a button, hat direction or axis zone); off when the last is released.</p>
    <h3>Actions</h3>
    <ActionList label={k.invert ? "On release (on)" : "On press"} bind:actions={k.onPress} {keys} {snapshot} />
    <ActionList label={k.invert ? "On press (off)" : "On release"} bind:actions={k.onRelease} {keys} {snapshot} />
  {:else if draft.kind.type === "counter"}
    {@const k = draft.kind}
    <h3>Range</h3>
    <div class="row settings">
      <label>Minimum <input type="number" step="any" bind:value={k.spec.min} /></label>
      <label>Maximum <input type="number" step="any" bind:value={k.spec.max} /></label>
      <label>Step <input type="number" step="any" bind:value={k.spec.step} /></label>
      <label>Initial <input type="number" step="any" bind:value={k.spec.initial} /></label>
      <label>Decimals <input type="number" min="0" max="6" bind:value={k.spec.decimals} /></label>
      <label>At the ends
        <select bind:value={k.spec.end}>
          <option value="wrap">wrap around</option>
          <option value="clamp">stop</option>
        </select>
      </label>
      <label>At startup
        <select bind:value={k.startup}>
          <option value="restoreLast">last value</option>
          <option value="empty">initial value</option>
        </select>
      </label>
    </div>
    <h3>Triggers</h3>
    <TriggerSlot {snapshot} {modes} {others} label="Increment" bind:triggers={k.increment} />
    <TriggerSlot {snapshot} {modes} {others} label="Decrement" bind:triggers={k.decrement} />
    <TriggerSlot {snapshot} {modes} {others} label="Reset" bind:triggers={k.reset} />
    <h3>Actions</h3>
    <ActionList label="On change" bind:actions={k.onChange} {keys} {snapshot} />
    <p class="hint muted">Actions that take text can use <span class="mono">{"{value}"}</span> for the new value.</p>
  {:else if draft.kind.type === "mode"}
    {@const k = draft.kind}
    <h3>Behaviour</h3>
    <div class="row settings">
      <label>At the ends
        <select bind:value={k.end}>
          <option value="wrap">wrap around</option>
          <option value="clamp">stop</option>
        </select>
      </label>
      <label>At startup
        <select bind:value={k.startup}>
          <option value="restoreLast">restore last state</option>
          <option value="empty">first state</option>
        </select>
      </label>
    </div>
    <p class="hint muted">Exactly one state is active, the first by default. Any trigger of any control can be limited to a state of this mode (“Only when”).</p>
    <h3>Triggers</h3>
    <TriggerSlot {snapshot} {modes} {others} label="Next state" bind:triggers={k.next} />
    <TriggerSlot {snapshot} {modes} {others} label="Previous state" bind:triggers={k.previous} />
  {/if}

  {#if draft.kind.type === "radio" || draft.kind.type === "mode"}
    {@const k = draft.kind}
    <h3>States</h3>
    {#each k.states as state, i (i)}
      <div class="state">
        <div class="row">
          <span class="muted">{i + 1}</span>
          <input class="sname" bind:value={state.name} placeholder={`State ${i + 1}`} />
          <span class="spacer"></span>
          <button class="small" title="Move up" onclick={() => moveState(i, -1)}>↑</button>
          <button class="small" title="Move down" onclick={() => moveState(i, 1)}>↓</button>
          <button class="small" disabled={k.states.length <= 1} onclick={() => removeState(i)}>Delete</button>
        </div>
        <TriggerSlot {snapshot} {modes} {others} label="Select this state" bind:triggers={state.select} />
        {#if "shift" in state}
          <TriggerSlot {snapshot} {modes} {others} hold label="While held (shift)" bind:triggers={state.shift} />
        {/if}
        <ActionList label="On activate" bind:actions={state.onActivate} {keys} {snapshot} />
        <ActionList label="On deactivate" bind:actions={state.onDeactivate} {keys} {snapshot} />
      </div>
    {/each}
    <div class="row">
      <button onclick={addState}>Add state</button>
    </div>
  {/if}

  <div class="row footer">
    <button class="primary" onclick={onsave}>{draft.id ? "Save" : "Create"}</button>
    <button onclick={oncancel}>Cancel</button>
    <span class="muted small">Editing never runs actions. Click a trigger to set long/double press, modifiers and conditions.</span>
  </div>
</div>

<style>
  .editor { display: flex; flex-direction: column; gap: 4px; margin-top: 8px; border-top: 1px solid var(--line); padding-top: 8px; }
  .editor :global(button) { font-size: 12px; }
  h3 { margin: 10px 0 2px; font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); }
  .name { display: flex; gap: 6px; align-items: center; color: var(--muted); }
  .settings label { display: flex; gap: 6px; align-items: center; color: var(--muted); font-size: 12px; }
  .settings input[type="number"] { width: 6em; }
  .check { cursor: pointer; }
  .state { border: 1px solid var(--line); border-radius: 6px; padding: 6px 8px; margin-bottom: 6px; background: var(--panel-2); }
  .state :global(input), .state :global(select), .state :global(button) { background: var(--panel); }
  .sname { min-width: 14em; font-weight: 600; }
  .spacer { flex: 1; }
  .footer { margin-top: 8px; }
  .hint { font-size: 11px; margin: 0 0 0 9em; padding-left: 8px; }
  .small { font-size: 12px; }
</style>

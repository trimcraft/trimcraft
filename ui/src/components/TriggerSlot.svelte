<script lang="ts" module>
  /** A mode a trigger condition can refer to. */
  export interface ModeOption {
    id: string;
    name: string;
    states: { id: string; name: string }[];
  }
</script>

<script lang="ts">
  // A list of triggers (0..n) with "learn" and a manual pick list. Each trigger can be opened
  // to set how it is pressed (press, release, long, double), modifier buttons and mode
  // conditions; axis zones get their range on a live axis bar.
  import type { EngineSnapshot, HatDirection, PressKind, StepDirection, Trigger } from "../lib/types";
  import { HAT_DIRECTIONS, buttonRefText, conditionText, deviceTitle, isDigital, newTrigger, sameInput, sameTrigger, triggerText } from "../lib/format";
  import { learning, startLearn, stopLearn } from "../lib/learn.svelte";

  let {
    snapshot,
    label,
    triggers = $bindable(),
    hold = false,
    modes = [],
    others = [],
  }: {
    snapshot: EngineSnapshot;
    label: string;
    triggers: Trigger[];
    /** The command lasts while held (momentary, mode shift): no release triggers, no encoders. */
    hold?: boolean;
    /** Modes of the profile, for conditions. */
    modes?: ModeOption[];
    /** All other triggers of the profile, to tell when a plain press becomes a short press. */
    others?: Trigger[];
  } = $props();

  // Unique per slot instance, for the shared learn state.
  const slotKey = `slot-${Math.random().toString(36).slice(2)}`;

  type Kind = Trigger["type"];
  let picking = $state(false);
  let kind = $state<Kind>("button");
  let device = $state("");
  let index = $state(1);
  let hatDirection = $state<HatDirection>("up");
  let encoder = $state("");
  let stepDirection = $state<StepDirection>("increment");
  let open = $state<number | null>(null);

  let inputDevices = $derived(snapshot.devices.filter((d) => d.kind.type === "physical"));

  function openPicker() {
    device = device || inputDevices[0]?.id || "";
    encoder = encoder || snapshot.encoders[0]?.id || "";
    picking = true;
  }

  function push(t: Trigger) {
    if (!triggers.some((x) => sameTrigger(x, t))) {
      triggers.push(t);
      // Axis zones need their range set; open them right away.
      if (t.type === "axisZone") open = triggers.length - 1;
    }
  }

  function add() {
    if (kind === "encoderStep") {
      if (!encoder) return;
      push(newTrigger({ type: "encoderStep", encoder, direction: stepDirection }));
    } else {
      if (!device) return;
      // Shown 1-based like Windows, stored 0-based.
      const i = Math.max(0, Math.round(index) - 1);
      if (kind === "button") push(newTrigger({ type: "button", device, index: i }));
      else if (kind === "hat") push(newTrigger({ type: "hat", device, index: i, direction: hatDirection }));
      else push(newTrigger({ type: "axisZone", device, index: i, min: 80, max: 100 }));
    }
    picking = false;
  }

  const learnKey = (what: string) => `${slotKey}:${what}`;

  function learnTrigger() {
    startLearn(learnKey("trigger"), (t) => push(t));
  }

  function learnModifier(i: number) {
    startLearn(learnKey(`mod:${i}`), (t) => {
      const target = triggers[i];
      if (!target || t.type !== "button") return;
      if (!target.modifiers.some((m) => m.device === t.device && m.index === t.index)) {
        target.modifiers.push({ device: t.device, index: t.index });
      }
    });
  }

  // Leaving the editor must not leave the engine swallowing input.
  $effect(() => () => {
    if (learning.key?.startsWith(slotKey)) stopLearn();
  });

  type PressChoice = PressKind["kind"];
  function setPress(t: Trigger, choice: PressChoice) {
    if (choice === "long") t.press = { kind: "long", ms: t.press.kind === "long" ? t.press.ms : 500 };
    else if (choice === "double") t.press = { kind: "double", ms: t.press.kind === "double" ? t.press.ms : 300 };
    else t.press = { kind: choice };
  }

  function setMs(t: Trigger, ms: number) {
    if (t.press.kind === "long" || t.press.kind === "double") t.press = { ...t.press, ms: Math.round(ms) || t.press.ms };
  }

  /** A plain press on a button that also has long or double triggers fires on release. */
  function shortPress(t: Trigger): boolean {
    if (hold || !isDigital(t) || t.press.kind !== "press") return false;
    return [...triggers, ...others].some((o) => sameInput(o, t) && (o.press.kind === "long" || o.press.kind === "double"));
  }

  function axisValue(t: Trigger): number | null {
    if (t.type !== "axisZone") return null;
    const v = snapshot.devices.find((d) => d.id === t.device)?.state.axes[t.index];
    return v === undefined ? null : ((Math.max(-1, Math.min(1, v)) + 1) / 2) * 100;
  }

  function addCondition(t: Trigger, modeId: string) {
    const mode = modes.find((m) => m.id === modeId);
    const state = mode?.states[0];
    if (mode && state) t.conditions.push({ mode: mode.id, state: state.id });
  }

  const warning = (t: Trigger) =>
    hold && t.type === "encoderStep" ? "encoder detents cannot be held" : hold && t.press.kind === "release" ? "a release cannot be held" : "";
</script>

<div class="slot">
  <span class="label">{label}</span>
  <div class="list">
    {#each triggers as t, i (i)}
      <span class="chip" class:open={open === i} class:bad={!!warning(t)}>
        <button class="link" title="Press kind, modifiers, conditions" onclick={() => (open = open === i ? null : i)}>{triggerText(snapshot, t)}</button>
        {#if shortPress(t)}<span class="tag" title="This button also has a long or double press trigger, so this plain press fires on release (short press)">short</span>{/if}
        <button class="x" title="Remove" onclick={() => { triggers.splice(i, 1); open = null; }}>×</button>
      </span>
    {:else}
      <span class="muted small">none</span>
    {/each}
    {#if learning.key === learnKey("trigger")}
      <span class="badge warn">Press a button, move a hat or an axis, or turn an encoder…</span>
      <button class="small" onclick={stopLearn}>Cancel</button>
    {:else}
      <button class="small" onclick={learnTrigger}>Assign</button>
      <button class="small" onclick={() => (picking ? (picking = false) : openPicker())}>Pick…</button>
    {/if}
  </div>
  {#if picking}
    <div class="picker row">
      <select bind:value={kind}>
        <option value="button">Button</option>
        <option value="hat">Hat direction</option>
        <option value="axisZone">Axis zone</option>
        {#if !hold}<option value="encoderStep">Encoder detent</option>{/if}
      </select>
      {#if kind === "encoderStep"}
        <select bind:value={encoder}>
          {#each snapshot.encoders as e (e.id)}<option value={e.id}>{e.name}</option>{:else}<option value="">no encoders set up</option>{/each}
        </select>
        <select bind:value={stepDirection}>
          <option value="increment">+ (increment)</option>
          <option value="decrement">− (decrement)</option>
        </select>
      {:else}
        <select bind:value={device}>
          {#each inputDevices as d (d.id)}<option value={d.id}>{deviceTitle(d)} ({d.id})</option>{/each}
        </select>
        <label class="small">{kind === "button" ? "Button" : kind === "hat" ? "Hat" : "Axis"} <input type="number" min="1" bind:value={index} /></label>
        {#if kind === "hat"}
          <select bind:value={hatDirection}>
            {#each HAT_DIRECTIONS as [value, text] (value)}<option {value}>{text}</option>{/each}
          </select>
        {/if}
      {/if}
      <button class="small primary" onclick={add}>Add</button>
    </div>
  {/if}
  {#each triggers as t, i (i)}
    {#if open === i}{@render details(t, i)}{/if}
  {/each}
</div>

{#snippet details(t: Trigger, i: number)}
    <div class="details">
      {#if warning(t)}<p class="error small">This control lasts while held: {warning(t)}.</p>{/if}
      {#if t.type === "axisZone"}
        {@const live = axisValue(t)}
        <div class="row">
          <span class="muted small">Zone</span>
          <label class="small">from <input type="number" min="0" max="100" step="1" bind:value={t.min} /> %</label>
          <label class="small">to <input type="number" min="0" max="100" step="1" bind:value={t.max} /> %</label>
          <select value={t.press.kind} onchange={(e) => setPress(t, e.currentTarget.value as PressChoice)}>
            <option value="press">on entering</option>
            {#if !hold}<option value="release">on leaving</option>{/if}
          </select>
        </div>
        <div class="zonebar" title="The axis now; the zone is highlighted (it is left only 2 % outside)">
          <div class="zone" style:left="{Math.min(t.min, t.max)}%" style:width="{Math.abs(t.max - t.min)}%"></div>
          {#if live !== null}<div class="now" style:left="{live}%"></div>{/if}
        </div>
        <div class="row">
          <span class="muted small">Now {live === null ? "–" : `${Math.round(live)} %`}</span>
          {#if live !== null}
            <button class="small" onclick={() => { t.min = Math.max(0, Math.round(live! - 10)); t.max = Math.min(100, Math.round(live! + 10)); }}>Zone around here</button>
            <button class="small" onclick={() => (t.min = Math.round(live!))}>From here</button>
            <button class="small" onclick={() => (t.max = Math.round(live!))}>To here</button>
          {/if}
        </div>
      {:else if t.type !== "encoderStep"}
        <div class="row">
          <span class="muted small">Press</span>
          <select value={t.press.kind} onchange={(e) => setPress(t, e.currentTarget.value as PressChoice)}>
            <option value="press">{hold ? "while held" : "press"}</option>
            {#if !hold}<option value="release">release</option>{/if}
            <option value="long">{hold ? "after a long press, while held" : "long press"}</option>
            <option value="double">{hold ? "after a double press, while held" : "double press"}</option>
          </select>
          {#if t.press.kind === "long" || t.press.kind === "double"}
            <label class="small"><input type="number" min="50" max="5000" step="50" value={t.press.ms} onchange={(e) => setMs(t, Number(e.currentTarget.value))} /> ms</label>
            <span class="muted small">{t.press.kind === "long" ? "held at least this long" : "second press within this time"}</span>
          {/if}
        </div>
        {#if !hold && (t.press.kind === "long" || t.press.kind === "double")}
          <p class="hint">Plain presses of this button then fire on release (short press), so they do not fire together with the long or double press.</p>
        {/if}
      {/if}
      <div class="row">
        <span class="muted small">Modifiers</span>
        {#each t.modifiers as m, mi (mi)}
          <span class="chip">{buttonRefText(snapshot, m)}<button class="x" title="Remove" onclick={() => t.modifiers.splice(mi, 1)}>×</button></span>
        {:else}
          <span class="muted small">none</span>
        {/each}
        {#if learning.key === learnKey(`mod:${i}`)}
          <span class="badge warn">Press the modifier button…</span>
          <button class="small" onclick={stopLearn}>Cancel</button>
        {:else}
          <button class="small" onclick={() => learnModifier(i)}>+ Modifier</button>
        {/if}
      </div>
      {#if t.modifiers.length}<p class="hint">Fires only while the modifiers are held; the same input with fewer modifiers then does not fire.</p>{/if}
      <div class="row">
        <span class="muted small">Only when</span>
        {#each t.conditions as c, ci (ci)}
          {@const mode = modes.find((m) => m.id === c.mode)}
          <span class="chip">
            {#if mode}
              {mode.name} =
              <select class="inline" bind:value={c.state}>
                {#each mode.states as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
              </select>
            {:else}
              <span class="error">{conditionText(snapshot, c)}</span>
            {/if}
            <button class="x" title="Remove" onclick={() => t.conditions.splice(ci, 1)}>×</button>
          </span>
        {:else}
          <span class="muted small">always</span>
        {/each}
        {#if modes.some((m) => m.states.length)}
          <select class="inline" value="" onchange={(e) => { addCondition(t, e.currentTarget.value); e.currentTarget.value = ""; }}>
            <option value="" disabled>+ Mode condition</option>
            {#each modes.filter((m) => m.states.length) as m (m.id)}<option value={m.id}>{m.name}</option>{/each}
          </select>
        {:else}
          <span class="muted small">(create a Mode control to use conditions)</span>
        {/if}
      </div>
    </div>
{/snippet}

<style>
  .slot { display: grid; grid-template-columns: 9em 1fr; gap: 4px 8px; align-items: start; padding: 4px 0; }
  .label { color: var(--muted); font-size: 12px; padding-top: 3px; }
  .list { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .chip { display: inline-flex; align-items: center; gap: 4px; background: var(--panel-2); border: 1px solid var(--line); border-radius: 10px; padding: 1px 4px 1px 8px; font-size: 12px; }
  .chip.open { border-color: var(--accent); }
  .chip.bad { border-color: var(--bad); }
  .link { border: none; background: none; padding: 0; font-size: 12px; text-align: left; }
  .tag { font-size: 10px; color: var(--warn); border: 1px solid var(--warn); border-radius: 8px; padding: 0 4px; }
  .x { border: none; background: none; padding: 0 4px; color: var(--muted); }
  .x:hover { color: var(--bad); }
  .picker, .details { grid-column: 2; }
  .details { display: flex; flex-direction: column; gap: 4px; border-left: 2px solid var(--accent-dim); padding: 4px 0 4px 8px; }
  .small { font-size: 12px; }
  .picker label, .details label { display: flex; gap: 4px; align-items: center; color: var(--muted); }
  .details input[type="number"] { width: 5em; }
  .hint { font-size: 11px; color: var(--muted); margin: 0; }
  select.inline { padding: 0 4px; font-size: 12px; }
  .zonebar { position: relative; height: 12px; background: var(--lamp-off); border-radius: 6px; max-width: 420px; }
  .zone { position: absolute; top: 0; bottom: 0; background: var(--accent-dim); border: 1px solid var(--accent); border-radius: 6px; }
  .now { position: absolute; top: -3px; bottom: -3px; width: 3px; margin-left: -1px; background: var(--text); border-radius: 2px; }
</style>

<script lang="ts">
  // A list of triggers (0..n) with "learn" and a manual pick list.
  import type { EngineSnapshot, HatDirection, StepDirection, Trigger } from "../lib/types";
  import { HAT_DIRECTIONS, deviceTitle, sameTrigger, triggerText } from "../lib/format";

  let {
    snapshot,
    label,
    triggers = $bindable(),
    learning,
    onlearn,
    oncancel,
  }: {
    snapshot: EngineSnapshot;
    label: string;
    triggers: Trigger[];
    learning: boolean;
    onlearn: () => void;
    oncancel: () => void;
  } = $props();

  type Kind = Trigger["type"];
  let picking = $state(false);
  let kind = $state<Kind>("button");
  let device = $state("");
  let index = $state(1);
  let hatDirection = $state<HatDirection>("up");
  let encoder = $state("");
  let stepDirection = $state<StepDirection>("increment");

  let inputDevices = $derived(snapshot.devices.filter((d) => d.kind.type === "physical"));

  function openPicker() {
    device = device || inputDevices[0]?.id || "";
    encoder = encoder || snapshot.encoders[0]?.id || "";
    picking = true;
  }

  function add() {
    let t: Trigger;
    if (kind === "encoderStep") {
      if (!encoder) return;
      t = { type: "encoderStep", encoder, direction: stepDirection };
    } else {
      if (!device) return;
      // Shown 1-based like Windows, stored 0-based.
      const i = Math.max(0, Math.round(index) - 1);
      t = kind === "button" ? { type: "button", device, index: i } : { type: "hat", device, index: i, direction: hatDirection };
    }
    if (!triggers.some((x) => sameTrigger(x, t))) triggers.push(t);
    picking = false;
  }
</script>

<div class="slot">
  <span class="label">{label}</span>
  <div class="list">
    {#each triggers as t, i (i)}
      <span class="chip">{triggerText(snapshot, t)}<button class="x" title="Remove" onclick={() => triggers.splice(i, 1)}>×</button></span>
    {:else}
      <span class="muted small">none</span>
    {/each}
    {#if learning}
      <span class="badge warn">Press a button, move a hat or turn an encoder…</span>
      <button class="small" onclick={oncancel}>Cancel</button>
    {:else}
      <button class="small" onclick={onlearn}>Assign</button>
      <button class="small" onclick={() => (picking ? (picking = false) : openPicker())}>Pick…</button>
    {/if}
  </div>
  {#if picking}
    <div class="picker row">
      <select bind:value={kind}>
        <option value="button">Button</option>
        <option value="hat">Hat direction</option>
        <option value="encoderStep">Encoder detent</option>
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
        <label class="small">{kind === "button" ? "Button" : "Hat"} <input type="number" min="1" bind:value={index} /></label>
        {#if kind === "hat"}
          <select bind:value={hatDirection}>
            {#each HAT_DIRECTIONS as [value, text] (value)}<option {value}>{text}</option>{/each}
          </select>
        {/if}
      {/if}
      <button class="small primary" onclick={add}>Add</button>
    </div>
  {/if}
</div>

<style>
  .slot { display: grid; grid-template-columns: 9em 1fr; gap: 4px 8px; align-items: start; padding: 4px 0; }
  .label { color: var(--muted); font-size: 12px; padding-top: 3px; }
  .list { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .chip { display: inline-flex; align-items: center; gap: 4px; background: var(--panel-2); border: 1px solid var(--line); border-radius: 10px; padding: 1px 4px 1px 8px; font-size: 12px; }
  .x { border: none; background: none; padding: 0 4px; color: var(--muted); }
  .x:hover { color: var(--bad); }
  .picker { grid-column: 2; }
  .small { font-size: 12px; }
  .picker label { display: flex; gap: 4px; align-items: center; color: var(--muted); }
</style>

<script lang="ts">
  // vJoy mappings of the active profile: physical axes, buttons and hats passed through to
  // vJoy. Axes get invert, deadzones, saturation and a response curve, with a live graph.
  // Edits are a draft until "Save mappings".
  import type { EngineSnapshot, VjoyMapping } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import { applyCurve, controlName, defaultCurve, deviceTitle, findDevice, vjoyAxisName } from "../lib/format";
  import { learning, startLearn, stopLearn } from "../lib/learn.svelte";
  import VjoyTarget from "./VjoyTarget.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let draft = $state<VjoyMapping[] | null>(null);
  let error = $state("");
  const list = $derived(draft ?? snapshot.vjoyMappings);
  const enabled = $derived(snapshot.connections.vjoy.enabled);
  const inputs = $derived(snapshot.devices.filter((d) => d.kind.type === "physical"));
  const LEARN = "vjoy-mapping";

  function edit(): VjoyMapping[] {
    draft ??= $state.snapshot(snapshot.vjoyMappings);
    return draft;
  }

  function add(type: VjoyMapping["type"], device = inputs[0]?.id ?? "", index = 0) {
    const m: VjoyMapping =
      type === "axis"
        ? { type, device, index, target: { device: 1, axis: "x" }, curve: defaultCurve() }
        : type === "button"
          ? { type, device, index, target: { device: 1, button: 1 } }
          : { type, device, index, target: { device: 1, pov: 1 } };
    edit().push(m);
  }

  /** Move an axis, press a button or a hat: a mapping for it. */
  function learn() {
    startLearn(LEARN, (t) => {
      if (t.type === "axisZone") add("axis", t.device, t.index);
      else if (t.type === "button") add("button", t.device, t.index);
      else if (t.type === "hat") add("hat", t.device, t.index);
    });
  }

  async function save() {
    if (!draft) return;
    error = "";
    const d = $state.snapshot(draft);
    try {
      await (await backend()).setVjoyMappings(d);
      draft = null;
    } catch (e) {
      error = errorText(e);
    }
  }

  const sourceValue = (m: VjoyMapping) => findDevice(snapshot, m.device)?.state.axes[m.index] ?? 0;
  const sourceName = (m: VjoyMapping) =>
    `${deviceTitle(findDevice(snapshot, m.device), m.device)} · ${controlName({ type: m.type, index: m.index })}`;

  /** The curve as an SVG path over -1..1 → -1..1, in a 100×100 box. */
  function curvePath(m: Extract<VjoyMapping, { type: "axis" }>): string {
    const pts: string[] = [];
    for (let i = 0; i <= 50; i++) {
      const x = -1 + i / 25;
      const y = applyCurve(m.curve, x);
      pts.push(`${(x + 1) * 50},${100 - (y + 1) * 50}`);
    }
    return `M${pts.join(" L")}`;
  }

  const pct = (v: number) => Math.round((v + 1) * 50);
</script>

<section class="panel">
  <h2>vJoy mappings</h2>
  <p class="muted small">
    Physical axes, buttons and hats passed straight through to vJoy, for this profile. Each axis move is written as it
    happens. {#if !enabled}<b>The vJoy connection is disabled</b> (Connections): mappings are kept but not written.{/if}
  </p>
  {#if error}<p class="error">{error}</p>{/if}

  {#each list as m, i (i)}
    <div class="mapping">
      <div class="row">
        <select value={m.device} disabled={!draft} onchange={(e) => (edit()[i]!.device = e.currentTarget.value)}>
          {#each inputs as d (d.id)}<option value={d.id}>{deviceTitle(d)}</option>{/each}
          {#if !inputs.some((d) => d.id === m.device)}<option value={m.device}>{m.device} (not connected)</option>{/if}
        </select>
        <label class="small">{m.type === "axis" ? "Axis" : m.type === "button" ? "Button" : "Hat"}
          <input type="number" min="1" value={m.index + 1} disabled={!draft} onchange={(e) => (edit()[i]!.index = Math.max(0, Number(e.currentTarget.value) - 1))} /></label>
        <span class="arrow">→</span>
        {#if draft}
          {@const dm = draft[i]!}
          <VjoyTarget {snapshot} kind={dm.type === "hat" ? "pov" : dm.type} bind:target={dm.target} />
        {:else}
          <span class="mono small">vJoy {m.target.device} {"axis" in m.target ? vjoyAxisName(m.target.axis) : "button" in m.target ? `button ${m.target.button}` : `POV ${m.target.pov}`}</span>
        {/if}
        <span class="spacer"></span>
        <button class="small" onclick={() => edit().splice(i, 1)}>Remove</button>
      </div>
      {#if m.type === "axis"}
        {@const input = sourceValue(m)}
        {@const output = applyCurve(m.curve, input)}
        <div class="axis">
          <svg viewBox="-2 -2 104 104" class="graph" aria-label="Response curve of {sourceName(m)}">
            <rect x="0" y="0" width="100" height="100" class="frame" />
            <line x1="50" y1="0" x2="50" y2="100" class="grid" />
            <line x1="0" y1="50" x2="100" y2="50" class="grid" />
            <path d={curvePath(m)} class="curve" />
            <circle cx={(input + 1) * 50} cy={100 - (output + 1) * 50} r="3" class="dot" />
          </svg>
          <div class="settings">
            {#if draft}
              {@const c = (draft[i] as Extract<VjoyMapping, { type: "axis" }>).curve}
              <label><input type="checkbox" bind:checked={c.invert} /> Invert</label>
              <label>Centre deadzone <input type="number" min="0" max="90" step="1" bind:value={c.deadzoneCenter} /> %</label>
              <label>End deadzones <input type="number" min="0" max="45" step="1" bind:value={c.deadzoneLow} /> /
                <input type="number" min="0" max="45" step="1" bind:value={c.deadzoneHigh} /> %</label>
              <label>Saturation <input type="number" min="1" max="100" step="1" bind:value={c.saturation} /> %</label>
              <label>Curve
                <select value={c.curve.type} onchange={(e) => (c.curve = e.currentTarget.value === "expo" ? { type: "expo", factor: 0.5 } : { type: "linear" })}>
                  <option value="linear">linear</option>
                  <option value="expo">expo</option>
                </select>
                {#if c.curve.type === "expo"}<input type="range" min="0" max="1" step="0.05" bind:value={c.curve.factor} /> {c.curve.factor}{/if}
              </label>
            {:else}
              <span class="muted small">
                {m.curve.invert ? "inverted · " : ""}deadzone {m.curve.deadzoneCenter} % centre, {m.curve.deadzoneLow}/{m.curve.deadzoneHigh} % ends ·
                saturation {m.curve.saturation} % · {m.curve.curve.type === "expo" ? `expo ${m.curve.curve.factor}` : "linear"}
              </span>
            {/if}
            <span class="muted small">Now: input {pct(input)} % → output {pct(output)} %</span>
          </div>
        </div>
      {/if}
    </div>
  {:else}
    <p class="muted">No mappings in this profile.</p>
  {/each}

  {#if enabled}
    <p class="muted small out">
      vJoy output now:
      {#each snapshot.vjoyOutput.devices as d (d)}
        {@const buttons = snapshot.vjoyOutput.buttons.filter((b) => b.device === d).map((b) => b.button)}
        {@const axes = snapshot.vjoyOutput.axes.filter((a) => a.axis.device === d)}
        {@const povs = snapshot.vjoyOutput.povs.filter((p) => p.pov.device === d)}
        <span class="dev"><b>vJoy {d}</b>
          {axes.map((a) => `${vjoyAxisName(a.axis.axis)} ${pct(a.value)} %`).join(", ")}
          {buttons.length ? ` buttons ${buttons.join(", ")}` : ""}
          {povs.map((p) => ` POV ${p.pov.pov} ${p.direction}`).join(", ")}
          {!axes.length && !buttons.length && !povs.length ? "idle" : ""}</span>
      {:else}
        nothing (no device in use in this profile)
      {/each}
    </p>
  {/if}

  <div class="row footer">
    {#if learning.key === LEARN}
      <span class="badge warn">Move an axis, press a button or move a hat…</span>
      <button class="small" onclick={stopLearn}>Cancel</button>
    {:else}
      <button class="small" onclick={learn}>Assign by input</button>
    {/if}
    <button class="small" onclick={() => add("axis")}>+ Axis</button>
    <button class="small" onclick={() => add("button")}>+ Button</button>
    <button class="small" onclick={() => add("hat")}>+ Hat → POV</button>
    {#if draft}
      <span class="spacer"></span>
      <button class="primary small" onclick={save}>Save mappings</button>
      <button class="small" onclick={() => (draft = null)}>Discard</button>
    {:else if list.length}
      <span class="spacer"></span>
      <button class="small" onclick={() => edit()}>Edit</button>
    {/if}
  </div>
</section>

<style>
  .small { font-size: 12px; }
  .mapping { border-top: 1px solid var(--line); padding: 8px 0; display: flex; flex-direction: column; gap: 6px; }
  .arrow { color: var(--muted); }
  .spacer { flex: 1; }
  label { display: inline-flex; gap: 4px; align-items: center; color: var(--muted); font-size: 12px; }
  input[type="number"] { width: 4.5em; }
  .axis { display: flex; gap: 16px; align-items: flex-start; flex-wrap: wrap; }
  .settings { display: flex; flex-direction: column; gap: 4px; }
  .graph { width: 140px; height: 140px; background: var(--panel-2); border-radius: 4px; }
  .frame { fill: none; stroke: var(--line); stroke-width: 1; }
  .grid { stroke: var(--line); stroke-width: 0.5; }
  .curve { fill: none; stroke: var(--accent); stroke-width: 2; }
  .dot { fill: var(--text); }
  .footer { margin-top: 8px; }
  .out { margin: 6px 0 0; }
  .dev { margin-right: 12px; }
</style>

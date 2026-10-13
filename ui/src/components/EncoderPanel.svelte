<script lang="ts">
  import type { ButtonPair, DetectorConfig, EncoderView, EngineSnapshot, PulseStats } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import { deviceTitle, findDevice, ms } from "../lib/format";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let error = $state("");
  let manualDevice = $state("");
  let manualMinus = $state(1);
  let manualPlus = $state(2);
  let manualName = $state("");
  // Edited locally: the snapshot is replaced many times per second.
  let detectorDraft = $state<DetectorConfig | null>(null);

  let deviceOptions = $derived(snapshot.devices.filter((d) => d.state.buttons.length > 1 && d.kind.type === "physical"));
  let lastButton = $derived.by(() => {
    const a = snapshot.lastActivity;
    if (!a || a.control.type !== "button") return null;
    return { device: a.device, number: a.control.index + 1 };
  });

  const title = (id: string) => deviceTitle(findDevice(snapshot, id), id);

  async function run(action: () => Promise<unknown>) {
    error = "";
    try {
      await action();
    } catch (e) {
      error = errorText(e);
    }
  }

  const accept = (p: ButtonPair) => run(async () => (await backend()).addEncoder("", p.device, p.decrement, p.increment));
  const dismiss = (p: ButtonPair) => run(async () => (await backend()).dismissSuggestion(p));
  const swap = (e: EncoderView) => run(async () => (await backend()).swapEncoder(e.id));
  const remove = (e: EncoderView) => run(async () => (await backend()).removeEncoder(e.id));
  const rename = (e: EncoderView, name: string) => run(async () => (await backend()).renameEncoder(e.id, name));

  function addManual() {
    const device = manualDevice || deviceOptions[0]?.id;
    if (!device) return;
    run(async () => {
      await (await backend()).addEncoder(manualName, device, manualMinus - 1, manualPlus - 1);
      manualName = "";
    });
  }

  function saveDetector() {
    const d = detectorDraft;
    if (d) run(async () => (await backend()).setDetectorConfig(d));
  }

  function pulseLine(s: PulseStats | null): string {
    if (!s) return "no pulses yet";
    return `${s.pulses} pulses · ${ms(s.minPulseMs)} / ${ms(s.avgPulseMs)} / ${ms(s.maxPulseMs)}${s.overlaps ? ` · ${s.overlaps}× overlapping` : ""}${s.held ? " · was held long" : ""}`;
  }
</script>

<section class="panel">
  <h2>Rotary encoders</h2>
  <p class="muted small">
    An encoder sends one short button pulse per notch: one button when turned left, another when turned right.
    Turn a knob a few notches each way and Trimcraft suggests it here.
  </p>
  {#if error}<p class="error">{error}</p>{/if}

  {#each snapshot.suggestions as s (s.device + s.decrement + "-" + s.increment)}
    <div class="suggestion row">
      <span>Detected on <b>{title(s.device)}</b>: buttons {s.decrement + 1} and {s.increment + 1}</span>
      <button class="primary small" onclick={() => accept(s)}>Use as encoder</button>
      <button class="small" onclick={() => dismiss(s)}>Not an encoder</button>
    </div>
  {/each}
  {#each snapshot.halfDetected as h (h.device + h.decrement)}
    <p class="muted small">Button {h.decrement + 1} on {title(h.device)} pulses like an encoder. Turn the other way to complete the pair.</p>
  {/each}

  {#each snapshot.encoders as e (e.id)}
    <div class="encoder">
      <div class="row">
        <input class="name" value={e.name} onchange={(ev) => rename(e, ev.currentTarget.value)} />
        <span class="muted">{title(e.device)}</span>
        <span>− Button {e.decrement + 1} · + Button {e.increment + 1}</span>
        <button class="small" onclick={() => swap(e)} title="Swap which direction counts up">Swap direction</button>
        <button class="small" onclick={() => remove(e)}>Remove</button>
      </div>
      <div class="stats mono">
        <span>Steps counted: −{e.stepsDown} / +{e.stepsUp}</span>
        <span>− {pulseLine(e.decrementStats)}</span>
        <span>+ {pulseLine(e.incrementStats)}</span>
      </div>
    </div>
  {/each}

  <details>
    <summary>Mark two buttons as an encoder by hand</summary>
    <form class="row manual" onsubmit={(ev) => { ev.preventDefault(); addManual(); }}>
      <select bind:value={manualDevice}>
        {#each deviceOptions as d (d.id)}<option value={d.id}>{d.label ?? d.name} ({d.id})</option>{/each}
      </select>
      <label>− button <input type="number" min="1" bind:value={manualMinus} /></label>
      <label>+ button <input type="number" min="1" bind:value={manualPlus} /></label>
      <input placeholder="Name, e.g. Radio knob" bind:value={manualName} />
      <button class="primary" type="submit">Add</button>
    </form>
    {#if lastButton}<p class="muted small">Last pressed: button {lastButton.number} on {title(lastButton.device)}</p>{/if}
  </details>

  <details ontoggle={(e) => (detectorDraft = e.currentTarget.open ? { ...snapshot.detector } : null)}>
    <summary>Detection thresholds</summary>
    {#if detectorDraft}
      <form class="row manual" onsubmit={(ev) => { ev.preventDefault(); saveDetector(); }}>
        <label>Max pulse <input type="number" min="1" bind:value={detectorDraft.maxPulseMs} /> ms</label>
        <label>Max gap in a run <input type="number" min="1" bind:value={detectorDraft.maxRunGapMs} /> ms</label>
        <label>Min pulses in a run <input type="number" min="2" bind:value={detectorDraft.minRunLength} /></label>
        <button type="submit">Apply and measure again</button>
      </form>
    {/if}
  </details>
</section>

<style>
  .small { font-size: 12px; }
  .suggestion { background: #2a2214; border: 1px solid var(--accent-dim); border-radius: 4px; padding: 6px 8px; margin-bottom: 6px; }
  .encoder { border-top: 1px solid var(--line); padding: 8px 0; }
  .name { width: 12em; font-weight: 600; }
  .stats { display: flex; flex-direction: column; gap: 2px; margin-top: 4px; color: var(--muted); }
  details { margin-top: 10px; }
  summary { cursor: pointer; color: var(--muted); }
  .manual { margin-top: 6px; }
</style>

<script lang="ts">
  // Picks a vJoy device and a button, axis or POV on it. Devices the driver reports as
  // configured come first; their capabilities are shown so a missing button is obvious.
  import type { EngineSnapshot, VjoyAxis, VjoyAxisRef, VjoyButton, VjoyPov } from "../lib/types";
  import { VJOY_AXES } from "../lib/format";

  type Target = VjoyButton | VjoyAxisRef | VjoyPov;
  let {
    snapshot,
    kind,
    target = $bindable(),
  }: { snapshot: EngineSnapshot; kind: "button" | "axis" | "pov"; target: Target } = $props();

  const info = $derived(snapshot.connectors.find((c) => c.id === "vjoy")?.vjoy ?? null);
  const configured = $derived(info?.devices.filter((d) => d.owner !== "missing") ?? []);
  const device = $derived(info?.devices.find((d) => d.id === target.device));

  const problem = $derived.by(() => {
    if (!info?.version || !device) return "";
    if (device.owner === "missing") return `vJoy device ${target.device} is not configured (Configure vJoy)`;
    if ("button" in target && target.button > device.buttons) return `device ${target.device} has ${device.buttons} buttons; add more in Configure vJoy`;
    if ("axis" in target && !device.axes.includes(target.axis)) return `device ${target.device} has no axis ${target.axis}; enable it in Configure vJoy`;
    if ("pov" in target && target.pov > Math.max(device.discretePovs, device.continuousPovs)) return `device ${target.device} has no POV ${target.pov}; add POVs in Configure vJoy`;
    return "";
  });

  function setDevice(v: number) {
    target.device = Math.min(16, Math.max(1, Math.round(v) || 1));
  }
</script>

<span class="target">
  <select value={target.device} onchange={(e) => setDevice(Number(e.currentTarget.value))} title="vJoy device">
    {#each configured as d (d.id)}<option value={d.id}>vJoy {d.id} ({d.axes.length} axes, {d.buttons} buttons)</option>{/each}
    {#each Array.from({ length: 16 }, (_, i) => i + 1).filter((i) => !configured.some((d) => d.id === i)) as i (i)}
      <option value={i}>vJoy {i}{info?.version ? " (not configured)" : ""}</option>
    {/each}
  </select>
  {#if kind === "button" && "button" in target}
    <label>button <input type="number" min="1" max="128" bind:value={target.button} /></label>
  {:else if kind === "axis" && "axis" in target}
    <select bind:value={target.axis}>
      {#each VJOY_AXES as [value, text] (value)}
        <option {value}>{text}{device && !device.axes.includes(value as VjoyAxis) ? " (off)" : ""}</option>
      {/each}
    </select>
  {:else if kind === "pov" && "pov" in target}
    <label>POV <input type="number" min="1" max="4" bind:value={target.pov} /></label>
  {/if}
  {#if problem}<span class="warn" title={problem}>⚠ {problem}</span>{/if}
</span>

<style>
  .target { display: inline-flex; gap: 6px; align-items: center; flex-wrap: wrap; font-size: 12px; }
  label { display: inline-flex; gap: 4px; align-items: center; color: var(--muted); }
  input[type="number"] { width: 4.5em; }
  .warn { color: var(--warn); font-size: 11px; }
</style>

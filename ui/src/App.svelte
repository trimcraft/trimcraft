<script lang="ts">
  import { onMount } from "svelte";
  import type { AppMeta, EngineSnapshot } from "./lib/types";
  import { backend, isTauri } from "./lib/backend";
  import DeviceList from "./components/DeviceList.svelte";
  import DeviceView from "./components/DeviceView.svelte";
  import EncoderPanel from "./components/EncoderPanel.svelte";
  import FunctionPanel from "./components/FunctionPanel.svelte";
  import OverlayPanel from "./components/OverlayPanel.svelte";
  import VirtualControlsView from "./components/VirtualControlsView.svelte";

  type Section = "hardware" | "controls";
  const sections: [Section, string][] = [
    ["hardware", "Hardware Test"],
    ["controls", "Virtual Controls"],
  ];

  let snapshot = $state<EngineSnapshot | null>(null);
  let meta = $state<AppMeta | null>(null);
  let selected = $state<string | null>(null);
  let section = $state<Section>(loadSection());

  let device = $derived(snapshot?.devices.find((d) => d.id === selected) ?? snapshot?.devices[0]);

  function loadSection(): Section {
    try {
      return localStorage.getItem("detent.section") === "controls" ? "controls" : "hardware";
    } catch {
      return "hardware";
    }
  }

  function show(s: Section) {
    section = s;
    try {
      localStorage.setItem("detent.section", s);
    } catch {
      // Only a convenience; the app works without it.
    }
  }

  onMount(() => {
    let stop: (() => void) | undefined;
    backend().then(async (b) => {
      meta = await b.meta();
      stop = await b.subscribe((s) => (snapshot = s));
    });
    return () => stop?.();
  });
</script>

<header>
  <h1>Detent</h1>
  {#if meta}
    <span class="muted mono">v{meta.version} · input: {meta.backend}</span>
    {#if !isTauri}<span class="badge warn">browser demo, not connected to devices</span>{/if}
  {/if}
  <nav>
    {#each sections as [value, text] (value)}
      <button class:current={section === value} onclick={() => show(value)}>{text}</button>
    {/each}
  </nav>
</header>

{#if snapshot}
  {#if snapshot.backendError}<p class="error banner">Input backend failed: {snapshot.backendError}</p>{/if}
  {#if section === "hardware"}
    <main>
      <aside>
        <h2>Devices</h2>
        <DeviceList devices={snapshot.devices} selected={device?.id ?? null} onselect={(id) => (selected = id)} />
        {#if meta}
          <div class="info muted mono">
            <div>Settings: {meta.dataDir}</div>
            <div>Logs: {meta.logDir}</div>
          </div>
        {/if}
      </aside>
      <div class="content">
        {#if device}<DeviceView {device} {snapshot} />{/if}
        <EncoderPanel {snapshot} />
        <FunctionPanel {snapshot} />
        <OverlayPanel {snapshot} />
      </div>
    </main>
  {:else}
    <main class="single">
      <div class="content">
        <VirtualControlsView {snapshot} />
        <OverlayPanel {snapshot} />
      </div>
    </main>
  {/if}
{:else}
  <p class="muted pad">Starting…</p>
{/if}

<style>
  header { display: flex; gap: 12px; align-items: baseline; padding: 10px 16px; border-bottom: 1px solid var(--line); flex-wrap: wrap; }
  h1 { font-size: 18px; margin: 0; letter-spacing: 0.04em; }
  nav { display: flex; gap: 4px; margin-left: auto; }
  nav button { border-color: transparent; background: none; color: var(--muted); }
  nav button.current { color: var(--accent); border-color: var(--accent); }
  main { display: grid; grid-template-columns: minmax(240px, 320px) 1fr; gap: 16px; padding: 16px; }
  main.single { grid-template-columns: minmax(0, 1100px); }
  aside { display: flex; flex-direction: column; gap: 8px; }
  .content { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
  .info { font-size: 11px; word-break: break-all; margin-top: 12px; display: flex; flex-direction: column; gap: 4px; }
  .banner { padding: 8px 16px; margin: 0; }
  .pad { padding: 16px; }
  @media (max-width: 760px) { main { grid-template-columns: 1fr; } }
</style>

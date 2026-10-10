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
  import RecorderSettings from "./components/RecorderSettings.svelte";
  import RecordingBanner from "./components/RecordingBanner.svelte";
  import ConnectionsView from "./components/ConnectionsView.svelte";
  import { SECTIONS, nav, show } from "./lib/nav.svelte";

  let snapshot = $state<EngineSnapshot | null>(null);
  let meta = $state<AppMeta | null>(null);
  let selected = $state<string | null>(null);

  let device = $derived(snapshot?.devices.find((d) => d.id === selected) ?? snapshot?.devices[0]);

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
  <nav>
    {#each SECTIONS as [value, text] (value)}
      <button class:current={nav.section === value} onclick={() => show(value)}>{text}</button>
    {/each}
  </nav>
  <span class="spacer"></span>
  {#if snapshot}<RecordingBanner {snapshot} />{/if}
  {#if meta}
    {#if !isTauri}<span class="badge warn">browser demo, not connected to devices</span>{/if}
    <span class="version muted mono" title="Detent {meta.version} · input backend: {meta.backend}">v{meta.version}</span>
  {/if}
</header>

{#if snapshot}
  {#if snapshot.backendError}<p class="error banner">Input backend failed: {snapshot.backendError}</p>{/if}
  {#if nav.section === "hardware"}
    <main>
      <aside>
        <h2>Devices</h2>
        <DeviceList devices={snapshot.devices} selected={device?.id ?? null} onselect={(id) => (selected = id)} />
        {#if meta}
          <div class="info muted mono">
            <div>Input backend: {meta.backend}</div>
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
  {:else if nav.section === "connections"}
    <main class="single">
      <div class="content">
        <ConnectionsView {snapshot} />
      </div>
    </main>
  {:else}
    <main class="single">
      <div class="content">
        <VirtualControlsView {snapshot} />
        <RecorderSettings {snapshot} />
        <OverlayPanel {snapshot} />
      </div>
    </main>
  {/if}
{:else}
  <p class="muted pad">Starting…</p>
{/if}

<style>
  header { display: flex; gap: 12px; align-items: center; padding: 8px 16px; border-bottom: 1px solid var(--line); flex-wrap: wrap; }
  nav { display: flex; gap: 4px; }
  .spacer { flex: 1; }
  .version { cursor: default; }
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

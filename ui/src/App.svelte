<script lang="ts">
  import { onMount } from "svelte";
  import type { AppMeta, EngineSnapshot } from "./lib/types";
  import { backend, isTauri } from "./lib/backend";
  import DeviceList from "./components/DeviceList.svelte";
  import DeviceView from "./components/DeviceView.svelte";
  import EncoderPanel from "./components/EncoderPanel.svelte";
  import FunctionPanel from "./components/FunctionPanel.svelte";
  import OverlayPanel from "./components/OverlayPanel.svelte";

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
  <h1>Detent</h1>
  {#if meta}
    <span class="muted mono">v{meta.version} · input: {meta.backend}</span>
    {#if !isTauri}<span class="badge warn">browser demo, not connected to devices</span>{/if}
  {/if}
</header>

{#if snapshot}
  {#if snapshot.backendError}<p class="error banner">Input backend failed: {snapshot.backendError}</p>{/if}
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
  <p class="muted pad">Starting…</p>
{/if}

<style>
  header { display: flex; gap: 12px; align-items: baseline; padding: 10px 16px; border-bottom: 1px solid var(--line); }
  h1 { font-size: 18px; margin: 0; letter-spacing: 0.04em; }
  main { display: grid; grid-template-columns: minmax(240px, 320px) 1fr; gap: 16px; padding: 16px; }
  aside { display: flex; flex-direction: column; gap: 8px; }
  .content { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
  .info { font-size: 11px; word-break: break-all; margin-top: 12px; display: flex; flex-direction: column; gap: 4px; }
  .banner { padding: 8px 16px; margin: 0; }
  .pad { padding: 16px; }
  @media (max-width: 760px) { main { grid-template-columns: 1fr; } }
</style>

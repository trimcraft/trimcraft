<script lang="ts">
  import { onMount } from "svelte";
  import type { AppMeta, EngineSnapshot } from "./lib/types";
  import { backend, errorText, isTauri } from "./lib/backend";
  import DeviceList from "./components/DeviceList.svelte";
  import DeviceView from "./components/DeviceView.svelte";
  import EncoderPanel from "./components/EncoderPanel.svelte";
  import FunctionPanel from "./components/FunctionPanel.svelte";
  import OverlayPanel from "./components/OverlayPanel.svelte";
  import VirtualControlsView from "./components/VirtualControlsView.svelte";
  import RecorderSettings from "./components/RecorderSettings.svelte";
  import RecordingBanner from "./components/RecordingBanner.svelte";
  import ConnectionsView from "./components/ConnectionsView.svelte";
  import DiagnosticsView from "./components/DiagnosticsView.svelte";
  import SettingsView from "./components/settings/SettingsView.svelte";
  import ProfileSelector from "./components/ProfileSelector.svelte";
  import ProfilesPanel from "./components/ProfilesPanel.svelte";
  import VjoyMappingsPanel from "./components/VjoyMappingsPanel.svelte";
  import { SECTIONS, nav, show, showSettings } from "./lib/nav.svelte";

  let snapshot = $state<EngineSnapshot | null>(null);
  let meta = $state<AppMeta | null>(null);
  let selected = $state<string | null>(null);
  let hardwareTab = $state<"devices" | "log">("devices");
  /** Why the first snapshot did not arrive: a failed call, or no answer in time. */
  let startError = $state<string | null>(null);
  let slowStart = $state(false);

  /** Without a snapshot after this long, "Starting…" turns into a hint. */
  const SLOW_START_MS = 10_000;

  let elevated = $derived(snapshot?.shell.elevated ?? meta?.elevated ?? false);
  let device = $derived(snapshot?.devices.find((d) => d.id === selected) ?? snapshot?.devices[0]);

  onMount(() => {
    let stop: (() => void) | undefined;
    const slow = setTimeout(() => (slowStart = true), SLOW_START_MS);
    backend()
      .then(async (b) => {
        // The version badge is a nicety; the pages only need the snapshot.
        b.meta().then((m) => (meta = m), (e) => console.error("get_meta failed", e));
        stop = await b.subscribe((s) => (snapshot = s));
      })
      .catch((e) => {
        console.error("startup failed", e);
        startError = errorText(e);
      });
    return () => {
      clearTimeout(slow);
      stop?.();
    };
  });
</script>

{#snippet pageFailed(error: unknown, reset: () => void)}
  <main class="single">
    <div class="content">
      <section class="panel">
        <h2>This page could not be shown</h2>
        <p class="error mono">{errorText(error)}</p>
        <p class="muted">The other pages keep working. Please report this with the log file{meta ? ` (${meta.logDir})` : ""}.</p>
        <div class="row"><button onclick={reset}>Try again</button></div>
      </section>
    </div>
  </main>
{/snippet}

<header>
  <nav>
    {#each SECTIONS as [value, text] (value)}
      <button class:current={nav.section === value} onclick={() => show(value)}>{text}</button>
    {/each}
  </nav>
  <span class="spacer"></span>
  {#if snapshot}<RecordingBanner {snapshot} />{/if}
  {#if snapshot}<ProfileSelector {snapshot} />{/if}
  {#if meta}
    {#if !isTauri}<span class="badge warn">browser demo, not connected to devices</span>{/if}
    <span class="version muted mono" title="Trimcraft {meta.version} · input backend: {meta.backend} · running as administrator: {elevated ? 'yes' : 'no'}">v{meta.version}</span>
    {#if snapshot?.update.state === "available"}
      <button class="update-badge" title="Trimcraft {snapshot.update.version} is available" onclick={() => showSettings("updates")}>update</button>
    {/if}
  {/if}
  <button class="gear" class:current={nav.section === "settings"} title="Settings" aria-label="Settings" onclick={() => show("settings")}>⚙</button>
</header>

{#if snapshot}
  {#if snapshot.backendError}<p class="error banner">Input backend failed: {snapshot.backendError}</p>{/if}
  {#if snapshot.shell.elevatedForeground}
    <p class="warn banner">
      {snapshot.shell.elevatedForeground} runs as administrator; Trimcraft's keys and mouse will not reach it.
      <button class="link" onclick={() => show("settings")}>Settings</button>
    </p>
  {/if}
  <!-- One page failing to render must not take the navigation or the other pages with it. -->
  {#key nav.section}
  <svelte:boundary failed={pageFailed} onerror={(e) => console.error(`page ${nav.section} failed`, e)}>
  {#if nav.section === "hardware"}
    <div class="tabs" role="tablist">
      <button role="tab" aria-selected={hardwareTab === "devices"} class:current={hardwareTab === "devices"} onclick={() => (hardwareTab = "devices")}>Devices</button>
      <button role="tab" aria-selected={hardwareTab === "log"} class:current={hardwareTab === "log"} onclick={() => (hardwareTab = "log")}>Event log</button>
    </div>
  {#if hardwareTab === "log"}
    <main class="single wide">
      <div class="content">
        <DiagnosticsView />
      </div>
    </main>
  {:else}
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
  {/if}
  {:else if nav.section === "settings"}
    <main class="single">
      <div class="content">
        <SettingsView {snapshot} />
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
        <ProfilesPanel {snapshot} />
        <VirtualControlsView {snapshot} />
        <VjoyMappingsPanel {snapshot} />
        <RecorderSettings {snapshot} />
        <OverlayPanel {snapshot} />
      </div>
    </main>
  {/if}
  </svelte:boundary>
  {/key}
{:else if startError}
  <div class="pad">
    <p class="error">Trimcraft could not start: {startError}</p>
    <p class="muted">Restart Trimcraft from the tray menu (Quit, then start it again). If it keeps happening, please report it with the log file{meta ? ` (${meta.logDir})` : ""}.</p>
  </div>
{:else if slowStart}
  <div class="pad">
    <p class="muted">Starting… this takes longer than usual.</p>
    <p class="muted">Trimcraft's engine has not answered yet. If nothing changes, quit Trimcraft from the tray menu and start it again; the log file{meta ? ` (${meta.logDir})` : ""} shows what it is waiting for.</p>
  </div>
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
  .tabs { display: flex; gap: 4px; padding: 8px 16px 0; }
  .tabs button { border-radius: 4px 4px 0 0; border-bottom-color: transparent; background: none; color: var(--muted); }
  .tabs button.current { color: var(--accent); border-color: var(--accent); }
  main { display: grid; grid-template-columns: minmax(240px, 320px) 1fr; gap: 16px; padding: 16px; }
  main.single { grid-template-columns: minmax(0, 1100px); }
  main.single.wide { grid-template-columns: minmax(0, 1600px); }
  aside { display: flex; flex-direction: column; gap: 8px; }
  .content { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
  .info { font-size: 11px; word-break: break-all; margin-top: 12px; display: flex; flex-direction: column; gap: 4px; }
  .banner { padding: 8px 16px; margin: 0; }
  .pad { padding: 16px; }
  .update-badge { font-size: 11px; padding: 0 6px; border-radius: 10px; border-color: var(--accent); color: var(--accent); background: none; }
  .gear { border-color: transparent; background: none; color: var(--muted); font-size: 16px; line-height: 1; padding: 4px 6px; }
  .gear.current { color: var(--accent); border-color: var(--accent); }
  .warn.banner { color: var(--warn); background: var(--panel); border-bottom: 1px solid var(--line); font-size: 12px; }
  .link { border: none; background: none; padding: 0 4px; color: var(--accent); text-decoration: underline; font-size: 12px; }
  @media (max-width: 760px) { main { grid-template-columns: 1fr; } }
</style>

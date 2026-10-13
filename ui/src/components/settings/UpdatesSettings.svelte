<script lang="ts">
  // Settings > Updates: the current version, the startup check, "Check now", and installing a
  // new version (Trimcraft quits and restarts; asks first when macros or vJoy are active).
  import type { AppMeta, EngineSnapshot } from "../../lib/types";
  import { backend, errorText } from "../../lib/backend";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let meta = $state<AppMeta | null>(null);
  let error = $state("");
  let confirming = $state<string | null>(null);

  backend()
    .then((b) => b.meta())
    .then((m) => (meta = m))
    .catch(() => {});

  let update = $derived(snapshot.update);
  let busy = $derived(update.state === "checking" || update.state === "downloading" || update.state === "installing");

  async function setStartupCheck(on: boolean) {
    error = "";
    try {
      await (await backend()).setSettings({ ...snapshot.settings, checkUpdatesOnStartup: on });
    } catch (e) {
      error = errorText(e);
    }
  }

  async function checkNow() {
    error = "";
    confirming = null;
    try {
      await (await backend()).checkForUpdates();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function install(confirmed = false) {
    error = "";
    try {
      const b = await backend();
      if (!confirmed) {
        const running = await b.updateBusy();
        const parts = [
          running.macros > 0 && `${running.macros} macro${running.macros === 1 ? " is" : "s are"} running`,
          running.vjoy && "vJoy devices are driven by Trimcraft",
        ].filter(Boolean);
        if (parts.length) {
          confirming = `${parts.join(" and ")}. Updating quits Trimcraft: macros stop and vJoy is released until it has restarted.`;
          return;
        }
      }
      confirming = null;
      await b.installUpdate();
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

<section class="panel">
  <h2>Updates</h2>

  <div class="group">
    <p class="state">Installed version: <b class="mono">{meta ? `v${meta.version}` : "…"}</b></p>
    <label class="check">
      <input type="checkbox" checked={snapshot.settings.checkUpdatesOnStartup} onchange={(e) => setStartupCheck(e.currentTarget.checked)} />
      Check for updates on startup
    </label>
    <p class="hint muted">Looks for a new release on GitHub shortly after Trimcraft starts. Nothing is installed without you.</p>
    <div class="row">
      <button disabled={busy} onclick={checkNow}>{update.state === "checking" ? "Checking…" : "Check now"}</button>
      {#if update.state === "upToDate"}
        <span class="ok small">{update.noReleases ? "You're up to date (no release published yet)." : "You're up to date."}</span>
      {:else if update.state === "failed"}
        <span class="error small">{update.message}</span>
      {/if}
    </div>
  </div>

  {#if update.state === "available" || update.state === "downloading" || update.state === "installing"}
    <div class="group">
      <h3>Trimcraft {update.version} is available</h3>
      {#if update.state === "available"}
        {#if update.date}<p class="hint muted">Released {update.date.slice(0, 10)}</p>{/if}
        {#if update.notes}<pre class="notes">{update.notes}</pre>{/if}
        {#if confirming}
          <p class="warn small">{confirming}</p>
          <div class="row">
            <button class="primary" onclick={() => install(true)}>Update anyway</button>
            <button onclick={() => (confirming = null)}>Not now</button>
          </div>
        {:else}
          <div class="row">
            <button class="primary" onclick={() => install()}>Download &amp; install</button>
            <span class="muted small">Trimcraft quits, installs the update and starts again.</span>
          </div>
        {/if}
      {:else if update.state === "downloading"}
        <p class="small">Downloading{update.percent !== null ? ` ${update.percent}%` : "…"}</p>
        {#if update.percent !== null}<progress max="100" value={update.percent}></progress>{/if}
      {:else}
        <p class="small">Installing; Trimcraft restarts in a moment.</p>
      {/if}
    </div>
  {/if}

  {#if error}<p class="error small">{error}</p>{/if}
</section>

<style>
  .group { display: flex; flex-direction: column; gap: 6px; padding: 8px 0; border-top: 1px solid var(--line); }
  .group:first-of-type { border-top: none; padding-top: 0; }
  .check { display: inline-flex; gap: 8px; align-items: center; cursor: pointer; }
  .hint { margin: 0 0 4px 24px; font-size: 12px; }
  .state { margin: 0; }
  .small { font-size: 12px; margin: 0; }
  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
  .notes { white-space: pre-wrap; font-family: inherit; font-size: 12px; background: var(--panel-2); border: 1px solid var(--line); border-radius: 4px; padding: 6px 8px; margin: 0; max-height: 16em; overflow-y: auto; }
  progress { width: 20em; }
  button:disabled { opacity: 0.5; cursor: default; }
</style>

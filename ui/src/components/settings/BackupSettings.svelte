<script lang="ts">
  // Settings > Backup & transfer: export, import, and the automatic backups of setup.json.
  import { onMount } from "svelte";
  import type { BackupInfo, EngineSnapshot } from "../../lib/types";
  import { backend, errorText } from "../../lib/backend";
  import ImportFlow from "../ImportFlow.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let selected = $state<string[]>([]);
  let backups = $state<BackupInfo[] | null>(null);
  let restoring = $state<string | null>(null);
  let error = $state("");
  let notice = $state("");
  let busy = $state(false);

  async function refresh() {
    try {
      backups = await (await backend()).listBackups();
    } catch (e) {
      error = errorText(e);
    }
  }

  onMount(refresh);

  async function run(action: () => Promise<string | void>) {
    error = "";
    notice = "";
    busy = true;
    try {
      const message = await action();
      if (message) notice = message;
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
      await refresh();
    }
  }

  const exportProfiles = (ids: string[] | null) =>
    run(async () => {
      const path = await (await backend()).exportSetup(ids);
      return path ? `Saved to ${path}. Credentials are not included.` : undefined;
    });

  const backupNow = () => run(async () => `Backup written: ${(await (await backend()).backupNow()).name}`);

  async function restore(name: string) {
    restoring = null;
    await run(async () => await (await backend()).restoreBackup(name));
  }

  function toggle(id: string) {
    selected = selected.includes(id) ? selected.filter((s) => s !== id) : [...selected, id];
  }

  const when = (secs: number) => new Date(secs * 1000).toLocaleString();
  const kb = (bytes: number) => `${Math.max(1, Math.round(bytes / 1024))} kB`;
</script>

<section class="panel">
  <h2>Backup &amp; transfer</h2>
  {#if error}<p class="error">{error}</p>{/if}
  {#if notice}<p class="ok">{notice}</p>{/if}

  <div class="group">
    <h3>Export</h3>
    <p class="hint muted">
      Saves the configuration to a .trimcraft.json file you can keep or give to someone else. Credentials (Discord client secret
      and sign-in) are never exported; after importing, enter them again under Connections.
    </p>
    <div class="row">
      <button onclick={() => exportProfiles(null)} disabled={busy}>Export everything…</button>
    </div>
    <div class="profiles">
      {#each snapshot.profiles as p (p.id)}
        <label><input type="checkbox" checked={selected.includes(p.id)} onchange={() => toggle(p.id)} /> {p.name}</label>
      {/each}
    </div>
    <div class="row">
      <button onclick={() => exportProfiles([...selected])} disabled={busy || selected.length === 0}>
        Export {selected.length || ""} selected profile{selected.length === 1 ? "" : "s"}…
      </button>
    </div>
  </div>

  <div class="group">
    <h3>Import</h3>
    <ImportFlow onchange={refresh} />
  </div>

  <div class="group">
    <h3>Automatic backups</h3>
    <p class="hint muted">
      Trimcraft copies the configuration to the backup folder whenever it is saved, at most every 5 minutes, and before every
      import and restore. It keeps the latest 20 and the newest of each of the last 14 days.
    </p>
    <div class="row">
      <button onclick={backupNow} disabled={busy}>Back up now</button>
      <button onclick={() => run(async () => void (await (await backend()).openBackupFolder()))}>Open backup folder</button>
    </div>
    {#if backups === null}
      <p class="muted small">Reading the backups…</p>
    {:else if backups.length === 0}
      <p class="muted small">No backups yet. One is written with the next change.</p>
    {:else}
      <table>
        <thead><tr><th>Date</th><th>Profiles</th><th>Size</th><th></th></tr></thead>
        <tbody>
          {#each backups as b (b.name)}
            <tr>
              <td title={b.name}>{when(b.unixSecs)}</td>
              <td>{b.profiles ?? "unreadable"}</td>
              <td>{kb(b.size)}</td>
              <td class="actions">
                {#if restoring === b.name}
                  <span class="small">Restore this one? The current configuration is backed up first.</span>
                  <button class="small danger" onclick={() => restore(b.name)} disabled={busy}>Restore</button>
                  <button class="small" onclick={() => (restoring = null)}>Keep current</button>
                {:else}
                  <button class="small" disabled={b.profiles === null || busy} onclick={() => (restoring = b.name)}>Restore…</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</section>

<style>
  .group { display: flex; flex-direction: column; gap: 8px; padding: 12px 0; border-top: 1px solid var(--line); }
  .group:first-of-type { border-top: none; }
  h3 { margin: 0; }
  .hint { margin: 0; font-size: 12px; }
  .small { font-size: 12px; }
  .ok { color: var(--ok); margin: 0; overflow-wrap: anywhere; }
  .profiles { display: flex; flex-wrap: wrap; gap: 4px 16px; }
  .profiles label { display: flex; gap: 6px; align-items: center; }
  table { border-collapse: collapse; width: 100%; }
  th { text-align: left; font-size: 11px; text-transform: uppercase; color: var(--muted); font-weight: 600; }
  td, th { padding: 4px 8px; border-bottom: 1px solid var(--line); }
  .actions { display: flex; gap: 8px; align-items: center; justify-content: flex-end; }
  .danger { border-color: var(--bad); color: var(--bad); }
</style>

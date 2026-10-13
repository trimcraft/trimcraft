<script lang="ts">
  // Import of an export file: choose the file, see what it holds, then replace everything
  // (after a confirmation) or add profiles. The file is read and checked by the app; a backup
  // of the current configuration is made before anything changes.
  import type { ImportResult, ImportSummary } from "../lib/types";
  import { backend, errorText } from "../lib/backend";

  let {
    onlyProfiles = false,
    onchange,
  }: {
    /** Offer "add profiles" only (the profile list). */
    onlyProfiles?: boolean;
    /** Something was imported (backups may have changed). */
    onchange?: () => void;
  } = $props();

  let summary = $state<ImportSummary | null>(null);
  let chosen = $state<string[]>([]);
  let mode = $state<"add" | "replace">("add");
  let confirming = $state(false);
  let busy = $state(false);
  let error = $state("");
  let result = $state<ImportResult | null>(null);

  async function pick() {
    error = "";
    result = null;
    busy = true;
    try {
      const s = await (await backend()).pickImportFile();
      if (!s) return;
      summary = s;
      chosen = s.profiles.map((p) => p.id);
      mode = onlyProfiles || s.kind === "profiles" ? "add" : "replace";
      confirming = false;
    } catch (e) {
      error = errorText(e);
      summary = null;
    } finally {
      busy = false;
    }
  }

  async function cancel() {
    summary = null;
    confirming = false;
    await (await backend()).cancelImport();
  }

  async function apply() {
    error = "";
    busy = true;
    try {
      result = await (await backend()).applyImport(mode === "replace" ? { type: "replaceAll" } : { type: "addProfiles", profiles: chosen });
      summary = null;
      confirming = false;
      onchange?.();
    } catch (e) {
      error = errorText(e);
      confirming = false;
    } finally {
      busy = false;
    }
  }

  function toggle(id: string) {
    chosen = chosen.includes(id) ? chosen.filter((c) => c !== id) : [...chosen, id];
  }

  const when = (iso: string | null) => (iso ? new Date(iso).toLocaleString() : "unknown");
</script>

<div class="flow">
  <div class="row">
    <button onclick={pick} disabled={busy}>{onlyProfiles ? "Import profiles…" : "Import from file…"}</button>
    {#if !summary && !result && !error}
      <span class="muted small">Trimcraft export files (.trimcraft.json) and setup backups.</span>
    {/if}
  </div>
  {#if error}<p class="error">{error}</p>{/if}
  {#if result}
    <p class="ok">{result.message}</p>
    {#if result.report?.addedEncoders || result.report?.addedFunctions}
      <p class="muted small">
        Also added {result.report.addedEncoders} encoder(s) and {result.report.addedFunctions} counter(s) the profiles use.
      </p>
    {/if}
  {/if}

  {#if summary}
    <div class="box">
      <p class="small">
        {summary.kind === "profiles" ? "Profiles" : summary.kind === "all" ? "Whole configuration" : "Setup backup"}, exported
        {when(summary.exportedAt)}{summary.trimcraftVersion ? ` by Trimcraft ${summary.trimcraftVersion}` : ""}.
        {#if summary.schemaVersion < 2}<span class="muted">Older file; it is converted on import.</span>{/if}
      </p>
      {#if summary.strippedFields.length}
        <p class="small warn">
          Credentials are not part of the file. Connections that need them (e.g. Discord) have to be set up again under
          Connections.
        </p>
      {/if}

      {#if !onlyProfiles && summary.kind !== "profiles"}
        <div class="row">
          <label><input type="radio" bind:group={mode} value="replace" /> Replace the whole configuration</label>
          <label><input type="radio" bind:group={mode} value="add" /> Add profiles from the file</label>
        </div>
      {/if}

      {#if mode === "add"}
        <div class="profiles">
          {#each summary.profiles as p (p.id)}
            <label class="prow">
              <input type="checkbox" checked={chosen.includes(p.id)} onchange={() => toggle(p.id)} />
              <span class="pname">{p.name}</span>
              <span class="muted small">{p.controls} control{p.controls === 1 ? "" : "s"}{p.executables.length ? ` · ${p.executables.join(", ")}` : ""}</span>
            </label>
          {/each}
        </div>
        <p class="muted small">
          Names already in use get a number, like "DCS (2)". The imported profiles do not become active, and programs another
          profile already lists are not taken over.
        </p>
      {:else}
        <p class="small">
          Replaces profiles, controls, encoders, counters, the overlay layout and connection settings with the file's
          ({summary.profiles.length} profile{summary.profiles.length === 1 ? "" : "s"}). This computer's tray and startup
          settings stay.
        </p>
      {/if}

      <div class="row">
        {#if mode === "replace" && confirming}
          <span class="small">Replace everything? A backup of the current configuration is made first.</span>
          <button class="small danger" onclick={apply} disabled={busy}>Replace all</button>
          <button class="small" onclick={() => (confirming = false)}>Keep current</button>
        {:else if mode === "replace"}
          <button class="danger" onclick={() => (confirming = true)}>Replace all…</button>
          <button onclick={cancel}>Cancel</button>
        {:else}
          <button class="primary" onclick={apply} disabled={busy || chosen.length === 0}>
            Add {chosen.length} profile{chosen.length === 1 ? "" : "s"}
          </button>
          <button onclick={cancel}>Cancel</button>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .flow { display: flex; flex-direction: column; gap: 8px; }
  .small { font-size: 12px; margin: 0; }
  .ok { color: var(--ok); margin: 0; }
  .warn { color: var(--warn); }
  .box { border: 1px solid var(--line); border-radius: 6px; padding: 10px; display: flex; flex-direction: column; gap: 8px; background: var(--bg); }
  .profiles { display: flex; flex-direction: column; gap: 2px; }
  .prow { display: flex; gap: 8px; align-items: baseline; }
  .pname { font-weight: 600; }
  .danger { border-color: var(--bad); color: var(--bad); }
</style>

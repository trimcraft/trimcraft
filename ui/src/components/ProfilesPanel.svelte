<script lang="ts">
  // Profiles: one per game, each with its own virtual controls and overlay widgets. Lists the
  // programs that activate a profile, the auto-switch settings, and create / duplicate /
  // rename / delete / make default.
  import type { EngineSnapshot, NoMatch, ProcessWindow, ProfileView } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import ImportFlow from "./ImportFlow.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let error = $state("");
  /** Where the last export went. */
  let exported = $state("");
  let newName = $state("");
  /** Profile being renamed, with the draft name. */
  let renaming = $state<{ id: string; name: string } | null>(null);
  /** Profile whose Delete was pressed once. */
  let confirmDelete = $state<string | null>(null);
  /** Draft of the "add program" field, per profile. */
  let exeDraft = $state<Record<string, string>>({});
  /** Profile whose process picker is open, and the list. */
  let picking = $state<{ id: string; windows: ProcessWindow[] | null } | null>(null);

  const defaultName = $derived(snapshot.profiles[0]?.name ?? "Default");

  async function run(action: () => Promise<unknown>) {
    error = "";
    try {
      await action();
      return true;
    } catch (e) {
      error = errorText(e);
      return false;
    }
  }

  const b = backend;

  const exportProfile = (p: ProfileView) => () => {
    exported = "";
    return run(async () => {
      const path = await (await b()).exportSetup([p.id]);
      if (path) exported = `Saved ${p.name} to ${path}`;
    });
  };

  async function create() {
    if (await run(async () => (await b()).createProfile(newName))) newName = "";
  }

  async function saveRename() {
    const r = renaming;
    if (r && (await run(async () => (await b()).renameProfile(r.id, r.name)))) renaming = null;
  }

  function setExecutables(p: ProfileView, executables: string[]) {
    return run(async () => (await b()).setProfileExecutables(p.id, executables));
  }

  async function addExe(p: ProfileView, exe: string) {
    const name = exe.trim();
    if (!name) return;
    if (await setExecutables(p, [...p.executables, name])) exeDraft[p.id] = "";
  }

  async function openPicker(p: ProfileView) {
    if (picking?.id === p.id) {
      picking = null;
      return;
    }
    picking = { id: p.id, windows: null };
    await run(async () => {
      const windows = await (await b()).listProcessWindows();
      if (picking?.id === p.id) picking = { id: p.id, windows };
    });
  }

  function setSwitching(change: Partial<{ autoSwitch: boolean; noMatch: NoMatch }>) {
    run(async () => (await b()).setProfileSwitching({ ...snapshot.profileSwitching, ...change }));
  }

  const listedBy = (exe: string) => snapshot.profiles.find((p) => p.executables.some((e) => e.toLowerCase() === exe.toLowerCase()));
</script>

<section class="panel" id="profiles">
  <h2>Profiles</h2>
  <p class="muted small">
    Each profile has its own radios and overlay widgets; devices, encoders, counters and connections are shared. A profile
    becomes active when one of its programs comes to the front. Switching never runs actions: every radio keeps its state.
  </p>
  {#if error}<p class="error">{error}</p>{/if}
  {#if exported}<p class="ok">{exported}</p>{/if}
  <ImportFlow onlyProfiles />

  <div class="row settings">
    <label><input type="checkbox" checked={snapshot.profileSwitching.autoSwitch} onchange={(e) => setSwitching({ autoSwitch: e.currentTarget.checked })} /> Auto-switch by the program in the foreground</label>
    <label class:off={!snapshot.profileSwitching.autoSwitch}>When no game matches
      <select value={snapshot.profileSwitching.noMatch} disabled={!snapshot.profileSwitching.autoSwitch} onchange={(e) => setSwitching({ noMatch: e.currentTarget.value as NoMatch })}>
        <option value="keepCurrent">Keep current profile</option>
        <option value="switchToDefault">Switch to {defaultName} (default profile)</option>
      </select>
    </label>
    <span class="muted small">In the foreground: <span class="mono">{snapshot.foreground ?? "–"}</span></span>
  </div>

  {#each snapshot.profiles as p, i (p.id)}
    {@const active = p.id === snapshot.activeProfile}
    <div class="profile" class:active>
      <div class="row">
        {#if renaming?.id === p.id}
          <form class="row" onsubmit={(e) => { e.preventDefault(); saveRename(); }}>
            <input bind:value={renaming.name} aria-label="Profile name" />
            <button class="primary small" type="submit">Save</button>
            <button class="small" type="button" onclick={() => (renaming = null)}>Cancel</button>
          </form>
        {:else}
          <span class="pname">{p.name}</span>
        {/if}
        {#if active}<span class="badge on">active</span>{/if}
        {#if i === 0}<span class="badge" title="Used when no game matches and the setting says so">default</span>{/if}
        <span class="spacer"></span>
        {#if !active}<button class="small" onclick={() => run(async () => (await b()).selectProfile(p.id))}>Activate</button>{/if}
        {#if renaming?.id !== p.id}<button class="small" onclick={() => (renaming = { id: p.id, name: p.name })}>Rename</button>{/if}
        <button class="small" title="Save this profile to a .detent.json file (without credentials)" onclick={exportProfile(p)}>Export</button>
        <button class="small" title="Copy with all radios, widgets and states (without its programs)" onclick={() => run(async () => (await b()).duplicateProfile(p.id, ""))}>Duplicate</button>
        {#if i > 0}<button class="small" onclick={() => run(async () => (await b()).makeDefaultProfile(p.id))}>Make default</button>{/if}
        {#if confirmDelete === p.id}
          <button class="small danger" onclick={() => { confirmDelete = null; run(async () => (await b()).deleteProfile(p.id)); }}>Delete "{p.name}" with its radios</button>
          <button class="small" onclick={() => (confirmDelete = null)}>Keep</button>
        {:else}
          <button class="small" disabled={snapshot.profiles.length === 1} title={snapshot.profiles.length === 1 ? "The last profile cannot be deleted" : ""} onclick={() => (confirmDelete = p.id)}>Delete</button>
        {/if}
      </div>
      <div class="row exes">
        <span class="muted small">Programs:</span>
        {#each p.executables as exe (exe)}
          <span class="chip mono">{exe}<button class="x" title="Remove" onclick={() => setExecutables(p, p.executables.filter((e) => e !== exe))}>×</button></span>
        {:else}
          <span class="muted small">{i === 0 ? "none (chosen by hand or as the fallback)" : "none (only chosen by hand)"}</span>
        {/each}
        <form class="row" onsubmit={(e) => { e.preventDefault(); addExe(p, exeDraft[p.id] ?? ""); }}>
          <input class="mono exe" placeholder="DCS.exe" value={exeDraft[p.id] ?? ""} oninput={(e) => (exeDraft[p.id] = e.currentTarget.value)} aria-label="Program to add" />
          <button class="small" type="submit">Add</button>
        </form>
        <button class="small" class:active={picking?.id === p.id} onclick={() => openPicker(p)}>Pick from running processes</button>
      </div>
      {#if picking?.id === p.id}
        <div class="picker">
          {#if picking.windows === null}
            <span class="muted small">Reading the window list…</span>
          {:else}
            {#each picking.windows as w (w.exe + "\n" + w.title)}
              {@const owner = listedBy(w.exe)}
              <button class="pick" disabled={owner?.id === p.id} onclick={() => addExe(p, w.exe)}>
                <span class="mono">{w.exe}</span>
                <span class="muted title">{w.title}</span>
                {#if owner && owner.id !== p.id}<span class="badge warn" title="Several profiles list it: the active one stays, otherwise the first in the list wins">also in {owner.name}</span>{/if}
                {#if owner?.id === p.id}<span class="badge">added</span>{/if}
              </button>
            {:else}
              <span class="muted small">No programs with a window found.</span>
            {/each}
          {/if}
        </div>
      {/if}
    </div>
  {/each}

  <form class="row new" onsubmit={(e) => { e.preventDefault(); create(); }}>
    <input bind:value={newName} placeholder="Profile name, e.g. DCS World" aria-label="New profile name" />
    <button type="submit">New profile</button>
    <span class="muted small">A new or duplicated profile becomes active, so its radios can be set up right away.</span>
  </form>
</section>

<style>
  .small { font-size: 12px; }
  .ok { color: var(--ok); margin: 0 0 6px; font-size: 12px; overflow-wrap: anywhere; }
  .settings { margin-bottom: 8px; }
  .settings label { display: flex; gap: 6px; align-items: center; font-size: 12px; }
  .settings label.off { opacity: 0.55; }
  .profile { border-top: 1px solid var(--line); padding: 8px 0; display: flex; flex-direction: column; gap: 6px; }
  .pname { font-size: 16px; }
  .profile.active .pname { color: var(--accent); font-weight: 600; }
  .badge.on { border-color: var(--accent); color: var(--accent); }
  .spacer { flex: 1; }
  .exes { gap: 6px; }
  .chip { display: inline-flex; align-items: center; gap: 4px; border: 1px solid var(--line); border-radius: 10px; padding: 0 2px 0 8px; }
  .x { border: none; background: none; padding: 0 4px; color: var(--muted); }
  .x:hover { color: var(--bad); }
  input.exe { width: 12em; }
  .danger { border-color: var(--bad); color: var(--bad); }
  .picker { display: flex; flex-direction: column; gap: 2px; max-height: 240px; overflow-y: auto; border: 1px solid var(--line); border-radius: 4px; padding: 4px; }
  .pick { display: flex; gap: 10px; align-items: center; text-align: left; border-color: transparent; background: none; }
  .pick:hover:not(:disabled) { border-color: var(--accent); }
  .pick:disabled { opacity: 0.6; cursor: default; }
  .title { font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; flex: 1; }
  .new { margin-top: 8px; }
</style>

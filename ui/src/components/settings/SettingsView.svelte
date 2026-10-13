<script lang="ts">
  // The Settings page, opened with the gear in the header. One component per section; add new
  // sections (Backup, Overlay appearance, Updates, ...) to SECTIONS.
  import type { Component } from "svelte";
  import type { EngineSnapshot } from "../../lib/types";
  import GeneralSettings from "./GeneralSettings.svelte";
  import BackupSettings from "./BackupSettings.svelte";
  import OverlayAppearanceSettings from "./OverlayAppearanceSettings.svelte";
  import AboutSettings from "./AboutSettings.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  type Section = { id: string; title: string; component: Component<{ snapshot: EngineSnapshot }> };
  const GENERAL: Section = { id: "general", title: "General", component: GeneralSettings };
  const BACKUP: Section = { id: "backup", title: "Backup & transfer", component: BackupSettings };
  const APPEARANCE: Section = { id: "appearance", title: "Overlay appearance", component: OverlayAppearanceSettings };
  const SECTIONS: Section[] = [GENERAL, APPEARANCE, BACKUP, { id: "about", title: "About", component: AboutSettings }];

  let current = $state(GENERAL.id);
  let section = $derived(SECTIONS.find((s) => s.id === current) ?? GENERAL);
</script>

<div class="settings">
  <nav aria-label="Settings sections">
    <h2>Settings</h2>
    {#each SECTIONS as s (s.id)}
      <button class:current={s.id === section.id} onclick={() => (current = s.id)}>{s.title}</button>
    {/each}
  </nav>
  <div class="body">
    <section.component {snapshot} />
  </div>
</div>

<style>
  .settings { display: grid; grid-template-columns: 180px minmax(0, 1fr); gap: 16px; }
  nav { display: flex; flex-direction: column; gap: 4px; }
  nav button { text-align: left; border-color: transparent; background: none; color: var(--muted); }
  nav button.current { color: var(--accent); border-color: var(--accent); }
  .body { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
  @media (max-width: 760px) { .settings { grid-template-columns: 1fr; } nav { flex-direction: row; flex-wrap: wrap; } }
</style>

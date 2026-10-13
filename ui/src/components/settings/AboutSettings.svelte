<script lang="ts">
  // Settings > About: version, licence notice, the third-party licences and the Credits page.
  import { onMount } from "svelte";
  import type { AboutLink, AppMeta, EngineSnapshot } from "../../lib/types";
  import { backend, errorText } from "../../lib/backend";
  import CreditsView from "./CreditsView.svelte";
  import ThirdPartyLicenses from "./ThirdPartyLicenses.svelte";

  // Same props as every section; not needed here.
  let {}: { snapshot: EngineSnapshot } = $props();

  let meta = $state<AppMeta | null>(null);
  let showCredits = $state(false);
  let error = $state("");

  onMount(async () => {
    try {
      meta = await (await backend()).meta();
    } catch (e) {
      error = errorText(e);
    }
  });

  async function open(link: AboutLink) {
    error = "";
    try {
      await (await backend()).openAboutLink(link);
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

{#if showCredits}
  <CreditsView onback={() => (showCredits = false)} />
{:else}
  <section class="panel">
    <h2>About</h2>
    {#if error}<p class="error">{error}</p>{/if}

    <div class="group">
      <h3>Trimcraft {meta?.version ?? ""}</h3>
      <p class="hint muted">
        HOTAS configuration tool for Windows. Early access: expect bugs and breaking changes, and keep backups of your
        configuration.
      </p>
      <div class="row">
        <button onclick={() => open("repository")}>Project page</button>
        <button onclick={() => open("issues")}>Report a bug or idea</button>
        <button onclick={() => (showCredits = true)}>Credits</button>
      </div>
    </div>

    <div class="group">
      <h3>Licence</h3>
      <p class="hint">
        Copyright © Samuel Breu and contributors. Trimcraft is free software: you can redistribute it and/or modify it under
        the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the
        License, or (at your option) any later version.
      </p>
      <p class="hint">
        Trimcraft is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied
        warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
      </p>
      <div class="row">
        <button onclick={() => open("license")}>GNU GPL v3</button>
      </div>
    </div>

    <div class="group">
      <h3>Third-party licences</h3>
      <p class="hint muted">
        Trimcraft is built on open source software. Their licences are listed here and installed next to the program
        (THIRD_PARTY_LICENSES).
      </p>
      <ThirdPartyLicenses />
    </div>
  </section>
{/if}

<style>
  .group { display: flex; flex-direction: column; gap: 8px; padding: 12px 0; border-top: 1px solid var(--line); }
  .group:first-of-type { border-top: none; }
  h3 { margin: 0; }
  .hint { margin: 0; font-size: 12px; }
  .row { display: flex; gap: 8px; flex-wrap: wrap; }
</style>

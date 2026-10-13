<script lang="ts">
  // Settings > About: version, licence notice and the third-party licences.
  import { onMount } from "svelte";
  import type { AboutLink, AppMeta, EngineSnapshot } from "../../lib/types";
  import { backend, errorText } from "../../lib/backend";

  // Same props as every section; not needed here.
  let {}: { snapshot: EngineSnapshot } = $props();

  let meta = $state<AppMeta | null>(null);
  let licenses = $state<string | null>(null);
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

  async function toggleLicenses() {
    error = "";
    if (licenses !== null) {
      licenses = null;
      return;
    }
    try {
      licenses = await (await backend()).thirdPartyLicenses();
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

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
      Trimcraft is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty
      of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
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
    <div class="row">
      <button onclick={toggleLicenses}>{licenses === null ? "Show third-party licences" : "Hide third-party licences"}</button>
    </div>
    {#if licenses !== null}
      <pre class="licenses">{licenses}</pre>
    {/if}
  </div>
</section>

<style>
  .group { display: flex; flex-direction: column; gap: 8px; padding: 12px 0; border-top: 1px solid var(--line); }
  .group:first-of-type { border-top: none; }
  h3 { margin: 0; }
  .hint { margin: 0; font-size: 12px; }
  .row { display: flex; gap: 8px; flex-wrap: wrap; }
  .licenses {
    margin: 0;
    max-height: 420px;
    overflow: auto;
    padding: 8px;
    border: 1px solid var(--line);
    font-size: 11px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>

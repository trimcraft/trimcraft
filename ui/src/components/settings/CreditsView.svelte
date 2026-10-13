<script lang="ts">
  // The Credits page (Settings > About > Credits): CREDITS.md as bundled at build time (no
  // network), the main libraries, and the donation buttons that are set up.
  import { onMount } from "svelte";
  import credits from "../../../../CREDITS.md?raw";
  import type { AboutLink } from "../../lib/types";
  import { backend, errorText } from "../../lib/backend";
  import { LIBRARIES, SUPPORT_LABELS } from "../../lib/about";
  import { parse } from "../../lib/markdown";
  import ThirdPartyLicenses from "./ThirdPartyLicenses.svelte";

  let { onback }: { onback: () => void } = $props();

  // The page title comes from the section heading here.
  const blocks = parse(credits).filter((b) => !(b.kind === "heading" && b.level === 1));
  let support = $state<AboutLink[]>([]);
  let error = $state("");

  onMount(async () => {
    try {
      support = await (await backend()).supportLinks();
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

<section class="panel">
  <div class="title">
    <button class="small" onclick={onback}>← About</button>
    <h2>Credits</h2>
  </div>
  {#if error}<p class="error">{error}</p>{/if}

  <div class="credits">
    {#each blocks as block, i (i)}
      {#if block.kind === "heading"}
        <h3>{#each block.text as t, j (j)}{#if t.code}<code>{t.text}</code>{:else if t.bold}<strong>{t.text}</strong>{:else}{t.text}{/if}{/each}</h3>
      {:else if block.kind === "paragraph"}
        <p>{#each block.text as t, j (j)}{#if t.code}<code>{t.text}</code>{:else if t.bold}<strong>{t.text}</strong>{:else}{t.text}{/if}{/each}</p>
      {:else}
        <ul>
          {#each block.items as item, k (k)}
            <li>{#each item as t, j (j)}{#if t.code}<code>{t.text}</code>{:else if t.bold}<strong>{t.text}</strong>{:else}{t.text}{/if}{/each}</li>
          {/each}
        </ul>
      {/if}
    {/each}
  </div>

  <div class="group">
    <h3>Open source libraries</h3>
    <ul class="libraries">
      {#each LIBRARIES as [name, what] (name)}
        <li><strong>{name}</strong> <span class="muted">{what}</span></li>
      {/each}
    </ul>
    <p class="hint muted">And many more; all of them, with their licences:</p>
    <ThirdPartyLicenses />
  </div>

  {#if support.length > 0}
    <div class="group">
      <h3>Support Trimcraft ♥</h3>
      <p class="hint muted">Trimcraft is free and stays free. If it makes your cockpit better, you can support its development.</p>
      <div class="row">
        {#each support as link (link)}
          <button onclick={() => open(link)}>{SUPPORT_LABELS[link] ?? link}</button>
        {/each}
      </div>
    </div>
  {/if}
</section>

<style>
  .title { display: flex; align-items: center; gap: 12px; }
  .title h2 { margin: 0; }
  .credits h3 { margin: 12px 0 4px; }
  .credits p, .credits ul { margin: 0 0 6px; }
  .credits ul, .libraries { padding-left: 20px; }
  .group { display: flex; flex-direction: column; gap: 8px; padding: 12px 0; border-top: 1px solid var(--line); }
  .libraries { margin: 0; }
  h3 { margin: 0; }
  .hint { margin: 0; font-size: 12px; }
  .small { font-size: 12px; }
  .row { display: flex; gap: 8px; flex-wrap: wrap; }
</style>

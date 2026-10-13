<script lang="ts">
  // Header dropdown: shows the active profile and switches it by hand (no actions run).
  // "Manage profiles…" opens the profile list on the Virtual Controls page.
  import { tick } from "svelte";
  import type { EngineSnapshot } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import { show } from "../lib/nav.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  const MANAGE = "__manage";
  let select = $state<HTMLSelectElement>();
  let error = $state("");

  const title = $derived(
    [
      "Active profile; switching runs no actions",
      snapshot.profileSwitching.autoSwitch ? "Auto-switch on" : "Auto-switch off",
      snapshot.foreground ? `In the foreground: ${snapshot.foreground}` : null,
      error || null,
    ]
      .filter(Boolean)
      .join("\n"),
  );

  async function choose(value: string) {
    error = "";
    if (value === MANAGE) {
      // Back to the active profile in the list; the page does the managing.
      if (select) select.value = snapshot.activeProfile;
      show("controls");
      await tick();
      document.getElementById("profiles")?.scrollIntoView({ behavior: "smooth" });
      return;
    }
    try {
      await (await backend()).selectProfile(value);
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

<label class="profile" {title}>
  <span class="muted">Profile</span>
  <select bind:this={select} class:error={!!error} value={snapshot.activeProfile} onchange={(e) => choose(e.currentTarget.value)}>
    {#each snapshot.profiles as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
    <option value={MANAGE}>Manage profiles…</option>
  </select>
  {#if snapshot.profileSwitching.autoSwitch}<span class="badge" title="Switches by the program in the foreground">auto</span>{/if}
</label>

<style>
  .profile { display: flex; gap: 6px; align-items: center; font-size: 12px; }
  select { max-width: 16em; }
  select.error { border-color: var(--bad); }
</style>

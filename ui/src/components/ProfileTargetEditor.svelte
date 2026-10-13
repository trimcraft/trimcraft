<script lang="ts">
  // Edits a "Switch profile" action: a specific profile, the next or the previous one.
  import type { ProfileTarget, ProfileView } from "../lib/types";

  let { target = $bindable(), profiles }: { target: ProfileTarget; profiles: ProfileView[] } = $props();

  const key = (t: ProfileTarget) => (t.type === "profile" ? `profile:${t.profile}` : t.type);
  const missing = $derived.by(() => {
    const t = target;
    return t.type === "profile" && !profiles.some((p) => p.id === t.profile);
  });

  function choose(value: string) {
    if (value === "next" || value === "previous") target = { type: value };
    else target = { type: "profile", profile: value.slice("profile:".length) };
  }
</script>

<div class="row">
  <select value={key(target)} onchange={(e) => choose(e.currentTarget.value)}>
    {#each profiles as p (p.id)}<option value="profile:{p.id}">{p.name}</option>{/each}
    {#if missing}<option value={key(target)} disabled>Deleted profile</option>{/if}
    <option value="next">Next profile</option>
    <option value="previous">Previous profile</option>
  </select>
</div>
<p class="muted small">
  Switches without running any actions; each profile keeps its own radio states. The choice stays until a program
  that a profile lists comes to the front.
</p>
{#if missing}<p class="error small">The profile this action switched to was deleted; choose another one.</p>{/if}

<style>
  .small { font-size: 11px; margin: 0; }
</style>

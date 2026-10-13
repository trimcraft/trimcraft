<script lang="ts">
  // The toasts, newest on top. Fading uses CSS transitions only; nothing runs while the list
  // does not change. An updated toast keeps its place and its element, only the text pulses.
  import { fade, fly } from "svelte/transition";
  import { flip } from "svelte/animate";
  import type { Toast } from "../lib/types";

  let { toasts }: { toasts: Toast[] } = $props();

  const separator = (title: string) => (title === "" ? "" : title.endsWith(":") ? " " : " · ");
</script>

<div class="toasts" aria-live="polite">
  {#each toasts as t (t.id)}
    <div class="toast" in:fly={{ y: -10, duration: 180 }} out:fade={{ duration: 250 }} animate:flip={{ duration: 150 }}>
      <span class="title">{t.title}{separator(t.title)}</span>{#key t.version}<span class="text">{t.text}</span>{/key}
    </div>
  {/each}
</div>

<style>
  .toasts { display: flex; flex-direction: column; gap: 6px; align-items: stretch; min-width: 240px; }
  .toast {
    background: var(--ov-bg);
    border: 1px solid var(--ov-border);
    border-radius: var(--ov-radius);
    padding: 8px 14px;
    color: var(--text);
    font-size: calc(18px * var(--text-scale, 1));
    white-space: nowrap;
  }
  .title { color: var(--muted); }
  .text { font-weight: 700; color: var(--accent); font-variant-numeric: tabular-nums; }
  .text { animation: bump 0.25s ease-out; }
  @keyframes bump { from { opacity: 0.4; } to { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .text { animation: none; } }
</style>

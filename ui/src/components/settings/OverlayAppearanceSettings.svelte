<script lang="ts">
  // Settings > Overlay appearance: theme, accent, background opacity, text size and corners
  // (global, all profiles), and the toast settings. The preview and the real overlay follow
  // while a slider moves.
  import type { Appearance, EngineSnapshot, Theme, ToastSettings } from "../../lib/types";
  import { backend, errorText } from "../../lib/backend";
  import { THEMES } from "../../lib/overlay-style";
  import OverlayPreview from "../OverlayPreview.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let error = $state("");
  const saved = (): Appearance => ({ ...snapshot.overlay.appearance });
  let draft = $state<Appearance>(saved());
  let pending: ReturnType<typeof setTimeout> | undefined;

  // Follow the app when nothing is being changed here (e.g. after an import).
  $effect(() => {
    const current = saved();
    if (pending === undefined) draft = current;
  });

  /** Changes the draft at once (preview) and saves shortly after the last change. */
  function change(patch: Partial<Appearance>) {
    draft = { ...draft, ...patch };
    clearTimeout(pending);
    pending = setTimeout(async () => {
      pending = undefined;
      error = "";
      try {
        await (await backend()).setOverlayAppearance($state.snapshot(draft));
      } catch (e) {
        error = errorText(e);
      }
    }, 120);
  }

  async function run(action: () => Promise<unknown>) {
    error = "";
    try {
      await action();
    } catch (e) {
      error = errorText(e);
    }
  }

  const toasts = $derived(snapshot.overlay.toasts);
  const setToasts = (patch: Partial<ToastSettings>) => run(async () => (await backend()).setToastSettings({ ...toasts, ...patch }));
  const seconds = (ms: number) => Math.round(ms / 100) / 10;
</script>

<section class="panel">
  <h2>Overlay appearance</h2>
  <p class="hint muted">For all profiles. The overlay window changes while you adjust.</p>
  {#if error}<p class="error">{error}</p>{/if}

  <OverlayPreview {snapshot} appearance={draft} />

  <div class="grid">
    <label>Theme
      <select value={draft.theme} onchange={(e) => change({ theme: e.currentTarget.value as Theme })}>
        {#each THEMES as [value, text] (value)}<option {value}>{text}</option>{/each}
      </select>
    </label>
    <label>Accent color
      <span class="row">
        <input type="color" value={draft.accent ?? "#f0a030"} oninput={(e) => change({ accent: e.currentTarget.value })} aria-label="Accent color" />
        <button class="small" disabled={draft.accent === null} onclick={() => change({ accent: null })}>Theme default</button>
      </span>
    </label>
    <label>Background opacity
      <span class="row">
        <input type="range" min="0" max="100" step="1" value={draft.backgroundOpacity} oninput={(e) => change({ backgroundOpacity: Number(e.currentTarget.value) })} />
        {draft.backgroundOpacity}%
      </span>
    </label>
    <label>Text size
      <span class="row">
        <input type="range" min="50" max="250" step="5" value={Math.round(draft.textScale * 100)} oninput={(e) => change({ textScale: Number(e.currentTarget.value) / 100 })} />
        {Math.round(draft.textScale * 100)}%
      </span>
    </label>
    <label>Corner radius
      <span class="row">
        <input type="range" min="0" max="32" step="1" value={draft.cornerRadius} oninput={(e) => change({ cornerRadius: Number(e.currentTarget.value) })} />
        {draft.cornerRadius}px
      </span>
    </label>
  </div>
  <p class="hint muted">
    Size of the whole overlay and of single widgets: Overlay panel. Widgets can also show only after a change, there.
  </p>

  <h3>Toasts</h3>
  <p class="hint muted">
    A short message on the overlay when a control changes (switch it on per control, in its editor) or the profile switches.
    Move the toast area with Edit layout; its place belongs to the profile.
  </p>
  <div class="grid">
    <label>Shown for
      <span class="row">
        <input type="number" min="0.5" max="30" step="0.5" value={seconds(toasts.durationMs)} onchange={(e) => setToasts({ durationMs: Math.round(Number(e.currentTarget.value) * 1000) || 2000 })} /> s
      </span>
    </label>
    <label class="check">
      <input type="checkbox" checked={toasts.whileHidden} onchange={(e) => setToasts({ whileHidden: e.currentTarget.checked })} />
      Show toasts while the overlay is hidden
    </label>
    <label class="check">
      <input type="checkbox" checked={toasts.profileToast} onchange={(e) => setToasts({ profileToast: e.currentTarget.checked })} />
      Show a toast when the profile switches
    </label>
    <div><button onclick={() => run(async () => (await backend()).testToast())}>Show a test toast</button></div>
  </div>
  <p class="hint muted">
    With the overlay hidden (hotkey), only the toasts appear: the window stays click-through and never takes the focus from
    the game.
  </p>
</section>

<style>
  .hint { margin: 4px 0 8px; font-size: 12px; }
  h3 { margin: 16px 0 0; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 12px 24px; margin: 12px 0; }
  label { display: flex; flex-direction: column; gap: 4px; color: var(--muted); font-size: 12px; }
  label.check { flex-direction: row; align-items: center; gap: 6px; }
  .row { display: flex; gap: 8px; align-items: center; color: var(--text); }
  input[type="number"] { width: 5em; }
  .small { font-size: 12px; }
</style>

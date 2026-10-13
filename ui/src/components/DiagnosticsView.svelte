<script lang="ts">
  import { onMount, tick } from "svelte";
  import { backend, errorText } from "../lib/backend";
  import { CATEGORIES, diagnostics as log } from "../lib/diagnostics.svelte";
  import { diagLine, diagTime } from "../lib/format";

  /** Rows drawn at most; filters and copy still work on everything the buffer holds. */
  const MAX_ROWS = 1000;
  /** Closer than this to the bottom counts as "at the bottom". */
  const BOTTOM_SLACK = 24;

  let visible = $derived(log.visible);
  let rows = $derived(visible.length > MAX_ROWS ? visible.slice(visible.length - MAX_ROWS) : visible);

  let scroller = $state<HTMLDivElement>();
  let follow = $state(true);
  let expanded = $state<number[]>([]);
  let notice = $state("");
  let noticeIsError = $state(false);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    log.start();
    return () => clearTimeout(noticeTimer);
  });

  function onScroll() {
    if (!scroller) return;
    follow = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight <= BOTTOM_SLACK;
  }

  async function toBottom() {
    await tick();
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  }

  // Keep the newest row in view while following; scrolling up stops it, scrolling back down
  // (or "Jump to latest") resumes.
  $effect(() => {
    rows;
    if (follow) toBottom();
  });

  function jump() {
    follow = true;
    toBottom();
  }

  function toggle(seq: number, hasDetail: boolean) {
    if (!hasDetail) return;
    expanded = expanded.includes(seq) ? expanded.filter((s) => s !== seq) : [...expanded, seq];
  }

  function say(text: string, isError = false) {
    notice = text;
    noticeIsError = isError;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = ""), 4000);
  }

  async function copy() {
    const lines = visible.map(diagLine);
    try {
      await navigator.clipboard.writeText(lines.join("\n"));
      say(`Copied ${lines.length} ${lines.length === 1 ? "entry" : "entries"}`);
    } catch (e) {
      say(`Could not copy: ${errorText(e)}`, true);
    }
  }

  async function openFolder() {
    try {
      await (await backend()).openLogFolder();
    } catch (e) {
      say(errorText(e), true);
    }
  }
</script>

<section class="panel">
  <div class="toolbar row">
    <h2>Event log</h2>
    <span class="muted count mono">{visible.length} of {log.entries.length}</span>
    <span class="spacer"></span>
    {#if notice}<span class="mono" class:error={noticeIsError} class:muted={!noticeIsError}>{notice}</span>{/if}
    <button class:active={log.paused} onclick={() => (log.paused ? log.resume() : log.pause())}>
      {log.paused ? `Resume${log.pending ? ` (${log.pending} new)` : ""}` : "Pause"}
    </button>
    <button onclick={() => log.clear()}>Clear</button>
    <button onclick={copy} title="Copies every entry that matches the filters, not only the rows on screen">Copy visible to clipboard</button>
    <button onclick={openFolder}>Open log folder</button>
  </div>

  <div class="filters row">
    {#each CATEGORIES as [value, text] (value)}
      <button class="chip cat-{value}" class:off={log.hidden.includes(value)} aria-pressed={!log.hidden.includes(value)} onclick={() => log.toggleCategory(value)}>{text}</button>
    {/each}
    <select bind:value={log.device} aria-label="Device filter">
      <option value="">All devices</option>
      {#each log.devices as [id, name] (id)}<option value={id}>{name}</option>{/each}
    </select>
    <input class="search" type="search" placeholder="Search…" bind:value={log.search} aria-label="Search the event log" />
  </div>

  <div class="scroller" bind:this={scroller} onscroll={onScroll}>
    {#if rows.length === 0}
      <p class="muted empty">{log.entries.length ? "No entries match the filters." : "Nothing recorded yet. Press a button or move an axis."}</p>
    {:else}
      {#if visible.length > rows.length}
        <p class="muted mono more">{visible.length - rows.length} older entries not drawn. Narrow the filters or copy to see them.</p>
      {/if}
      {#each rows as e (e.seq)}
        {@const open = expanded.includes(e.seq)}
        <div class="entry {e.level}" class:open>
          <button class="line" class:plain={!e.detail} aria-expanded={e.detail ? open : undefined} onclick={() => toggle(e.seq, !!e.detail)}>
            <span class="time mono">{diagTime(e.timeMs)}</span>
            <span class="cat cat-{e.category}">{e.category}</span>
            <span class="device muted">{e.deviceName ?? ""}</span>
            <span class="text">{e.text}</span>
            <span class="caret muted">{e.detail ? (open ? "▾" : "▸") : ""}</span>
          </button>
          {#if open && e.detail}<pre class="detail mono">{e.detail}</pre>{/if}
        </div>
      {/each}
    {/if}
  </div>
  {#if !follow}
    <button class="jump" onclick={jump}>Jump to latest ↓</button>
  {/if}
</section>

<style>
  section { position: relative; display: flex; flex-direction: column; gap: 8px; }
  h2 { margin: 0; }
  .toolbar { gap: 8px; }
  .spacer { flex: 1; }
  .count { color: var(--muted); }
  .search { flex: 1; min-width: 12em; }
  .chip { border-radius: 12px; padding: 1px 10px; font-size: 12px; }
  .chip.off { opacity: 0.45; text-decoration: line-through; border-style: dashed; }
  .scroller { height: 60vh; min-height: 260px; overflow-y: auto; border: 1px solid var(--line); border-radius: 4px; background: var(--bg); }
  .empty, .more { padding: 8px 12px; margin: 0; }
  .entry { border-bottom: 1px solid var(--panel); }
  .entry.warn { background: color-mix(in srgb, var(--warn) 10%, transparent); }
  .entry.error { background: color-mix(in srgb, var(--bad) 14%, transparent); }
  .line {
    display: grid; grid-template-columns: 7.5em 5.5em minmax(6em, 12em) 1fr 1.2em; gap: 8px; align-items: baseline;
    width: 100%; text-align: left; border: 0; border-radius: 0; background: none; padding: 2px 8px;
  }
  .line.plain { cursor: default; }
  .line:hover { background: var(--panel); }
  .time { color: var(--muted); }
  .cat { font-size: 11px; text-transform: uppercase; letter-spacing: 0.04em; }
  .device { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .text { overflow-wrap: anywhere; }
  .detail { margin: 0; padding: 2px 8px 6px calc(7.5em + 5.5em + 24px); white-space: pre-wrap; overflow-wrap: anywhere; color: var(--muted); }
  .jump { position: absolute; right: 24px; bottom: 20px; background: var(--accent); color: #111; border-color: var(--accent); font-weight: 600; }
  .cat-input { color: #9fb4f0; }
  .cat-trigger { color: var(--accent); }
  .cat-control { color: var(--ok); }
  .cat-action { color: #d58ee0; }
  .cat-connector { color: #5fc9d6; }
  .cat-log { color: var(--warn); }
  .chip.cat-input, .chip.cat-trigger, .chip.cat-control, .chip.cat-action, .chip.cat-connector, .chip.cat-log { border-color: currentColor; }
</style>

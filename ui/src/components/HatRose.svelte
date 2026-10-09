<script lang="ts">
  // 3x3 grid; the cells matching the hat's direction bits light up.
  let { mask }: { mask: number } = $props();
  const cells = [9, 1, 3, 8, 0, 2, 12, 4, 6];
  const lit = (cell: number) => cell === mask;
</script>

<div class="rose">
  {#each cells as cell}
    <span class:on={lit(cell)}></span>
  {/each}
</div>

<style>
  .rose { display: grid; grid-template-columns: repeat(3, 10px); gap: 2px; }
  .rose span { width: 10px; height: 10px; border-radius: 2px; background: var(--lamp-off); }
  .rose span.on { background: var(--lamp-on); }
</style>

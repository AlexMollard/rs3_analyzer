<script lang="ts">
  let { values, dir }: { values: number[]; dir: -1 | 0 | 1 } = $props();

  const W = 280;
  const H = 60;
  const PAD = 4;

  const points = $derived.by(() => {
    if (values.length < 2) return '';
    const min = Math.min(...values);
    const span = Math.max(...values) - min || 1;
    return values
      .map((v, i) => `${((i * W) / (values.length - 1)).toFixed(1)},${(H - PAD - ((v - min) / span) * (H - 2 * PAD)).toFixed(1)}`)
      .join(' ');
  });
  const label = $derived(
    `Price ${dir > 0 ? 'trending up' : dir < 0 ? 'trending down' : 'flat'} over the last ${values.length} days`,
  );
</script>

<svg class="spark" class:up={dir > 0} class:down={dir < 0} role="img" aria-label={label} viewBox="0 0 {W} {H}" preserveAspectRatio="none">
  {#if points}
    <polyline {points} fill="none" stroke-width="2.2" vector-effect="non-scaling-stroke" />
  {/if}
</svg>

<style>
  .spark {
    width: 100%;
    height: 60px;
    display: block;
    background: var(--bg-0);
    border-radius: 3px;
    stroke: var(--muted);
  }
  .spark.up {
    stroke: var(--profit);
  }
  .spark.down {
    stroke: var(--loss);
  }
</style>

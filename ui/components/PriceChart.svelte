<script lang="ts">
  // Interactive price history: scroll to zoom, drag to pan, hover for date + price.
  import uPlot from 'uplot';
  import { formatGp, formatVolume } from '../lib/format.ts';
  import type { HistoryPoint } from '../lib/types.ts';

  let { history }: { history: HistoryPoint[] } = $props();

  const HEIGHT = 200;
  const MIN_SPAN = 7 * 86_400; // seconds

  let host: HTMLDivElement | undefined = $state();
  let plot: uPlot | null = null;
  let tip = $state<{ left: number; top: number; flip: boolean; point: HistoryPoint } | null>(null);

  const xs = $derived(history.map((h) => Date.parse(`${h.date}T00:00:00Z`) / 1000));
  const summary = $derived.by(() => {
    if (history.length === 0) return 'No price history';
    const prices = history.map((h) => h.price);
    return (
      `Price history, ${history.length} days from ${history[0].date} to ${history[history.length - 1].date}. ` +
      `Latest ${formatGp(prices[prices.length - 1])}, range ${formatGp(Math.min(...prices))} to ${formatGp(Math.max(...prices))}.`
    );
  });

  export function resetView() {
    if (plot && xs.length > 1) plot.setScale('x', { min: xs[0], max: xs[xs.length - 1] });
  }

  function clampRange(min: number, max: number): [number, number] {
    const lo = xs[0];
    const hi = xs[xs.length - 1];
    let span = Math.min(Math.max(max - min, Math.min(MIN_SPAN, hi - lo)), hi - lo);
    if (span <= 0) span = hi - lo;
    min = Math.max(lo, Math.min(min, hi - span));
    return [min, min + span];
  }

  $effect(() => {
    const el = host;
    const data = history;
    const x = xs;
    if (!el || data.length === 0) return;

    const css = getComputedStyle(document.documentElement);
    const v = (name: string) => css.getPropertyValue(name).trim();
    const axis = {
      stroke: v('--muted'),
      grid: { stroke: v('--row-line'), width: 1 },
      ticks: { stroke: v('--border'), width: 1 },
      font: `10px ${v('--font-mono')}`,
    };

    const u = new uPlot(
      {
        width: el.clientWidth || 340,
        height: HEIGHT,
        tzDate: (ts) => uPlot.tzDate(new Date(ts * 1000), 'Etc/UTC'),
        legend: { show: false },
        scales: { x: { time: true } },
        cursor: {
          drag: { x: false, y: false, setScale: false },
          points: { size: 8, fill: v('--accent'), stroke: v('--bg-0'), width: 2 },
        },
        series: [{}, { stroke: v('--accent'), width: 2, points: { show: false } }],
        axes: [
          { ...axis, size: 28 },
          { ...axis, size: 54, values: (_u, vals) => vals.map((n) => formatGp(n)) },
        ],
        hooks: {
          setCursor: [
            (p) => {
              const idx = p.cursor.idx;
              if (idx == null || p.cursor.left == null || p.cursor.left < 0) {
                tip = null;
                return;
              }
              const left = p.over.offsetLeft + p.cursor.left;
              const top = p.over.offsetTop + p.valToPos(data[idx].price, 'y');
              tip = { left, top, flip: p.cursor.left > p.over.clientWidth / 2, point: data[idx] };
            },
          ],
        },
      },
      [x, data.map((h) => h.price)],
      el,
    );
    plot = u;

    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const { min, max } = u.scales.x;
      if (min == null || max == null) return;
      const at = u.posToVal(e.clientX - u.over.getBoundingClientRect().left, 'x');
      const f = e.deltaY < 0 ? 0.8 : 1.25;
      const [a, b] = clampRange(at - (at - min) * f, at + (max - at) * f);
      u.setScale('x', { min: a, max: b });
    };

    let drag: { x: number; min: number; max: number } | null = null;
    const onDown = (e: PointerEvent) => {
      const { min, max } = u.scales.x;
      if (e.button !== 0 || min == null || max == null) return;
      drag = { x: e.clientX, min, max };
      u.over.setPointerCapture(e.pointerId);
    };
    const onMove = (e: PointerEvent) => {
      if (!drag) return;
      const shift = (-(e.clientX - drag.x) * (drag.max - drag.min)) / u.over.clientWidth;
      const [a, b] = clampRange(drag.min + shift, drag.max + shift);
      u.setScale('x', { min: a, max: b });
    };
    const onUp = () => (drag = null);

    u.over.addEventListener('wheel', onWheel, { passive: false });
    u.over.addEventListener('pointerdown', onDown);
    u.over.addEventListener('pointermove', onMove);
    u.over.addEventListener('pointerup', onUp);
    u.over.addEventListener('pointercancel', onUp);

    const ro = new ResizeObserver(() => u.setSize({ width: el.clientWidth, height: HEIGHT }));
    ro.observe(el);

    return () => {
      ro.disconnect();
      u.destroy();
      plot = null;
      tip = null;
    };
  });
</script>

<figure class="chart">
  <div class="plot" role="img" aria-label={summary}>
    <div bind:this={host}></div>
    {#if tip}
      <div class="tip mono" class:flip={tip.flip} style:left="{tip.left}px" style:top="{tip.top}px" aria-hidden="true">
        <div class="muted">{tip.point.date}</div>
        <div>{formatGp(tip.point.price)}</div>
        <div class="muted">vol {formatVolume(tip.point.volume)}</div>
      </div>
    {/if}
  </div>
  <figcaption>
    <span>Scroll to zoom · drag to pan</span>
    <button class="link-btn" onclick={resetView}>Reset view</button>
  </figcaption>
</figure>

<style>
  .chart {
    margin: 0;
    background: var(--bg-0);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 10px 6px;
  }
  .plot {
    position: relative;
    min-height: 200px;
    cursor: grab;
  }
  .plot:active {
    cursor: grabbing;
  }
  .tip {
    position: absolute;
    z-index: 2;
    pointer-events: none;
    transform: translate(12px, -50%);
    background: var(--panel);
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    padding: 4px 8px;
    font-size: 11px;
    white-space: nowrap;
    color: var(--text);
  }
  .tip.flip {
    transform: translate(calc(-100% - 12px), -50%);
  }
  figcaption {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    color: var(--muted);
    padding-top: 4px;
  }
  .plot :global(.u-cursor-x),
  .plot :global(.u-cursor-y) {
    border-color: var(--bevel);
  }
  .plot :global(.u-cursor-y) {
    display: none;
  }
</style>

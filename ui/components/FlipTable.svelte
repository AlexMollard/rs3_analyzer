<script lang="ts">
  // Fixed-row-height windowing: only rows in (or near) the viewport are rendered.
  // Columns drop out by priority as the pane narrows, so the table never needs horizontal scroll
  // at or above the window's minimum size.
  import { untrack } from 'svelte';
  import { app } from '../lib/store.svelte.ts';
  import { formatGp, formatPct, formatVolume, limitText, qtyText, RISK_LABEL, trendInfo } from '../lib/format.ts';
  import type { SortKey } from '../lib/flips.ts';
  import type { PricedRow } from '../lib/types.ts';
  import TierBadge from './TierBadge.svelte';

  const ROW_H = 36;
  const OVERSCAN = 8;

  type ColKey = SortKey | 'star';
  // width: px (item column flexes); hideBelow: pane width under which the column is dropped.
  // Thresholds keep the item column at least ~140px wide.
  type Col = { key: ColKey; label: string; width?: number; hideBelow?: number; title?: string };
  const COLS: Col[] = [
    { key: 'star', label: 'Favourite', width: 34 },
    { key: 'name', label: 'Item' },
    { key: 'tier', label: 'Tier', width: 52 },
    { key: 'risk', label: 'Risk', width: 40, hideBelow: 830, title: 'Crash, spike and outlier warnings' },
    { key: 'buy', label: 'Buy', width: 72 },
    { key: 'sell', label: 'Sell', width: 72, hideBelow: 720 },
    { key: 'unitProfit', label: 'Profit/ea', width: 88, hideBelow: 800, title: 'Net profit per item after GE tax' },
    { key: 'roi', label: 'ROI', width: 64 },
    { key: 'qty', label: 'Qty', width: 76, hideBelow: 640, title: 'Quantity at your budget, capped by the GE limit' },
    { key: 'avgVolume', label: 'Vol/day', width: 74, hideBelow: 930, title: 'Average daily volume' },
    { key: 'profit', label: 'Total', width: 76, title: 'Total profit at your budget' },
    { key: 'score', label: 'Score', width: 62, hideBelow: 570 },
    { key: 'trend', label: 'Trend', width: 60 },
  ];

  let scroller: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewH = $state(600);
  let paneW = $state(1000);
  let headH = $state(34);

  const cols = $derived(COLS.filter((c) => !c.hideBelow || paneW >= c.hideBelow));
  const rows = $derived(app.visible);
  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW_H) - OVERSCAN));
  const end = $derived(Math.min(rows.length, Math.ceil((scrollTop + viewH) / ROW_H) + OVERSCAN));
  const slice = $derived(rows.slice(start, end));

  function sortBy(key: SortKey) {
    if (app.sort.key === key) app.sort.desc = !app.sort.desc;
    else app.sort = { key, desc: key !== 'name' };
  }

  function ariaSort(key: ColKey): 'ascending' | 'descending' | undefined {
    if (key === 'star' || app.sort.key !== key) return undefined;
    return app.sort.desc ? 'descending' : 'ascending';
  }

  // Keep the selected row in view when the selection changes (keyboard moves, "Open in desk").
  // Only selectedId is tracked: starring, filtering or editing the budget must not yank the scroll.
  $effect(() => {
    const id = app.selectedId;
    const el = scroller;
    if (id === null || !el) return;
    untrack(() => {
      const i = rows.findIndex((r) => r.itemId === id);
      const visibleH = el.clientHeight - headH;
      if (i < 0 || visibleH <= 0) return;
      const top = i * ROW_H;
      if (top < el.scrollTop) el.scrollTop = top;
      else if (top + ROW_H > el.scrollTop + visibleH) el.scrollTop = top + ROW_H - visibleH;
      // Don't wait for the scroll event (only fired on a rendered frame) to move the window
      scrollTop = el.scrollTop;
    });
  });

  function onKey(e: KeyboardEvent) {
    const page = Math.max(1, Math.floor((viewH - headH) / ROW_H) - 1);
    const moves: Record<string, number | 'first' | 'last'> = {
      ArrowDown: 1,
      ArrowUp: -1,
      PageDown: page,
      PageUp: -page,
      Home: 'first',
      End: 'last',
    };
    if (e.key in moves) {
      e.preventDefault();
      app.moveSelection(moves[e.key]);
    }
  }
</script>

{#snippet cell(r: PricedRow, key: ColKey)}
  {#if key === 'star'}
    {@const fav = app.favorites.has(r.name)}
    <td class="c-star">
      <button
        class="star"
        class:on={fav}
        aria-pressed={fav}
        aria-label={fav ? `Remove ${r.name} from favourites` : `Add ${r.name} to favourites`}
        onclick={(e) => {
          e.stopPropagation();
          app.toggleFavorite(r.name);
        }}>★</button
      >
    </td>
  {:else if key === 'name'}
    <td class="c-name" title={r.name}>{r.name}</td>
  {:else if key === 'tier'}
    <td class="c-tier"><TierBadge tier={r.tier} /></td>
  {:else if key === 'risk'}
    {@const risk = RISK_LABEL[r.risk]}
    <td class="c-risk {risk.cls}" title="{risk.text.replace(/^\S+ /, '')}: {r.notes}">
      <span aria-hidden="true">{risk.text.split(' ')[0]}</span><span class="sr-only">{risk.text.replace(/^\S+ /, '')}</span>
    </td>
  {:else if key === 'buy'}
    <td class="mono">{formatGp(r.buy)}</td>
  {:else if key === 'sell'}
    <td class="mono">{formatGp(r.sell)}</td>
  {:else if key === 'unitProfit'}
    <td class="mono" class:profit={r.unitProfit > 0} class:loss={r.unitProfit < 0}>{formatGp(r.unitProfit)}</td>
  {:else if key === 'roi'}
    <td class="mono" class:loss={r.roi < 0}>{formatPct(r.roi)}</td>
  {:else if key === 'qty'}
    <td class="mono muted" title={limitText(r)}>{qtyText(r)}</td>
  {:else if key === 'avgVolume'}
    <td class="mono muted">{formatVolume(r.avgVolume)}</td>
  {:else if key === 'profit'}
    <td class="mono" class:profit={r.profit > 0} class:loss={r.profit < 0}>{formatGp(r.profit)}</td>
  {:else if key === 'score'}
    <td class="mono" class:loss={r.score < 0}>{r.score}</td>
  {:else if key === 'trend'}
    {@const trend = trendInfo(r.trend)}
    <td class="mono" class:profit={trend.dir > 0} class:loss={trend.dir < 0} title={trend.label}>{trend.arrow}</td>
  {/if}
{/snippet}

<div
  class="scroller"
  bind:this={scroller}
  bind:clientHeight={viewH}
  bind:clientWidth={paneW}
  onscroll={() => (scrollTop = scroller?.scrollTop ?? 0)}
>
  <table
    role="grid"
    tabindex="0"
    aria-rowcount={rows.length + 1}
    aria-label="Flips, {rows.length} rows. Arrow keys change the selected item."
    onkeydown={onKey}
  >
    <colgroup>
      {#each cols as c (c.key)}<col style:width={c.width ? `${c.width}px` : null} />{/each}
    </colgroup>
    <thead bind:clientHeight={headH}>
      <tr>
        {#each cols as c (c.key)}
          <th scope="col" class:c-name={c.key === 'name'} aria-sort={ariaSort(c.key)} title={c.title}>
            {#if c.key === 'star'}
              <span class="sr-only">{c.label}</span>
            {:else}
              {@const key = c.key}
              <button class="sort" class:active={app.sort.key === key} onclick={() => sortBy(key)}>
                {c.label}{#if app.sort.key === key}<span aria-hidden="true">{app.sort.desc ? ' ↓' : ' ↑'}</span>{/if}
              </button>
            {/if}
          </th>
        {/each}
      </tr>
    </thead>
    <tbody>
      {#if start > 0}<tr class="spacer" aria-hidden="true" style:height="{start * ROW_H}px"><td colspan={cols.length}></td></tr>{/if}
      {#each slice as r, i (r.itemId)}
        <tr
          class:selected={app.selectedId === r.itemId}
          aria-selected={app.selectedId === r.itemId}
          aria-rowindex={start + i + 2}
          onclick={() => app.toggleSelect(r.itemId)}
        >
          {#each cols as c (c.key)}{@render cell(r, c.key)}{/each}
        </tr>
      {/each}
      {#if end < rows.length}<tr class="spacer" aria-hidden="true" style:height="{(rows.length - end) * ROW_H}px"><td colspan={cols.length}></td></tr>{/if}
    </tbody>
  </table>
</div>

<style>
  .scroller {
    flex: 1;
    min-height: 0;
    min-width: 0;
    overflow: auto;
    position: relative;
    scrollbar-gutter: stable;
  }
  table {
    width: 100%;
    table-layout: fixed;
    border-collapse: collapse;
    font-size: 13px;
  }
  table:focus-visible {
    outline-offset: -2px;
  }
  thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--bg-0);
    border-bottom: 1px solid var(--border);
    padding: 0;
    text-align: right;
    font-weight: 500;
  }
  th.c-name {
    text-align: left;
  }
  .sort {
    width: 100%;
    background: none;
    border: 0;
    padding: 10px 6px;
    color: var(--muted);
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.3px;
    text-align: inherit;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  th.c-name .sort {
    padding-left: 4px;
  }
  .sort:hover {
    color: var(--text);
  }
  .sort.active {
    color: var(--accent-hi);
  }
  .sort:focus-visible {
    outline-offset: -2px;
  }
  tbody tr {
    height: 36px;
    cursor: pointer;
  }
  tbody tr:not(.spacer):hover {
    background: #1d1a16;
  }
  tbody tr.selected,
  tbody tr.selected:hover {
    background: var(--selected);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  tr.spacer {
    cursor: default;
  }
  td {
    border-bottom: 1px solid var(--row-line);
    padding: 0 6px;
    text-align: right;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  tr.spacer td {
    border: 0;
    padding: 0;
  }
  td.c-name {
    text-align: left;
    padding-left: 4px;
  }
  td.c-star {
    padding: 0 0 0 6px;
    text-align: left;
  }
  td.c-tier,
  td.c-risk {
    text-align: center;
  }
  .star {
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    font-size: 15px;
    color: var(--star-off);
  }
  .star:hover {
    color: var(--text-soft);
  }
  .star.on {
    color: var(--accent);
  }
  .r-stable {
    color: var(--profit);
  }
  .r-outliers {
    color: var(--muted);
  }
  .r-spiked {
    color: var(--tier-b);
  }
  .r-crash {
    color: var(--loss);
  }
</style>

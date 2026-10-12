<script lang="ts">
  import { app } from '../lib/store.svelte.ts';
  import { backend, errorText } from '../lib/api.ts';
  import { copyText, formatGp, formatInt, formatPct, formatVolume, limitText, qtyText, TIER_LABEL, toFixedRust, trendInfo } from '../lib/format.ts';
  import type { HistoryPoint, PricedRow } from '../lib/types.ts';
  import PriceChart from './PriceChart.svelte';

  let history = $state.raw<HistoryPoint[]>([]);
  let historyState = $state<'loading' | 'ready' | 'error'>('ready');
  let historyError = $state('');

  const sel = $derived(app.selected);
  const trend = $derived(sel ? trendInfo(sel.trend) : null);

  // Reload history whenever the selected item changes; ignore responses for stale selections
  $effect(() => {
    const id = app.selectedId;
    history = [];
    if (id === null) return;
    let stale = false;
    historyState = 'loading';
    backend()
      .then((b) => b.itemHistory(id))
      .then((h) => {
        if (stale) return;
        history = h;
        historyState = 'ready';
      })
      .catch((e) => {
        if (stale) return;
        historyError = errorText(e);
        historyState = 'error';
      });
    return () => {
      stale = true;
    };
  });

  type Note = { tone: 'ok' | 'warn' | 'error' | 'info'; text: string };

  function riskNotes(r: PricedRow): Note[] {
    const notes: Note[] = [];
    if (r.risk === 'crashing') notes.push({ tone: 'error', text: 'Volatile and crashing: recent prices are well below the 90-day highs' });
    else if (r.risk === 'crashed') notes.push({ tone: 'error', text: 'Crashed: the 14-day median is well below the 90-day median, so prices use the recent window' });
    else if (r.risk === 'spiked') notes.push({ tone: 'warn', text: 'Spiked: the 14-day median is 25%+ above the 90-day median and may fall back' });
    else notes.push({ tone: 'ok', text: 'No crash or spike detected in the last 14 days' });
    if (r.outliers > 0) notes.push({ tone: 'warn', text: `${r.outliers} outlier${r.outliers === 1 ? '' : 's'} removed (update-day spikes)` });
    if (r.tier === 'X') notes.push({ tone: 'error', text: 'Net loss after GE tax at these prices' });
    if (r.geLimit === null) notes.push({ tone: 'warn', text: 'GE limit unknown: quantity is not capped' });
    else if (r.qty === r.geLimit && r.buy * r.geLimit < app.budget)
      notes.push({ tone: 'info', text: `GE limit caps quantity at ${formatInt(r.geLimit)} per 4 h` });
    if (r.dataPoints < 14) notes.push({ tone: 'warn', text: `Only ${r.dataPoints} days of data` });
    return notes;
  }

  async function copy(r: PricedRow) {
    try {
      await navigator.clipboard.writeText(copyText(r));
      app.setStatus(`Copied flip summary for ${r.name}`, 'ok');
    } catch (e) {
      app.setStatus(`Could not copy to the clipboard: ${errorText(e)}`, 'error');
    }
  }
</script>

<section class="detail" aria-label="Selected item">
  {#if sel && trend}
    <div class="head">
      <div class="title-block">
        <h2>{sel.name}</h2>
        <div class="sub">
          Tier {TIER_LABEL[sel.tier]} · {sel.geLimit === null ? 'GE limit ?' : `GE limit ${formatInt(sel.geLimit)}`} · score {sel.score} · 90-day window
        </div>
      </div>
      <button class="icon-btn" aria-label="Close details" onclick={() => app.select(null)}>✕</button>
    </div>

    {#if historyState === 'loading'}
      <div class="chart-placeholder">Loading price history…</div>
    {:else if historyState === 'error'}
      <div class="chart-placeholder loss">{historyError}</div>
    {:else if history.length === 0}
      <div class="chart-placeholder">No price history</div>
    {:else}
      <PriceChart {history} />
    {/if}

    <div class="tiles">
      <div class="tile"><div class="label">Buy at (Q{sel.band[0]})</div><div class="value mono">{formatGp(sel.buy)}</div></div>
      <div class="tile"><div class="label">Sell at (Q{sel.band[1]})</div><div class="value mono">{formatGp(sel.sell)}</div></div>
      <div class="tile"><div class="label">GE tax / ea</div><div class="value mono">{formatGp(sel.tax)}</div></div>
      <div class="tile" title="{trend.label} (linear trend over the 90-day window, per daily sample)">
        <div class="label">Trend</div>
        <div class="value mono" class:profit={trend.dir > 0} class:loss={trend.dir < 0}>
          {trend.arrow} {sel.trend >= 0 ? '+' : ''}{toFixedRust(sel.trend, 2)}%/day
        </div>
      </div>
      <div class="tile" title={limitText(sel)}><div class="label">Qty at budget</div><div class="value mono">{qtyText(sel)}</div></div>
      <div class="tile">
        <div class="label">Total profit</div>
        <div class="value mono" class:profit={sel.profit > 0} class:loss={sel.profit < 0}>{formatGp(sel.profit)}</div>
      </div>
    </div>

    <div class="facts mono">
      <span>ROI {formatPct(sel.roi)}</span>
      <span>Cost {formatGp(sel.totalCost)}</span>
      <span>Profit/ea {formatGp(sel.unitProfit)}</span>
      <span>Vol {formatVolume(sel.avgVolume)}/day</span>
    </div>

    <div class="notes">
      <h3>Risk notes</h3>
      <ul>
        {#each riskNotes(sel) as n (n.text)}
          <li><span class="dot {n.tone}" aria-hidden="true"></span><span>{n.text}</span></li>
        {/each}
      </ul>
      <p class="analysis mono" title="Analysis notes">{sel.notes}</p>
    </div>

    <button class="btn btn-tall copy" onclick={() => copy(sel)}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="9" y="9" width="12" height="12" rx="2" /><path d="M5 15V5a2 2 0 0 1 2-2h10" /></svg>
      Copy flip summary
    </button>
  {:else}
    <div class="empty">
      <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path d="M3 17l6-6 4 4 8-8" /><path d="M14 7h7v7" /></svg>
      <h2>No item selected</h2>
      <p>
        {app.result ? 'Pick a row to see its price chart and risk notes.' : 'Pick a row once the scan finishes to see its price chart and risk notes.'}
      </p>
    </div>
  {/if}
</section>

<style>
  .detail {
    width: 360px;
    flex: none;
    border-left: 1px solid var(--border);
    background: var(--bg-1);
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    overflow-y: auto;
    position: relative;
    min-height: 0;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 8px;
  }
  .title-block {
    min-width: 0;
  }
  h2 {
    font-size: 19px;
    overflow-wrap: anywhere;
  }
  .sub {
    font-size: 13px;
    color: var(--muted);
    margin-top: 2px;
  }
  .chart-placeholder {
    height: 236px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-0);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    font-style: italic;
    padding: 12px;
    text-align: center;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }
  .tile {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px;
    box-shadow: inset 0 1px 0 var(--border);
  }
  .label {
    font-size: 12px;
    color: var(--muted);
  }
  .value {
    font-size: 16px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 14px;
    font-size: 12px;
    color: var(--muted);
  }
  .notes {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 12px;
    font-size: 13px;
  }
  h3 {
    font-size: 12px;
    letter-spacing: 0.6px;
    color: var(--text-soft);
    margin-bottom: 6px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  li {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .dot.info {
    background: var(--text-soft);
  }
  .analysis {
    margin: 8px 0 0;
    font-size: 11px;
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .copy {
    width: 100%;
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    text-align: center;
    color: var(--muted);
  }
  .empty svg {
    color: var(--bevel);
  }
  .empty h2 {
    font-size: 15px;
    color: var(--text-soft);
  }
  .empty p {
    max-width: 260px;
    line-height: 1.5;
    margin: 0;
  }
</style>

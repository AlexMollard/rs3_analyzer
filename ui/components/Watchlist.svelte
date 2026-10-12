<script lang="ts">
  import { app } from '../lib/store.svelte.ts';
  import { formatGp, formatInt, formatPct, trendInfo } from '../lib/format.ts';
  import type { PricedRow } from '../lib/types.ts';
  import Sparkline from './Sparkline.svelte';
  import TierBadge from './TierBadge.svelte';

  type CardSort = 'score' | 'profit' | 'roi' | 'name';
  let sort = $state<CardSort>('score');

  const cards = $derived.by(() => {
    const q = app.search.trim().toLowerCase();
    const rows = app.priced.filter((r) => app.favorites.has(r.name) && (q === '' || r.name.toLowerCase().includes(q)));
    const cmp: Record<CardSort, (a: PricedRow, b: PricedRow) => number> = {
      score: (a, b) => b.score - a.score,
      profit: (a, b) => b.profit - a.profit,
      roi: (a, b) => b.roi - a.roi,
      name: (a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0),
    };
    return rows.sort(cmp[sort]);
  });
  // Favourites with no row in the current scan (or before any scan)
  const missing = $derived.by(() => {
    const present = new Set(app.priced.map((r) => r.name));
    return [...app.favorites].filter((n) => !present.has(n)).sort();
  });
</script>

{#if app.favorites.size === 0}
  <main class="empty-wrap">
    <div class="card empty">
      <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path d="M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z" /></svg>
      <h1>No favourites yet</h1>
      <p class="muted">Star items on the Trading Desk to track them here with their price trend and profit at your budget.</p>
      <button class="btn btn-primary btn-tall" onclick={() => (app.tab = 'desk')}>Go to Trading Desk</button>
    </div>
  </main>
{:else}
  <main class="watch">
    <div class="head">
      <div>
        <h1>Watchlist</h1>
        <div class="muted small">
          {app.favorites.size} favourite{app.favorites.size === 1 ? '' : 's'} · totals at your {formatGp(app.budget)} budget
        </div>
      </div>
      <label class="sort muted">
        Sort
        <select bind:value={sort}>
          <option value="score">Score</option>
          <option value="profit">Total profit</option>
          <option value="roi">ROI</option>
          <option value="name">Name</option>
        </select>
      </label>
    </div>

    {#if !app.result}
      <p class="muted">{app.scanning ? 'Scanning…' : 'Scan the market to see prices for your favourites.'}</p>
    {:else if cards.length === 0 && app.search}
      <p class="muted">No favourites match “{app.search}”.</p>
    {/if}

    <div class="grid">
      {#each cards as c (c.itemId)}
        {@const trend = trendInfo(c.trend)}
        <article class="card item">
          <div class="item-head">
            <div class="min0">
              <div class="name-row">
                <TierBadge tier={c.tier} />
                <h2>{c.name}</h2>
              </div>
              <div class="muted small">GE limit {c.geLimit === null ? '?' : formatInt(c.geLimit)} · score {c.score}</div>
            </div>
            <button class="star-btn on" aria-pressed="true" aria-label="Remove {c.name} from favourites" onclick={() => app.toggleFavorite(c.name)}>★</button>
          </div>
          <Sparkline values={c.spark} dir={trend.dir} />
          <div class="stats">
            <div><div class="label">Buy</div><div class="mono">{formatGp(c.buy)}</div></div>
            <div><div class="label">Sell</div><div class="mono">{formatGp(c.sell)}</div></div>
            <div><div class="label">ROI</div><div class="mono" class:loss={c.roi < 0}>{formatPct(c.roi)}</div></div>
          </div>
          <div class="foot">
            <span class="total mono" class:profit={c.profit > 0} class:loss={c.profit < 0} title="Total profit at your budget">{formatGp(c.profit)}</span>
            <button class="link-btn" onclick={() => app.openInDesk(c.itemId)}>Open in desk →</button>
          </div>
        </article>
      {/each}

      {#each missing as name (name)}
        <article class="card item missing">
          <div class="item-head">
            <div class="min0">
              <h2>{name}</h2>
              <div class="muted small">{app.result ? 'Not in the current scan' : 'No scan data yet'}</div>
            </div>
            <button class="star-btn on" aria-pressed="true" aria-label="Remove {name} from favourites" onclick={() => app.toggleFavorite(name)}>★</button>
          </div>
        </article>
      {/each}
    </div>
  </main>
{/if}

<style>
  .watch {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow-y: auto;
    position: relative;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }
  h1 {
    font-size: 22px;
  }
  .small {
    font-size: 13px;
  }
  .sort {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  select {
    height: 36px;
    background: var(--bg-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    color: var(--text);
    padding: 0 10px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 16px;
  }
  .item {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .item.missing {
    opacity: 0.8;
  }
  .item-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 8px;
  }
  .min0 {
    min-width: 0;
  }
  .name-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  h2 {
    font-size: 16px;
    overflow-wrap: anywhere;
  }
  .small {
    margin-top: 4px;
    font-size: 12px;
  }
  .star-btn {
    width: 44px;
    height: 44px;
    flex: none;
    border-radius: var(--radius);
    border: 1px solid var(--accent);
    background: var(--selected);
    color: var(--accent);
    font-size: 18px;
  }
  .star-btn:hover {
    background: var(--chip-on);
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px;
  }
  .label {
    font-size: 11px;
    color: var(--muted);
  }
  .foot {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }
  .total {
    font-size: 17px;
  }
  .link-btn {
    font-size: 13px;
  }
  .empty-wrap {
    flex: 1;
    display: flex;
    justify-content: center;
    align-items: center;
    padding: 40px 16px;
  }
  .empty {
    max-width: 460px;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    padding: 36px 32px;
    font-size: 15px;
  }
  .empty svg {
    color: var(--bevel);
  }
  .empty p {
    margin: 0;
    line-height: 1.55;
  }
</style>

<script lang="ts">
  import { app } from '../lib/store.svelte.ts';
  import { formatGp, formatInt, formatPct, formatVolume } from '../lib/format.ts';
  import type { StrategyResult } from '../lib/types.ts';

  const HOUR = 3_600_000;
  const info = $derived(app.result?.strategy ?? null);

  // The schedule's "now" moves on its own while the tab is open
  let now = $state(Date.now());
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(timer);
  });

  // The signal day's hours, as epoch ms
  const day = $derived.by(() => {
    if (!info?.signalDate) return null;
    const [y, m, d] = info.signalDate.split('-').map(Number);
    const start = Date.UTC(y, m - 1, d, info.signalHour);
    return {
      start,
      entryEnd: start + info.entryHours * HOUR,
      exitStart: start + (info.horizonHours - 1) * HOUR,
      end: start + info.horizonHours * HOUR,
    };
  });

  const localFmt = new Intl.DateTimeFormat(undefined, { weekday: 'short', hour: '2-digit', minute: '2-digit' });
  const utc = (ms: number) => new Date(ms).toISOString().slice(11, 16);
  const local = (ms: number) => localFmt.format(ms);

  function duration(ms: number): string {
    const min = Math.max(1, Math.round(ms / 60_000));
    return min >= 60 ? `${Math.floor(min / 60)} h ${min % 60} min` : `${min} min`;
  }

  type Phase = 'before' | 'entry' | 'hold' | 'exit' | 'over';
  const phase = $derived.by((): { key: Phase; text: string } | null => {
    if (!day) return null;
    if (now < day.start) return { key: 'before', text: `Trading starts at ${utc(day.start)} UTC (${local(day.start)} your time).` };
    if (now < day.entryEnd)
      return { key: 'entry', text: `Entry window open: bids can fill for another ${duration(day.entryEnd - now)}.` };
    if (now < day.exitStart)
      return { key: 'hold', text: `Entry closed at ${utc(day.entryEnd)} UTC: cancel unfilled bids and keep your sell offers up.` };
    if (now < day.end) return { key: 'exit', text: `Forced exit: sell anything left at the instant-sell price before ${utc(day.end)} UTC.` };
    return { key: 'over', text: `This plan's day ended at ${utc(day.end)} UTC on ${new Date(day.end).toISOString().slice(0, 10)}.` };
  });
  const marker = $derived(day && now >= day.start && now < day.end ? ((now - day.start) / (day.end - day.start)) * 100 : null);

  // The tested budget closest to yours (on a log scale), highlighted in the results table
  const nearest = $derived.by((): StrategyResult | null => {
    const rows = info?.results ?? [];
    let best: StrategyResult | null = null;
    for (const r of rows) {
      if (!best || Math.abs(Math.log(r.budget / app.budget)) < Math.abs(Math.log(best.budget / app.budget))) best = r;
    }
    return best;
  });
  const alloc = $derived(info ? app.budget / info.picks : 0);
</script>

<main class="strategy">
  <div class="head">
    <div class="min0">
      <div class="title-row">
        <h1>Hourly Strategy</h1>
        {#if info?.validated}
          <span class="badge ok">Passed the held-out test</span>
        {:else}
          <span class="badge warn">Experimental · not validated</span>
        {/if}
      </div>
      {#if info}
        <p class="muted desc"><span class="mono name">{info.name}</span> · {info.description}</p>
      {/if}
    </div>
  </div>

  {#if !app.result}
    <p class="muted">{app.scanning ? 'Scanning…' : 'Scan the market to see the strategy’s picks.'}</p>
  {:else if info}
    <div class="callout" class:validated={info.validated} role="note">
      {#if info.validated}
        <strong>Promising, not proven.</strong> It met the pre-registered bar (at least 70% of trades profitable and a
        positive profit per day, top {info.picks} at 50M) on the held-out test, {info.testDays}. That is a short sample,
        and the backtest’s fills are estimates from hourly averages (see Risks).
      {:else}
        <strong>Experimental.</strong> It did not meet the pre-registered bar (at least 70% of trades profitable and a
        positive profit per day, top {info.picks} at 50M) on the held-out test, {info.testDays}, so it is not validated.
      {/if}
      The Trading Desk’s Score ranking stays the default; nothing here changes it.
    </div>

    <div class="grid">
      <section class="card" aria-labelledby="schedule-h">
        <h2 id="schedule-h">Schedule</h2>
        {#if day && phase}
          <p class="small muted">
            Targets for <span class="mono strong">{info.signalDate}</span> (UTC) · hourly data through
            <span class="mono">{info.dataThrough}</span> UTC
          </p>
          <div class="timeline" aria-hidden="true">
            <div class="seg entry" style:flex-grow={info.entryHours}>Bids live</div>
            <div class="seg hold" style:flex-grow={info.horizonHours - info.entryHours - 1}>Sell offers</div>
            <div class="seg exit" style:flex-grow={1}></div>
            {#if marker !== null}<div class="now" style:left="{marker}%"></div>{/if}
          </div>
          <div class="ticks mono" aria-hidden="true">
            <span>{utc(day.start)}</span><span>{utc(day.entryEnd)}</span><span>{utc(day.exitStart)}</span>
          </div>
          <p class="phase" class:over={phase.key === 'over'}>
            <span class="dot" class:ok={phase.key === 'entry'} class:warn={phase.key === 'hold' || phase.key === 'exit'} aria-hidden="true"></span>
            {phase.text}
            {#if phase.key === 'over'}A day’s targets need the hours through 23:00 UTC the day before, which the collector stores
              from 01:00 UTC: run it, then scan again.{/if}
          </p>
          <ol class="steps">
            <li>
              <span class="when mono">{utc(day.start + HOUR)} UTC <span class="muted">· {local(day.start + HOUR)}</span></span>
              Once the picks appear, place a buy offer for each at its <strong>Buy at</strong> price and quantity. (The
              backtest bid from {utc(day.start)} UTC; the data it needs is stored an hour later.)
            </li>
            <li>
              <span class="when mono">Until {utc(day.entryEnd)} UTC</span>
              When a buy fills, offer what you bought at the <strong>Sell at</strong> price. After a full sale, buy again.
            </li>
            <li>
              <span class="when mono">{utc(day.entryEnd)} UTC <span class="muted">· {local(day.entryEnd)}</span></span>
              Cancel buy offers that have not filled. Keep sell offers up.
            </li>
            <li>
              <span class="when mono">{utc(day.exitStart)} UTC <span class="muted">· {local(day.exitStart)}</span></span>
              Sell anything left at the instant-sell price.
            </li>
          </ol>
        {:else}
          <p class="muted">
            No recent hourly prices. Run <span class="mono">rs3_collector.py</span> (or <span class="mono">backfill_rt.py</span>) to store
            the hourly buckets the strategy reads, then scan again.
          </p>
        {/if}
      </section>

      <section class="card" aria-labelledby="results-h">
        <h2 id="results-h">Held-out results</h2>
        {#if info.results.length > 0}
          <p class="small muted">
            Backtest on {info.testDays}, top {info.picks} picks a day. Profit does not grow in step with the budget: thin
            markets cap the size. Your budget’s nearest row is marked.
          </p>
          <table>
            <caption class="sr-only">Held-out test results by budget</caption>
            <thead>
              <tr>
                <th scope="col">Budget</th>
                <th scope="col">Trades</th>
                <th scope="col">Win rate <span class="muted">(95% CI)</span></th>
                <th scope="col">Profit/day <span class="muted">(95% CI)</span></th>
              </tr>
            </thead>
            <tbody>
              {#each info.results as r (r.budget)}
                <tr class:near={r === nearest}>
                  <th scope="row" class="mono">{formatGp(r.budget)}{#if r === nearest}<span class="sr-only"> (nearest to your budget)</span>{/if}</th>
                  <td class="mono">{formatInt(r.trips)}</td>
                  <td class="mono">
                    <span class:profit={r.winRate >= 0.7} class:loss={r.winRate < 0.7}>{formatPct(r.winRate * 100)}</span>
                    <span class="ci">{formatPct(r.winCi[0] * 100)} – {formatPct(r.winCi[1] * 100)}</span>
                  </td>
                  <td class="mono">
                    <span class:profit={r.profitDay > 0} class:loss={r.profitDay <= 0}>{formatGp(r.profitDay)}</span>
                    <span class="ci">{formatGp(r.profitCi[0])} – {formatGp(r.profitCi[1])}</span>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {:else}
          <p class="muted">No held-out results in this build.</p>
        {/if}
      </section>
    </div>

    <section class="card picks" aria-labelledby="picks-h">
      <div class="picks-head">
        <h2 id="picks-h">Picks</h2>
        <span class="small muted">
          {formatGp(alloc)} each of your {formatGp(app.budget)} budget ·
          <button class="link-btn" onclick={() => (app.tab = 'desk')}>change it on the Trading Desk</button>
        </span>
      </div>
      {#if phase?.key === 'over'}
        <p class="stale" role="status">These targets were for {info.signalDate} (UTC), which is over. Scan again after the collector has run.</p>
      {/if}
      {#if !day}
        <p class="muted">No picks without recent hourly prices.</p>
      {:else if app.picks.length === 0}
        <p class="muted">No item passes the strategy’s filters for {info.signalDate} at this budget.</p>
      {:else}
        <table class="pick-table">
          <caption class="sr-only">Today’s picks with buy and sell targets</caption>
          <thead>
            <tr>
              <th scope="col" class="rank">#</th>
              <th scope="col" class="item">Item</th>
              <th scope="col">Buy at</th>
              <th scope="col">Sell at</th>
              <th scope="col">Margin/ea</th>
              <th scope="col">Qty</th>
              <th scope="col" title="Margin times the expected round-trip size">Est. profit/trip</th>
              <th scope="col" class="opt">Vol/hour</th>
              <th scope="col" class="opt">GE limit</th>
            </tr>
          </thead>
          <tbody>
            {#each app.picks as p, i (p.row.itemId)}
              <tr>
                <td class="rank mono muted">{i + 1}</td>
                <td class="item">
                  <button class="link-btn item-btn" onclick={() => app.openInDesk(p.row.itemId)} aria-label="{p.row.name}: open in the Trading Desk"
                    >{p.row.name}</button
                  >
                </td>
                <td class="mono">{formatGp(p.quote.buy)}</td>
                <td class="mono">{formatGp(p.quote.sell)}</td>
                <td class="mono profit">{formatGp(p.quote.margin)}</td>
                <td class="mono">{formatInt(p.qty)}{p.row.geLimit === null ? '?' : ''}</td>
                <td class="mono">{formatGp(p.tripProfit)}</td>
                <td class="mono opt">{formatVolume(p.quote.liquidity)}</td>
                <td class="mono opt">{p.row.geLimit === null ? '?' : formatInt(p.row.geLimit)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        <p class="small muted foot">
          Qty is the buy offer: your share of the budget, capped by the GE limit (bought at most once per 4 hours). Est.
          profit/trip assumes one round trip of at most 10% of an hour’s volume.
        </p>
      {/if}
    </section>

    <section class="card" aria-labelledby="risk-h">
      <h2 id="risk-h">Risks</h2>
      <ul class="risks">
        <li><strong>Fills are estimates.</strong> Hourly averages can’t show who was first in line at a price. The backtest let one offer take at most 10% of an hour’s trades; real fills can be fewer.</li>
        <li><strong>Losses come from the forced exit.</strong> A sale at the Sell at price always clears the margin; what is still unsold by {day ? utc(day.exitStart) : '23:00'} UTC is dumped at the instant-sell price, and that is where every losing trade in the backtest came from.</li>
        <li><strong>You start late.</strong> The collector counts the 23:00 UTC hour as final only after 01:00 UTC, so the first hour of the entry window is usually gone. The backtest’s bids were live from 00:00 UTC.</li>
        <li><strong>Prices move.</strong> Game updates and events shift markets within a day. The health checks skip items that fell over the last 3 days or that the guide price flags as crashing, not sudden moves.</li>
        <li><strong>Small sample.</strong> The test covers {info.testDays}. A run of losing days is possible even if the averages hold.</li>
      </ul>
    </section>
  {/if}
</main>

<style>
  .strategy {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow-y: auto;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .min0 {
    min-width: 0;
  }
  .title-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }
  h1 {
    font-size: 22px;
  }
  h2 {
    font-size: 16px;
    margin-bottom: 8px;
  }
  .desc {
    margin: 6px 0 0;
    max-width: 90ch;
    line-height: 1.5;
  }
  .name {
    color: var(--text);
  }
  .badge {
    padding: 2px 8px;
    border: 1px solid currentColor;
    border-radius: var(--radius);
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.3px;
    text-transform: uppercase;
  }
  .badge.ok {
    color: var(--profit);
  }
  .badge.warn {
    color: var(--tier-b);
  }
  .callout {
    padding: 12px 14px;
    border: 1px solid var(--tier-b);
    border-left-width: 4px;
    border-radius: var(--radius);
    background: var(--bg-2);
    line-height: 1.5;
  }
  .callout.validated {
    border-color: var(--profit);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 400px), 1fr));
    gap: 16px;
  }
  .card {
    padding: 16px;
    min-width: 0;
  }
  .small {
    font-size: 13px;
  }
  .strong {
    color: var(--text);
  }
  p {
    margin: 0 0 10px;
  }
  .timeline {
    position: relative;
    display: flex;
    height: 28px;
    margin-top: 12px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    overflow: hidden;
    font-size: 12px;
  }
  .seg {
    flex-basis: 0;
    display: flex;
    align-items: center;
    padding: 0 8px;
    white-space: nowrap;
    overflow: hidden;
  }
  .seg.entry {
    background: var(--chip-on);
    color: var(--accent-hi);
  }
  .seg.hold {
    background: var(--panel);
    color: var(--text-soft);
  }
  .seg.exit {
    background: #4a2418;
  }
  .now {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: var(--heading);
  }
  .ticks {
    display: grid;
    grid-template-columns: 12fr 11fr 1fr;
    margin-top: 4px;
    font-size: 11px;
    color: var(--muted);
  }
  .ticks span:last-child {
    justify-self: end;
  }
  .phase {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 8px;
    margin: 12px 0;
    color: var(--text);
  }
  .phase .dot {
    align-self: center;
  }
  .phase.over {
    color: var(--loss);
  }
  .steps {
    margin: 0;
    padding-left: 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    line-height: 1.45;
  }
  .when {
    display: block;
    font-size: 12px;
    color: var(--accent);
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th,
  td {
    padding: 7px 8px;
    text-align: right;
    white-space: nowrap;
    border-bottom: 1px solid var(--row-line);
    vertical-align: top;
  }
  thead th {
    font-size: 12px;
    font-weight: 700;
    color: var(--muted);
    border-bottom-color: var(--border-strong);
  }
  th:first-child,
  td:first-child {
    text-align: left;
  }
  tbody th {
    font-weight: 400;
    color: var(--text);
  }
  tr.near {
    background: var(--selected);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .ci {
    display: block;
    font-size: 11px;
    color: var(--muted);
  }
  .picks-head {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: baseline;
    gap: 4px 16px;
    margin-bottom: 8px;
  }
  .picks-head h2 {
    margin: 0;
  }
  .stale {
    padding: 8px 12px;
    border: 1px solid var(--loss);
    border-radius: var(--radius);
    color: var(--loss);
  }
  .pick-table td.item,
  .pick-table th.item {
    text-align: left;
    white-space: normal;
    width: 100%;
  }
  .item-btn {
    text-align: left;
    overflow-wrap: anywhere;
  }
  .rank {
    width: 2.5em;
  }
  .foot {
    margin: 10px 0 0;
    line-height: 1.5;
  }
  .risks {
    margin: 0;
    padding-left: 20px;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 360px), 1fr));
    gap: 8px 28px;
    line-height: 1.5;
  }
  @media (max-width: 900px) {
    .opt {
      display: none;
    }
  }
</style>

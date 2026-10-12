<script lang="ts">
  import { app } from '../lib/store.svelte.ts';
  import { formatInt } from '../lib/format.ts';
  import DetailPanel from './DetailPanel.svelte';
  import FilterRail from './FilterRail.svelte';
  import FlipTable from './FlipTable.svelte';

  const SORT_LABEL: Record<string, string> = {
    name: 'Item', tier: 'Tier', risk: 'Risk', buy: 'Buy', sell: 'Sell', unitProfit: 'Profit/ea', roi: 'ROI',
    qty: 'Qty', avgVolume: 'Vol/day', profit: 'Total', score: 'Score', trend: 'Trend', liveMargin: 'Live margin',
    expDay: 'Exp/day',
  };
  const STEPS = ['Read 90 days of history', 'Building price stats', 'Scoring flips'];
  const SKELETON = [180, 240, 150, 210, 170, 260, 140, 200, 230, 160, 190, 220, 150, 210];
  const progress = $derived(Math.round((Math.max(app.scanStep, 1) - 0.5) * (100 / STEPS.length)));
</script>

<div class="desk">
  <FilterRail />

  <main class="main" aria-busy={app.scanning}>
    {#if app.scanning}
      <div class="progress">
        <div class="progress-head">
          <span class="progress-title">Scanning the market</span>
          <span class="mono muted small">step {Math.max(app.scanStep, 1)} of {STEPS.length}</span>
        </div>
        <div class="bar" role="progressbar" aria-label="Scan progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={progress}>
          <div class="fill" style:width="{progress}%"></div>
        </div>
        <ol class="steps">
          {#each STEPS as s, i (s)}
            {@const n = i + 1}
            <li class:done={n < app.scanStep} class:current={n === Math.max(app.scanStep, 1)}>
              {n < app.scanStep ? '✓' : n === Math.max(app.scanStep, 1) ? '●' : '○'} {s}
            </li>
          {/each}
        </ol>
      </div>
    {/if}

    {#if app.result}
      <div class="hintbar">
        <span>
          {formatInt(app.visible.length)} flips match · sorted by
          <span class="strong">{SORT_LABEL[app.sort.key]} {app.sort.desc ? '↓' : '↑'}</span>
        </span>
        <span>Click a header to sort · ↑ ↓ to move</span>
      </div>
      {#if app.visible.length > 0}
        <FlipTable />
      {:else}
        <div class="center">
          <h2>No items match your filters</h2>
          <p class="muted">Try adjusting your search or filter settings.</p>
        </div>
      {/if}
    {:else if app.scanning}
      <div class="skeleton" aria-hidden="true">
        {#each SKELETON as w, i (i)}
          <div class="sk-row">
            <div class="sk sk-star"></div>
            <div class="sk sk-name" style:width="{w}px"></div>
            <div class="sk sk-tier"></div>
            <div class="sk sk-70 push"></div>
            <div class="sk sk-70"></div>
            <div class="sk sk-60"></div>
            <div class="sk sk-50"></div>
          </div>
        {/each}
      </div>
    {:else if app.scanError}
      <div class="center">
        <h2>Scan failed</h2>
        <p class="loss">{app.scanError}</p>
        <button class="btn btn-primary btn-tall" onclick={() => app.scan()}>Retry</button>
      </div>
    {:else}
      <div class="center">
        <h2>Welcome to the Grand Exchange Analyzer</h2>
        <p class="muted">Click “Scan market” to begin analysing flips.</p>
      </div>
    {/if}
  </main>

  <DetailPanel />
</div>

<style>
  .desk {
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    position: relative;
    overflow: hidden;
  }
  .progress {
    padding: 18px 20px;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .progress-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
  }
  .progress-title {
    font-family: var(--font-title);
    font-weight: 700;
    font-size: 15px;
    color: var(--heading);
  }
  .small {
    font-size: 12px;
  }
  .bar {
    height: 8px;
    border-radius: 4px;
    background: var(--panel);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    box-shadow: inset 0 1px 0 var(--accent-top);
    transition: width 0.25s ease;
  }
  .steps {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: 20px;
    font-size: 13px;
    color: var(--muted);
  }
  .steps .done {
    color: var(--profit);
  }
  .steps .current {
    color: var(--text);
  }
  .hintbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
    color: var(--muted);
  }
  .strong {
    color: var(--text);
  }
  .center {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    text-align: center;
    padding: 24px;
  }
  .center h2 {
    font-size: 20px;
  }
  .center p {
    margin: 0;
  }
  .skeleton {
    padding: 8px 16px;
    display: flex;
    flex-direction: column;
  }
  .sk-row {
    display: flex;
    gap: 14px;
    align-items: center;
    height: 38px;
    border-bottom: 1px solid var(--row-line);
  }
  .sk {
    height: 12px;
    border-radius: 2px;
    background: var(--panel);
  }
  .sk-star {
    width: 18px;
  }
  .sk-name {
    background: var(--skeleton);
  }
  .sk-tier {
    width: 26px;
    height: 14px;
    border-radius: 3px;
    background: var(--selected);
  }
  .sk-70 {
    width: 70px;
  }
  .sk-60 {
    width: 60px;
  }
  .sk-50 {
    width: 50px;
  }
  .push {
    margin-left: auto;
  }
</style>

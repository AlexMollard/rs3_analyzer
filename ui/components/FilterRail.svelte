<script lang="ts">
  import { app } from '../lib/store.svelte.ts';
  import { MIN_BUDGET } from '../lib/flips.ts';
  import { formatGp, formatInt, parseGp, TIER_LABEL } from '../lib/format.ts';
  import type { Tier } from '../lib/types.ts';

  const TIERS: (Tier | null)[] = [null, 'S', 'A', 'B', 'C', 'X'];

  let budgetText = $state(formatInt(app.budget));
  let budgetInvalid = $state(false);

  function onBudgetInput() {
    const v = parseGp(budgetText);
    budgetInvalid = v === null;
    if (v !== null) app.budget = Math.max(v, MIN_BUDGET);
  }

  function onBudgetBlur() {
    budgetInvalid = false;
    budgetText = formatInt(app.budget);
  }

  function onVolumeInput(e: Event) {
    const v = Number((e.currentTarget as HTMLInputElement).value);
    app.filters.minVolume = Number.isFinite(v) && v > 0 ? v : 0;
  }

  const locked = $derived(!app.result || app.scanning);
</script>

<aside class="rail" aria-label="Filters">
  <h2 class="heading">Filters</h2>
  <fieldset disabled={locked} class:dim={locked}>
    <legend class="sr-only">Flip filters</legend>

    <label class="field">
      Budget (gp)
      <input
        class="input mono"
        type="text"
        inputmode="decimal"
        aria-invalid={budgetInvalid}
        aria-describedby="budget-hint"
        bind:value={budgetText}
        oninput={onBudgetInput}
        onblur={onBudgetBlur}
      />
      <span id="budget-hint" class="hint" class:loss={budgetInvalid}>
        {budgetInvalid ? 'Enter an amount like 50m or 50,000,000' : `Min ${formatGp(MIN_BUDGET)} · accepts k / m / b`}
      </span>
    </label>

    <div class="field" role="group" aria-labelledby="tier-label">
      <span id="tier-label">Tier</span>
      <div class="chips">
        {#each TIERS as t (t ?? 'all')}
          <button
            class="chip"
            class:crash={t === 'X'}
            aria-pressed={app.filters.tier === t}
            onclick={() => (app.filters.tier = t)}>{t === null ? 'All' : TIER_LABEL[t]}</button
          >
        {/each}
      </div>
    </div>

    <label class="field">
      Min profit / flip
      <input type="range" min="0" max="10000000" step="100000" bind:value={app.filters.minProfit} />
      <span class="hint mono">{app.filters.minProfit > 0 ? `≥ ${formatGp(app.filters.minProfit)}` : 'Off'}</span>
    </label>

    <label class="field">
      Min ROI
      <input type="range" min="0" max="100" step="1" bind:value={app.filters.minRoi} />
      <span class="hint mono">{app.filters.minRoi > 0 ? `≥ ${app.filters.minRoi.toFixed(1)}%` : 'Off'}</span>
    </label>

    <label class="field">
      Min daily volume
      <input class="input mono" type="number" min="0" step="100" value={app.filters.minVolume} oninput={onVolumeInput} />
    </label>

    <label class="check"><input type="checkbox" bind:checked={app.filters.favouritesOnly} /> Favourites only</label>
    <label class="check"><input type="checkbox" bind:checked={app.filters.showNegative} /> Include negative score</label>

    {#if app.scanning && !app.result}
      <p class="hint">Filters unlock when the scan finishes.</p>
    {/if}

    <button class="btn btn-ghost reset" onclick={() => app.resetFilters()}>Reset filters</button>
  </fieldset>
</aside>

<style>
  .rail {
    width: 230px;
    flex: none;
    border-right: 1px solid var(--border);
    background: var(--bg-1);
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 18px;
    overflow-y: auto;
    position: relative;
    min-height: 0;
  }
  .heading {
    font-size: 13px;
    letter-spacing: 1px;
    color: var(--text-soft);
  }
  fieldset {
    border: 0;
    margin: 0;
    padding: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 18px;
    flex: 1;
  }
  fieldset.dim {
    opacity: 0.55;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .hint {
    font-size: 12px;
    color: var(--muted);
    margin: 0;
  }
  .hint.loss {
    color: var(--loss);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    height: 30px;
    min-width: 36px;
    padding: 0 10px;
    border-radius: var(--radius);
    border: 1px solid var(--border-strong);
    background: transparent;
    color: var(--text-soft);
  }
  .chip:hover:not(:disabled) {
    border-color: var(--bevel);
    color: var(--text);
  }
  .chip[aria-pressed='true'] {
    border-color: var(--accent);
    background: var(--chip-on);
    color: var(--accent-hi);
  }
  .chip.crash[aria-pressed='true'] {
    border-color: var(--loss);
    background: #3a1f18;
    color: var(--loss);
  }
  input[type='range'],
  input[type='checkbox'] {
    accent-color: var(--accent);
  }
  input[type='checkbox'] {
    width: 16px;
    height: 16px;
    margin: 0;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .reset {
    margin-top: auto;
  }
</style>

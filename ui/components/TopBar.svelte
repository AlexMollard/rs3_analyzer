<script lang="ts">
  import { app } from '../lib/store.svelte.ts';
  import { formatGp, formatInt } from '../lib/format.ts';
  import type { Tab } from '../lib/store.svelte.ts';

  const tabs: { id: Tab; label: () => string }[] = [
    { id: 'desk', label: () => 'Trading Desk' },
    { id: 'watch', label: () => `Watchlist (${app.favorites.size})` },
  ];

  function onTabKey(e: KeyboardEvent) {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    app.tab = app.tab === 'desk' ? 'watch' : 'desk';
    document.getElementById(`tab-${app.tab}`)?.focus();
  }
</script>

<header class="topbar">
  <div class="row">
    <div class="brand">
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M3 17l6-6 4 4 8-8" /><path d="M14 7h7v7" /></svg>
      <span class="title">GE Analyzer</span>
    </div>

    <label class="search">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="M20 20l-3.5-3.5" /></svg>
      <span class="sr-only">Search items</span>
      <input id="search" type="search" placeholder="Search items  ( / )" autocomplete="off" spellcheck="false" bind:value={app.search} />
    </label>

    <div class="readouts mono" aria-label="Snapshot summary">
      <span>Snapshot <span class="val">{app.result?.snapshotDate ?? '—'}</span></span>
      <span>Items <span class="val">{app.result ? formatInt(app.result.itemCount) : '—'}</span></span>
      <span>Budget <span class="val accent">{formatGp(app.budget)}</span></span>
    </div>

    {#if app.scanning}
      <button class="btn scanning" disabled>
        <svg class="spinner" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="M12 3a9 9 0 1 0 9 9" /></svg>
        Scanning…
      </button>
    {:else}
      <button class="btn btn-primary" onclick={() => app.scan()}>Scan market</button>
    {/if}
  </div>

  <div class="tabs" role="tablist" aria-label="Views">
    {#each tabs as t (t.id)}
      <button
        id="tab-{t.id}"
        class="tab"
        role="tab"
        aria-selected={app.tab === t.id}
        aria-controls="panel-{t.id}"
        tabindex={app.tab === t.id ? 0 : -1}
        onclick={() => (app.tab = t.id)}
        onkeydown={onTabKey}>{t.label()}</button
      >
    {/each}
  </div>
</header>

<style>
  .topbar {
    background: var(--bg-2);
    border-bottom: 2px solid var(--border-strong);
    box-shadow: inset 0 -1px 0 var(--bevel);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 16px;
    padding: 12px 20px;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--accent);
  }
  .title {
    font-family: var(--font-title);
    font-weight: 700;
    font-size: 18px;
    letter-spacing: 0.6px;
    color: var(--heading);
  }
  .search {
    flex: 1 1 280px;
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-0);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    padding: 0 10px;
    height: 36px;
    color: var(--muted);
  }
  .search:focus-within {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: 0;
    color: var(--text);
    outline: none;
  }
  .search input::placeholder {
    color: var(--muted);
  }
  .readouts {
    display: flex;
    gap: 20px;
    font-size: 12px;
    color: var(--muted);
  }
  .val {
    color: var(--text);
  }
  .val.accent {
    color: var(--accent);
  }
  .scanning {
    background: var(--selected);
    color: var(--text-soft);
    font-weight: 700;
  }
  .scanning .spinner {
    color: var(--accent);
  }
  .tabs {
    display: flex;
    gap: 4px;
    padding: 0 20px;
  }
  .tab {
    padding: 10px 16px;
    font-family: var(--font-title);
    font-weight: 700;
    font-size: 14px;
    letter-spacing: 0.4px;
    border: 1px solid transparent;
    border-bottom: 0;
    border-radius: var(--radius) var(--radius) 0 0;
    background: transparent;
    color: var(--muted);
  }
  .tab:hover {
    color: var(--text);
  }
  .tab[aria-selected='true'] {
    color: var(--heading);
    background: var(--bg-0);
    border-color: var(--border-strong);
    box-shadow: inset 0 2px 0 var(--accent);
    margin-bottom: -2px;
  }
</style>

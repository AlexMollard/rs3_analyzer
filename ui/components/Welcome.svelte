<script lang="ts">
  import { app } from '../lib/store.svelte.ts';
  import { ATTRIBUTION } from '../lib/format.ts';

  let { searched }: { searched: string[] } = $props();

  const SOURCES = [
    { title: 'Unpack a snapshot', body: 'Have rs3_market.db.gz? Unpack it next to the app.', cmd: '.\\decompress_db.ps1' },
    { title: 'Build full history', body: "Downloads every item's full price history. Run rs3_collector.py once first.", cmd: 'python backfill_history.py' },
    { title: 'Keep it current', body: 'Schedule once a day to add the latest prices and volumes.', cmd: 'python rs3_collector.py' },
  ];
</script>

<main class="welcome">
  <div class="inner">
    <div class="card hero">
      <div class="hero-head">
        <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><ellipse cx="12" cy="5" rx="8" ry="3" /><path d="M4 5v14c0 1.7 3.6 3 8 3s8-1.3 8-3V5" /><path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3" /></svg>
        <div>
          <h1>Market database not found</h1>
          <p class="muted">The analyzer needs <span class="mono strong">rs3_market.db</span> to score flips.</p>
        </div>
      </div>
      <div class="paths mono">
        <div>Looked in:</div>
        {#each searched as p (p)}<div>{p}</div>{/each}
      </div>
      <div>
        <button class="btn btn-primary btn-tall" disabled={app.scanning} onclick={() => app.scan()}>
          {app.scanning ? 'Checking…' : 'Retry'}
        </button>
      </div>
    </div>

    <h2>Get the data</h2>
    <div class="sources">
      {#each SOURCES as s (s.cmd)}
        <div class="source">
          <div class="source-title">{s.title}</div>
          <p class="muted">{s.body}</p>
          <code class="mono">{s.cmd}</code>
        </div>
      {/each}
    </div>
    <p class="muted small">{ATTRIBUTION}</p>
  </div>
</main>

<style>
  .welcome {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow-y: auto;
    position: relative;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: 56px 16px;
    font-size: 15px;
  }
  .inner {
    width: 100%;
    max-width: 760px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
  .hero {
    padding: 28px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .hero-head {
    display: flex;
    gap: 14px;
    align-items: center;
    color: var(--accent);
  }
  h1 {
    font-size: 24px;
  }
  p {
    margin: 2px 0 0;
  }
  .strong {
    color: var(--text);
  }
  .paths {
    font-size: 13px;
    color: var(--muted);
    background: var(--bg-0);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 12px;
    line-height: 1.7;
    overflow-wrap: anywhere;
  }
  h2 {
    margin-top: 8px;
    font-size: 16px;
    color: var(--text-soft);
  }
  .sources {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 12px;
  }
  .source {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .source p {
    font-size: 14px;
    line-height: 1.5;
    margin: 0;
    flex: 1;
  }
  .source-title {
    font-family: var(--font-title);
    font-weight: 700;
    color: var(--heading);
  }
  code {
    font-size: 12px;
    background: var(--bg-0);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 6px 8px;
    color: var(--text);
  }
  .small {
    font-size: 12px;
  }
</style>

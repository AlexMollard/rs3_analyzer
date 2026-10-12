<script lang="ts">
  import { app } from '../lib/store.svelte.ts';
  import { ATTRIBUTION, formatInt } from '../lib/format.ts';

  const skipped = $derived(app.result?.skippedRows ?? 0);
</script>

<footer class="status">
  <span class="msg" role={app.status.kind === 'error' ? 'alert' : 'status'}>
    <span class="dot {app.status.kind}" aria-hidden="true"></span>
    <span class:loss={app.status.kind === 'error'}>{app.status.text}</span>
  </span>
  {#if app.result}
    <span class:loss={skipped > 0}>{formatInt(skipped)} unreadable row{skipped === 1 ? '' : 's'} skipped</span>
  {/if}
  {#if app.favNote}
    <span role={app.favNote.error ? 'alert' : 'status'} class:loss={app.favNote.error}>{app.favNote.text}</span>
  {/if}
  <span class="attr">{ATTRIBUTION}</span>
</footer>

<style>
  .status {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 18px;
    padding: 6px 16px;
    border-top: 1px solid var(--border);
    background: var(--bg-2);
    font-size: 12px;
    color: var(--muted);
  }
  .msg {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .attr {
    margin-left: auto;
  }
</style>

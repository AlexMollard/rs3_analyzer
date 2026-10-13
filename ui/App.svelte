<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from './lib/store.svelte.ts';
  import Desk from './components/Desk.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import Strategy from './components/Strategy.svelte';
  import TopBar from './components/TopBar.svelte';
  import Watchlist from './components/Watchlist.svelte';
  import Welcome from './components/Welcome.svelte';

  function isTyping(el: EventTarget | null): boolean {
    if (!(el instanceof HTMLElement)) return false;
    return el.isContentEditable || el instanceof HTMLInputElement || el instanceof HTMLSelectElement || el instanceof HTMLTextAreaElement;
  }

  function onKey(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === '/' && !isTyping(e.target)) {
      e.preventDefault();
      document.getElementById('search')?.focus();
    } else if (e.key === 'Escape' && app.tab === 'desk' && app.selectedId !== null && !isTyping(e.target)) {
      app.select(null);
    } else if (
      (e.key === 'ArrowDown' || e.key === 'ArrowUp') &&
      app.tab === 'desk' &&
      (e.target === document.body || e.target === document.documentElement)
    ) {
      // Arrow keys move the table selection when nothing else has focus
      e.preventDefault();
      app.moveSelection(e.key === 'ArrowDown' ? 1 : -1);
    }
  }

  onMount(async () => {
    await app.init();
    // Dev-only deep links for screenshots: ?mock&select=<itemId>&tab=watch (or tab=strategy)
    if (import.meta.env.DEV) {
      const p = new URLSearchParams(location.search);
      const sel = Number(p.get('select'));
      if (sel) app.select(sel);
      const tab = p.get('tab');
      if (tab === 'watch' || tab === 'strategy') app.tab = tab;
    }
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="shell">
  <TopBar />
  <div id="panel-{app.tab}" class="body" role="tabpanel" aria-labelledby="tab-{app.tab}">
    {#if app.tab === 'watch'}
      <Watchlist />
    {:else if app.missingDb}
      <Welcome searched={app.missingDb} />
    {:else if app.tab === 'strategy'}
      <Strategy />
    {:else}
      <Desk />
    {/if}
  </div>
  <StatusBar />
</div>

<style>
  .shell {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
  }
  .shell > :global(header),
  .shell > :global(footer) {
    flex: none;
  }
  .body {
    flex: 1;
    min-height: 0;
    min-width: 0;
    display: flex;
    position: relative;
    overflow: hidden;
  }
</style>

<script lang="ts">
  import type { Tier } from '../lib/types.ts';

  let { tier }: { tier: Tier } = $props();
  const title = $derived(tier === 'X' ? 'Crash tier: net loss after GE tax' : `Tier ${tier}`);
</script>

<span class="badge tier-{tier}" {title}>
  <span aria-hidden="true">{tier === 'X' ? '✕' : tier}</span>
  <span class="sr-only">{title}</span>
</span>

<style>
  .badge {
    display: inline-block;
    min-width: 22px;
    text-align: center;
    padding: 2px 6px;
    border-radius: 3px;
    font-weight: 700;
    font-size: 12px;
    line-height: 1.3;
    color: var(--bg-0);
    font-family: var(--font-body);
  }
  .tier-S {
    background: var(--tier-s);
  }
  .tier-A {
    background: var(--tier-a);
  }
  .tier-B {
    background: var(--tier-b);
  }
  .tier-C {
    background: var(--tier-c);
  }
  .tier-X {
    background: var(--tier-x);
  }
</style>

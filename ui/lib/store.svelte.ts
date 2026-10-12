// App state (Svelte 5 runes). Components read and write `app` directly.
import { backend, errorText } from './api.ts';
import { DEFAULT_BUDGET, DEFAULT_FILTERS, DEFAULT_SORT, filterRows, priceRow, sortRows } from './flips.ts';
import type { Filters, Sort } from './flips.ts';
import { formatInt } from './format.ts';
import type { PricedRow, ScanError, ScanResult } from './types.ts';

export type Tab = 'desk' | 'watch';
export type StatusKind = 'ok' | 'busy' | 'error' | 'info';

class AppState {
  tab = $state<Tab>('desk');
  search = $state('');
  budget = $state(DEFAULT_BUDGET);
  filters = $state<Filters>({ ...DEFAULT_FILTERS });
  sort = $state<Sort>({ ...DEFAULT_SORT });

  // Large and replaced wholesale, so not deeply reactive
  result = $state.raw<ScanResult | null>(null);
  favorites = $state.raw<ReadonlySet<string>>(new Set());

  scanning = $state(false);
  scanStep = $state(0);
  missingDb = $state.raw<string[] | null>(null); // paths searched, when the DB is missing
  scanError = $state<string | null>(null);

  selectedId = $state<number | null>(null);

  status = $state<{ text: string; kind: StatusKind }>({ text: 'Ready', kind: 'info' });
  favNote = $state<{ text: string; error: boolean } | null>(null);

  priced: PricedRow[] = $derived(this.result ? this.result.rows.map((r) => priceRow(r, this.budget)) : []);
  visible: PricedRow[] = $derived(sortRows(filterRows(this.priced, this.filters, this.search, this.favorites), this.sort));
  // Looked up in all rows, so the panel stays open when the item is filtered out
  selected: PricedRow | null = $derived(this.priced.find((r) => r.itemId === this.selectedId) ?? null);

  async init() {
    try {
      const snap = await (await backend()).getFavorites();
      this.favorites = new Set(snap.names);
      if (snap.warning) this.favNote = { text: snap.warning, error: !snap.warning.startsWith('Imported') };
    } catch (e) {
      this.favNote = { text: `Could not load favourites: ${errorText(e)}`, error: true };
    }
    await this.scan();
  }

  async scan() {
    if (this.scanning) return;
    this.scanning = true;
    this.scanStep = 0;
    this.scanError = null;
    this.status = { text: 'Scanning rs3_market.db… you can keep using the window', kind: 'busy' };
    const started = performance.now();
    try {
      const result = await (await backend()).scan((step) => (this.scanStep = step));
      this.result = result;
      this.missingDb = null;
      // Drop the selection if the item disappeared from the new data
      if (this.selectedId !== null && !result.rows.some((r) => r.itemId === this.selectedId)) {
        this.selectedId = null;
      }
      const secs = ((performance.now() - started) / 1000).toFixed(1);
      const live = result.liveDate ? `live prices to ${result.liveDate}` : 'no recent real-time data';
      this.status = { text: `Loaded ${formatInt(result.itemCount)} items in ${secs} s, ${live}`, kind: 'ok' };
    } catch (e) {
      const err = e as Partial<ScanError>;
      if (Array.isArray(err?.searched) && err.searched.length > 0) {
        this.missingDb = err.searched;
        this.result = null;
        this.selectedId = null;
      } else {
        this.scanError = errorText(e);
      }
      this.status = { text: errorText(e), kind: 'error' };
    } finally {
      this.scanning = false;
    }
  }

  isFavorite(name: string): boolean {
    return this.favorites.has(name);
  }

  async toggleFavorite(name: string) {
    const on = !this.favorites.has(name);
    const api = await backend();
    try {
      this.favorites = new Set(await api.setFavorite(name, on));
      this.favNote = { text: 'Favourites saved', error: false };
    } catch (e) {
      // The change still applies for this session; resync with what the backend holds
      this.favNote = { text: errorText(e), error: true };
      try {
        this.favorites = new Set((await api.getFavorites()).names);
      } catch {
        // keep the current set
      }
    }
  }

  select(itemId: number | null) {
    this.selectedId = itemId;
  }

  /** Clicking the selected row again deselects it (egui behaviour). */
  toggleSelect(itemId: number) {
    this.selectedId = this.selectedId === itemId ? null : itemId;
  }

  /** Keyboard navigation over the visible (filtered + sorted) rows. */
  moveSelection(move: number | 'first' | 'last') {
    const rows = this.visible;
    if (rows.length === 0) return;
    const cur = rows.findIndex((r) => r.itemId === this.selectedId);
    let next: number;
    if (move === 'first') next = 0;
    else if (move === 'last') next = rows.length - 1;
    else if (cur < 0) next = move > 0 ? 0 : rows.length - 1;
    else next = Math.min(rows.length - 1, Math.max(0, cur + move));
    this.selectedId = rows[next].itemId;
  }

  openInDesk(itemId: number) {
    this.selectedId = itemId;
    this.tab = 'desk';
  }

  resetFilters() {
    // Search and favourites-only are left alone, as in the egui app
    this.filters = { ...DEFAULT_FILTERS, favouritesOnly: this.filters.favouritesOnly };
    this.sort = { ...DEFAULT_SORT };
  }

  setStatus(text: string, kind: StatusKind = 'info') {
    this.status = { text, kind };
  }
}

export const app = new AppState();

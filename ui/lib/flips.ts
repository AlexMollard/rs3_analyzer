// Budget, filter and sort logic (budget-dependent, so it runs here instead of rescanning the DB)
import type { FlipRow, PricedRow, Risk, Tier } from './types.ts';

export const MIN_BUDGET = 100_000;
export const DEFAULT_BUDGET = 50_000_000;
const I32_MAX = 2_147_483_647;

// Always-on sanity filters from the egui app
const MAX_ROI = 150;
const MIN_BUY = 100;

/** qty = affordable count (truncated), capped by the GE limit when it is known. */
export function priceRow(r: FlipRow, budget: number): PricedRow {
  let qty = 0;
  if (r.buy > 0) {
    const affordable = Math.min(Math.trunc(budget / r.buy), I32_MAX);
    qty = r.geLimit === null ? affordable : Math.min(affordable, r.geLimit);
  }
  return { ...r, qty, profit: Math.round(r.unitProfit * qty), totalCost: r.buy * qty };
}

export interface Filters {
  tier: Tier | null;
  minProfit: number;
  minRoi: number;
  minVolume: number;
  favouritesOnly: boolean;
  showNegative: boolean;
}

export const DEFAULT_FILTERS: Filters = {
  tier: null,
  minProfit: 0,
  minRoi: 0,
  minVolume: 500,
  favouritesOnly: false,
  showNegative: false,
};

export function filterRows(rows: PricedRow[], f: Filters, search: string, favs: ReadonlySet<string>): PricedRow[] {
  const q = search.trim().toLowerCase();
  return rows.filter(
    (r) =>
      r.qty > 0 &&
      r.avgVolume >= f.minVolume &&
      r.roi <= MAX_ROI &&
      r.buy >= MIN_BUY &&
      (!f.favouritesOnly || favs.has(r.name)) &&
      (q === '' || r.name.toLowerCase().includes(q)) &&
      (f.tier === null || r.tier === f.tier) &&
      (f.minProfit <= 0 || r.profit >= f.minProfit) &&
      (f.minRoi <= 0 || r.roi >= f.minRoi) &&
      (f.showNegative || r.score >= 0),
  );
}

export type SortKey =
  | 'name'
  | 'tier'
  | 'risk'
  | 'buy'
  | 'sell'
  | 'unitProfit'
  | 'roi'
  | 'qty'
  | 'avgVolume'
  | 'profit'
  | 'score'
  | 'trend';

export interface Sort {
  key: SortKey;
  desc: boolean;
}

export const DEFAULT_SORT: Sort = { key: 'score', desc: true };

const TIER_RANK: Record<Tier, number> = { X: 0, C: 1, B: 2, A: 3, S: 4 };
const RISK_RANK: Record<Risk, number> = { stable: 0, outliers: 1, spiked: 2, crashed: 3, crashing: 4 };

function sortValue(r: PricedRow, key: SortKey): number | string {
  if (key === 'tier') return TIER_RANK[r.tier];
  if (key === 'risk') return RISK_RANK[r.risk];
  return r[key];
}

export function sortRows(rows: PricedRow[], s: Sort): PricedRow[] {
  const dir = s.desc ? -1 : 1;
  return rows.slice().sort((a, b) => {
    const va = sortValue(a, s.key);
    const vb = sortValue(b, s.key);
    return va < vb ? -dir : va > vb ? dir : 0;
  });
}

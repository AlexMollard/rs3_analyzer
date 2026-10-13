// Budget, filter and sort logic (budget-dependent, so it runs here instead of rescanning the DB)
import type { FlipRow, PricedRow, Risk, StrategyQuote, Tier } from './types.ts';

export const MIN_BUDGET = 100_000;
export const DEFAULT_BUDGET = 50_000_000;
const I32_MAX = 2_147_483_647;

// Always-on sanity filters from the egui app
const MAX_ROI = 150;
const MIN_BUY = 100;

/** Share of a day's volume one order is assumed to fill (VOLUME_SHARE in src-tauri/src/trade.rs). */
export const VOLUME_SHARE = 0.1;

/** Affordable count (truncated), capped by the GE limit when it is known. */
function orderQty(buy: number, geLimit: number | null, budget: number): number {
  if (buy <= 0) return 0;
  const affordable = Math.min(Math.trunc(budget / buy), I32_MAX);
  return geLimit === null ? affordable : Math.min(affordable, geLimit);
}

/**
 * qty is the order at the guide buy price. liveQty is the order at the live buy price, also
 * capped at VOLUME_SHARE of the live volume, and expDay is its expected profit per day: the
 * backtest's order size and rank key for the live estimate (cases shared in price-cases.json).
 */
export function priceRow(r: FlipRow, budget: number): PricedRow {
  const qty = orderQty(r.buy, r.geLimit, budget);
  const liveQty = r.live ? Math.min(orderQty(r.live.buy, r.geLimit, budget), Math.floor(VOLUME_SHARE * r.live.volume)) : 0;
  const expDay = r.live?.unitDay == null ? null : r.live.unitDay * liveQty;
  return { ...r, qty, profit: Math.round(r.unitProfit * qty), totalCost: r.buy * qty, liveQty, expDay };
}

/**
 * One strategy pick's buy offer size (its share of the budget, capped by the GE limit) and
 * capacity: the offer also capped at VOLUME_SHARE of the hourly volume, the size of one round
 * trip (hourly::capacity in src-tauri/src/hourly.rs; cases shared in strategy-cases.json).
 */
export function strategySize(q: StrategyQuote, geLimit: number | null, alloc: number): { qty: number; capacity: number } {
  const qty = orderQty(q.buy, geLimit, alloc);
  return { qty, capacity: Math.min(qty, Math.floor(VOLUME_SHARE * q.liquidity)) };
}

export interface StrategyPick {
  row: FlipRow;
  quote: StrategyQuote;
  qty: number;
  capacity: number;
  tripProfit: number; // margin x capacity: one round trip's expected profit, the rank key
}

/**
 * The hourly strategy's day: the top `picks` items, each with budget / picks, by margin x capacity
 * (the item id breaks ties; no capacity, no pick), as the backtest trades them.
 */
export function strategyPicks(rows: FlipRow[], budget: number, picks: number): StrategyPick[] {
  const alloc = budget / picks;
  const out: StrategyPick[] = [];
  for (const row of rows) {
    if (!row.strategy) continue;
    const { qty, capacity } = strategySize(row.strategy, row.geLimit, alloc);
    if (capacity >= 1) out.push({ row, quote: row.strategy, qty, capacity, tripProfit: row.strategy.margin * capacity });
  }
  out.sort((a, b) => b.tripProfit - a.tripProfit || a.row.itemId - b.row.itemId);
  return out.slice(0, picks);
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
      // Tier X loses money after tax at the model's own prices: hide unless asked for
      (f.tier === 'X' || f.showNegative || (r.score >= 0 && r.tier !== 'X')),
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
  | 'trend'
  | 'liveMargin'
  | 'expDay';

export interface Sort {
  key: SortKey;
  desc: boolean;
}

export const DEFAULT_SORT: Sort = { key: 'score', desc: true };

const TIER_RANK: Record<Tier, number> = { X: 0, C: 1, B: 2, A: 3, S: 4 };
const RISK_RANK: Record<Risk, number> = { stable: 0, outliers: 1, spiked: 2, crashed: 3, crashing: 4 };

function sortValue(r: PricedRow, key: SortKey): number | string | null {
  if (key === 'tier') return TIER_RANK[r.tier];
  if (key === 'risk') return RISK_RANK[r.risk];
  if (key === 'liveMargin') return r.live?.margin ?? null;
  return r[key];
}

/** Rows without a value (no live data) go last in either direction. */
export function sortRows(rows: PricedRow[], s: Sort): PricedRow[] {
  const dir = s.desc ? -1 : 1;
  return rows.slice().sort((a, b) => {
    const va = sortValue(a, s.key);
    const vb = sortValue(b, s.key);
    if (va === null || vb === null) return va === vb ? 0 : va === null ? 1 : -1;
    return va < vb ? -dir : va > vb ? dir : 0;
  });
}

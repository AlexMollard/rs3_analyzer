// Runnable parity check: `node ui/lib/format.check.ts` (also part of `pnpm check`).
// The case tables are shared with Rust tests: format-gp-cases.json with src-tauri/src/format.rs,
// price-cases.json with src-tauri/src/backtest.rs, strategy-cases.json with src-tauri/src/hourly.rs.
import cases from './format-gp-cases.json' with { type: 'json' };
import priceCases from './price-cases.json' with { type: 'json' };
import strategyCases from './strategy-cases.json' with { type: 'json' };
import { copyText, formatGp, parseGp } from './format.ts';
import { priceRow, sortRows, strategyPicks, strategySize } from './flips.ts';
import type { FlipRow } from './types.ts';

const failures: string[] = [];
const expect = (what: string, got: unknown, want: unknown) => {
  if (got !== want) failures.push(`${what}: got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`);
};

for (const [value, want] of cases as [number, string][]) expect(`formatGp(${value})`, formatGp(value), want);

expect('parseGp("50m")', parseGp('50m'), 50_000_000);
expect('parseGp("250,000,000")', parseGp('250,000,000'), 250_000_000);
expect('parseGp("1.5b")', parseGp('1.5b'), 1_500_000_000);
expect('parseGp("abc")', parseGp('abc'), null);

const row: FlipRow = {
  itemId: 1, name: 'Coal', score: 10, tier: 'B', buy: 300, sell: 330, geLimit: 25_000, unitProfit: 24,
  roi: 8, avgVolume: 1234.5, trend: 0.5, tax: 6, risk: 'stable', outliers: 0, dataPoints: 90,
  band: [10, 90], notes: '', spark: [], live: null, strategy: null,
};
const capped = priceRow(row, 50_000_000);
expect('qty capped by GE limit', capped.qty, 25_000);
expect('total profit', capped.profit, 600_000);
const uncapped = priceRow({ ...row, geLimit: null }, 1_000);
expect('qty truncates budget / buy', uncapped.qty, 3);
expect('copy text qty note', copyText(uncapped).includes('Qty: 3 (GE limit ?)'), true);

expect('no live data: liveQty', capped.liveQty, 0);
expect('no live data: expDay', capped.expDay, null);
for (const c of priceCases) {
  const live = { buy: c.buy, sell: 0, margin: 0, volume: c.volume, unitDay: c.unitDay, samples: 0 };
  const p = priceRow({ ...row, geLimit: c.geLimit, live }, c.budget);
  expect(`liveQty for buy ${c.buy}, volume ${c.volume}`, p.liveQty, c.qty);
  expect(`expDay for buy ${c.buy}, volume ${c.volume}`, p.expDay, c.expDay);
}
const live = { buy: 290, sell: 340, margin: 44, volume: 900, unitDay: null, samples: 1 };
expect('no track record: expDay', priceRow({ ...row, live }, 50_000_000).expDay, null);
const withLive = priceRow({ ...row, itemId: 2, live: { ...live, unitDay: 5 } }, 50_000_000);
const sorted = (desc: boolean) => sortRows([capped, withLive], { key: 'expDay', desc }).map((r) => r.itemId).join();
expect('rows without expDay sort last, descending', sorted(true), '2,1');
expect('rows without expDay sort last, ascending', sorted(false), '2,1');
expect('copy text live line', copyText(withLive).includes('Live buy/sell: 290 / 340 (margin 44)'), true);

for (const c of strategyCases) {
  const size = strategySize({ buy: c.buy, sell: 0, margin: 0, liquidity: c.liquidity }, c.geLimit, c.budget / 10);
  expect(`strategy qty for buy ${c.buy}, budget ${c.budget}`, size.qty, c.qty);
  expect(`strategy capacity for buy ${c.buy}, budget ${c.budget}`, size.capacity, c.capacity);
}
// Rank by margin x capacity, the lower item id first on ties; no capacity, no pick
const quote = (margin: number, liquidity: number) => ({ buy: 1000, sell: 1100, margin, liquidity });
const ranked = [
  { ...row, itemId: 5, geLimit: null, strategy: quote(10, 1000) },  // 10 x 100
  { ...row, itemId: 3, geLimit: null, strategy: quote(20, 500) },  // 20 x 50, ties with 5
  { ...row, itemId: 4, geLimit: null, strategy: quote(1, 30_000) },  // 1 x 3000
  { ...row, itemId: 6, geLimit: null, strategy: quote(500, 5) },  // no capacity
  { ...row, itemId: 7, geLimit: null, strategy: null },
];
const order = (picks: number) => strategyPicks(ranked, 50_000_000, picks).map((p) => p.row.itemId).join();
expect('strategy picks order', order(10), '4,3,5');
expect('strategy picks keep the top ones', order(2), '4,3');

if (failures.length > 0) {
  throw new Error(`format check failed:\n${failures.join('\n')}`);
}
console.log(
  `format check passed (${cases.length} formatGp cases, ${priceCases.length} live price cases, ${strategyCases.length} strategy cases)`,
);

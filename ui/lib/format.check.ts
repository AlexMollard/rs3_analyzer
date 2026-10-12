// Runnable parity check: `node ui/lib/format.check.ts` (also part of `pnpm check`).
// The case table is shared with the Rust test in src-tauri/src/format.rs.
import cases from './format-gp-cases.json' with { type: 'json' };
import { copyText, formatGp, parseGp } from './format.ts';
import { priceRow } from './flips.ts';
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
  band: [10, 90], notes: '', spark: [],
};
const capped = priceRow(row, 50_000_000);
expect('qty capped by GE limit', capped.qty, 25_000);
expect('total profit', capped.profit, 600_000);
const uncapped = priceRow({ ...row, geLimit: null }, 1_000);
expect('qty truncates budget / buy', uncapped.qty, 3);
expect('copy text qty note', copyText(uncapped).includes('Qty: 3 (GE limit ?)'), true);

if (failures.length > 0) {
  throw new Error(`format check failed:\n${failures.join('\n')}`);
}
console.log(`format check passed (${cases.length} formatGp cases)`);

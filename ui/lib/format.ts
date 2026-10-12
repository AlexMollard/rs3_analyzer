// Number formatting ported from the egui app (format_gp in src-tauri/src/format.rs)
import type { PricedRow, Risk, Tier } from './types.ts';

/**
 * Like Number#toFixed, but rounds exact ties half-to-even, as Rust's `{:.N}` does
 * (toFixed rounds them up: 1.25 -> "1.3" where Rust gives "1.2").
 */
export function toFixedRust(x: number, digits: number): string {
  const exact = x.toFixed(100);
  const dot = exact.indexOf('.');
  if (/^50*$/.test(exact.slice(dot + 1 + digits))) {
    const kept = exact.slice(0, digits === 0 ? dot : dot + 1 + digits);
    if (Number(kept[kept.length - 1]) % 2 === 0) return kept;
  }
  return x.toFixed(digits);
}

export function formatGp(value: number): string {
  if (value < 0) {
    const s = formatGp(-value);
    return s === '0' ? s : `-${s}`;
  }
  if (value >= 1_000_000_000) return `${toFixedRust(value / 1_000_000_000, 2)}B`;
  if (value >= 1_000_000) return `${toFixedRust(value / 1_000_000, 2)}M`;
  if (value >= 1_000) return `${toFixedRust(value / 1_000, 1)}K`;
  return toFixedRust(value, 0);
}

export function formatVolume(v: number): string {
  if (v >= 1_000_000) return `${toFixedRust(v / 1_000_000, 1)}M`;
  if (v >= 1_000) return `${toFixedRust(v / 1_000, 1)}K`;
  return toFixedRust(v, 0);
}

export const ATTRIBUTION = 'Price data from the RuneScape Wiki and Weird Gloop, CC BY-NC-SA 3.0.';

export const formatInt = (n: number): string => n.toLocaleString('en-US');

export const formatPct = (n: number): string => `${toFixedRust(n, 1)}%`;

export interface TrendInfo {
  arrow: string;
  label: string;
  dir: -1 | 0 | 1;
}

/** Thresholds are % per daily sample (same as the egui app). */
export function trendInfo(trend: number): TrendInfo {
  if (trend > 1.0) return { arrow: '↑↑', label: 'Rising++', dir: 1 };
  if (trend > 0.2) return { arrow: '↑', label: 'Rising+', dir: 1 };
  if (trend < -1.0) return { arrow: '↓↓', label: 'Falling--', dir: -1 };
  if (trend < -0.2) return { arrow: '↓', label: 'Falling-', dir: -1 };
  return { arrow: '→', label: 'Stable', dir: 0 };
}

export const TIER_LABEL: Record<Tier, string> = {
  S: 'S',
  A: 'A',
  B: 'B',
  C: 'C',
  X: 'Loss',
};

/** Table label and colour class for the headline risk. */
export const RISK_LABEL: Record<Risk, { text: string; cls: string }> = {
  stable: { text: '● Stable', cls: 'r-stable' },
  outliers: { text: '⚠ Outliers', cls: 'r-outliers' },
  spiked: { text: '▲ Spiked', cls: 'r-spiked' },
  crashed: { text: '▼ Crashed', cls: 'r-crash' },
  crashing: { text: '▼▼ Crashing', cls: 'r-crash' },
};

export function qtyText(r: PricedRow): string {
  return r.geLimit === null ? `${formatInt(r.qty)}?` : formatInt(r.qty);
}

export function limitText(r: PricedRow): string {
  return r.geLimit === null ? 'GE limit: ? (unknown, qty not capped)' : `GE limit: ${formatInt(r.geLimit)}`;
}

/** Same layout as the egui copy button, plus the live quote when there is one. */
export function copyText(r: PricedRow): string {
  const limitNote = r.geLimit === null ? ' (GE limit ?)' : '';
  const live = r.live
    ? [
        `Live buy/sell: ${formatGp(r.live.buy)} / ${formatGp(r.live.sell)} (margin ${formatGp(r.live.margin)})`,
        `Exp/day: ${r.expDay === null ? '?' : formatGp(r.expDay)} (qty ${r.liveQty}, ${r.live.samples} past signals)`,
      ]
    : [];
  return [
    `${r.name}:`,
    `Score: ${r.score}`,
    `Tier: ${TIER_LABEL[r.tier]} ${trendInfo(r.trend).label}`,
    `Buy: ${formatGp(r.buy)}`,
    `Sell: ${formatGp(r.sell)}`,
    `Qty: ${r.qty}${limitNote}`,
    `Avg Vol: ${String(r.avgVolume)}`,
    `Total cost: ${formatGp(r.totalCost)}`,
    `Profit: ${formatGp(r.profit)}`,
    `ROI: ${toFixedRust(r.roi, 1)}%`,
    ...live,
  ].join('\n');
}

/** Accepts "50000000", "50,000,000", "50m", "1.5b", "750k". Returns null if unparseable. */
export function parseGp(text: string): number | null {
  const m = /^\s*([0-9][0-9,_]*(?:\.[0-9]+)?)\s*([kmb])?\s*$/i.exec(text);
  if (!m) return null;
  const n = Number(m[1].replace(/[,_]/g, ''));
  if (!Number.isFinite(n)) return null;
  const mult = { k: 1e3, m: 1e6, b: 1e9 }[(m[2] ?? '').toLowerCase() as 'k' | 'm' | 'b'] ?? 1;
  return Math.round(n * mult);
}

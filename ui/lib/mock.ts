// Dev-only fake backend (loaded via ?mock). Deterministic so screenshots are repeatable.
// ?mock=missing simulates a missing database.
import type { Backend, FlipRow, HistoryPoint, Risk, ScanError, Tier } from './types.ts';

const BASE_NAMES = [
  'Abyssal whip', 'Saradomin brew flask (6)', 'Super restore flask (6)', 'Dragon bones', 'Elder rune bar',
  'Prayer potion (4)', 'Magic logs', 'Onyx bolts (e)', 'Black dragonhide', 'Blue partyhat', 'Rune bar',
  'Coal', 'Yew logs', 'Frost dragon bones', 'Overload flask (6)', 'Super antifire flask (6)', 'Elder logs',
  'Necrite stone spirit', 'Phasmatite stone spirit', 'Banite ore', 'Luminite', 'Orichalcite ore',
  'Drakolith', 'Dark animica', 'Light animica', 'Grimy lantadyme', 'Grimy dwarf weed', 'Torstol',
  'Raw rocktail', 'Rocktail', 'Sailfish', 'Blue blubber jellyfish', 'Bond', 'Noxious scythe',
  'Seismic wand', 'Ascension crossbow', 'Elder rune platebody + 5', 'Dragon pickaxe', 'Crystal key',
  'Soul rune', 'Blood rune', 'Death rune', 'Wine of Zamorak', 'Snapdragon seed', 'Potato cactus',
];
const VARIANTS = ['', ' (noted)', ' fragment', ' shard', ' (broken)', ' token', ' bundle', ' pack'];
const TIERS: Tier[] = ['S', 'A', 'A', 'B', 'B', 'B', 'C', 'C', 'C', 'C', 'X'];
const RISKS: Risk[] = ['stable', 'stable', 'stable', 'stable', 'outliers', 'outliers', 'spiked', 'crashed', 'crashing'];

// mulberry32
function rng(seed: number) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = seed;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function priceSeries(itemId: number, days: number, base: number, trend: number): number[] {
  const rand = rng(itemId * 7919);
  const out: number[] = [];
  let p = base * (1 - (trend / 100) * days * 0.5);
  for (let i = 0; i < days; i++) {
    p *= 1 + trend / 100 + (rand() - 0.5) * 0.04;
    out.push(Math.max(1, Math.round(p)));
  }
  return out;
}

function makeRows(): FlipRow[] {
  const rand = rng(42);
  const rows: FlipRow[] = [];
  let id = 1;
  for (const variant of VARIANTS) {
    for (const base of BASE_NAMES) {
      for (let n = 0; n < 8; n++) {
        const name = n === 0 ? `${base}${variant}` : `${base}${variant} #${n + 1}`;
        const buy = Math.round(10 ** (2 + rand() * 5.5));
        const tier = TIERS[Math.floor(rand() * TIERS.length)];
        const margin = tier === 'X' ? -0.02 : tier === 'S' ? 0.4 : tier === 'A' ? 0.22 : tier === 'B' ? 0.1 : 0.04;
        const sell = Math.round(buy * (1 + margin + rand() * 0.05));
        const tax = name.startsWith('Bond') ? 0 : Math.floor(sell / 50);
        const unitProfit = sell - buy - tax;
        const trend = (rand() - 0.45) * 3;
        const risk = RISKS[Math.floor(rand() * RISKS.length)];
        const itemId = id++;
        // About a third of items have no recent two-sided real-time trades
        const liveBuy = Math.round(buy * (1 + (rand() - 0.3) * 0.08));
        const liveSell = Math.round(liveBuy * (1 + (rand() - 0.25) * 0.1));
        const samples = Math.floor(rand() * 22);
        const live =
          rand() < 0.35
            ? null
            : {
                buy: liveBuy,
                sell: liveSell,
                margin: liveSell - liveBuy - (name.startsWith('Bond') ? 0 : Math.floor(liveSell / 50)),
                volume: Math.round(10 ** (1 + rand() * 4)),
                unitDay: samples < 2 ? null : Math.round(liveBuy * (rand() - 0.5) * 0.03 * 100) / 100,
                samples,
              };
        rows.push({
          itemId,
          name,
          score: Math.round(rand() * 160 - 25),
          tier,
          buy,
          sell,
          geLimit: rand() < 0.1 ? null : [2, 10, 70, 100, 1000, 5000, 10000, 25000][Math.floor(rand() * 8)],
          unitProfit,
          roi: buy > 0 ? (unitProfit / buy) * 100 : 0,
          avgVolume: Math.round(10 ** (2 + rand() * 4) * 10) / 10,
          trend,
          tax,
          risk,
          outliers: risk === 'outliers' ? 1 + Math.floor(rand() * 4) : 0,
          dataPoints: 90,
          band: risk === 'crashed' || risk === 'spiked' ? [15, 85] : [10, 90],
          notes: `Vol:${Math.round(margin * 100 + 5)}% | Spread:${sell - buy}gp | Q5-Q95:${Math.round(buy * 0.95)}-${Math.round(sell * 1.05)} | Data:90pts`,
          spark: priceSeries(itemId, 30, (buy + sell) / 2, trend),
          live,
        });
      }
    }
  }
  return rows;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
let rows: FlipRow[] | undefined;
const favorites = new Map<string, boolean>([
  ['Abyssal whip', true],
  ['Dragon bones', true],
  ['Saradomin brew flask (6)', true],
  ['Onyx bolts (e)', true],
]);
const favNames = () => [...favorites].filter(([, on]) => on).map(([n]) => n).sort();

export const mockBackend: Backend = {
  async scan(onProgress) {
    for (const step of [1, 2, 3]) {
      onProgress(step);
      await sleep(250);
    }
    if (new URLSearchParams(location.search).get('mock') === 'missing') {
      const err: ScanError = {
        message: 'rs3_market.db not found — run decompress_db.ps1 or backfill_history.py',
        searched: ['D:\\rs3_analyzer\\rs3_market.db', 'D:\\rs3_analyzer\\src-tauri\\target\\debug\\rs3_market.db'],
      };
      throw err;
    }
    rows ??= makeRows();
    return { rows, skippedRows: 0, snapshotDate: '2026-10-12', liveDate: '2026-10-11', itemCount: rows.length, dbPath: 'mock' };
  },
  async itemHistory(itemId) {
    await sleep(120);
    const row = rows?.find((r) => r.itemId === itemId);
    if (!row) return [];
    const prices = priceSeries(itemId, 365, (row.buy + row.sell) / 2, row.trend / 4);
    const end = Date.UTC(2026, 9, 12);
    return prices.map<HistoryPoint>((price, i) => ({
      date: new Date(end - (prices.length - 1 - i) * 86_400_000).toISOString().slice(0, 10),
      price,
      volume: Math.round(row.avgVolume * (0.6 + ((i * 37) % 80) / 100)),
    }));
  },
  async getFavorites() {
    return { names: favNames(), warning: null };
  },
  async setFavorite(name, on) {
    favorites.set(name, on);
    return favNames();
  },
};

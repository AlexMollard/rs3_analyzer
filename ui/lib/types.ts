// Mirrors the serde types in src-tauri/src/commands.rs and lib.rs (camelCase)

export type Tier = 'S' | 'A' | 'B' | 'C' | 'X';
export type Risk = 'stable' | 'outliers' | 'spiked' | 'crashed' | 'crashing';

export interface FlipRow {
  itemId: number;
  name: string;
  score: number;
  tier: Tier;
  buy: number;
  sell: number;
  geLimit: number | null; // null = unknown, qty not capped
  unitProfit: number; // net per item after tax
  roi: number;
  avgVolume: number;
  trend: number; // % change per daily sample
  tax: number;
  risk: Risk;
  outliers: number;
  dataPoints: number;
  band: [number, number];
  notes: string;
  spark: number[];
  live: LiveQuote | null; // null without fresh real-time data for the item
}

/** Real-time quote and its track record; informational, buy/sell/score stay on guide prices. */
export interface LiveQuote {
  buy: number; // instant-sell average: what a buy offer pays
  sell: number; // instant-buy average: what a sell offer receives
  margin: number; // per item after tax
  volume: number; // median daily units on the thinner side; caps liveQty
  unitDay: number | null; // realised profit/day of one unit at past live quotes, minus one standard error
  samples: number; // past signals behind unitDay
}

export interface ScanResult {
  rows: FlipRow[];
  skippedRows: number;
  snapshotDate: string | null;
  liveDate: string | null; // real-time day the live quotes are as of; null when missing or stale
  itemCount: number;
  dbPath: string;
}

/** `searched` is non-empty only when the database is missing. */
export interface ScanError {
  message: string;
  searched: string[];
}

export interface HistoryPoint {
  date: string;
  price: number;
  volume: number;
}

export interface FavoritesSnapshot {
  names: string[];
  warning: string | null;
}

/** A row with the budget-dependent fields filled in. */
export interface PricedRow extends FlipRow {
  qty: number;
  totalCost: number;
  profit: number; // total for qty
  liveQty: number; // at the live buy price, also capped by the live volume (0 without live data)
  expDay: number | null; // live.unitDay * liveQty
}

export interface Backend {
  scan(onProgress: (step: number) => void): Promise<ScanResult>;
  itemHistory(itemId: number): Promise<HistoryPoint[]>;
  getFavorites(): Promise<FavoritesSnapshot>;
  setFavorite(name: string, on: boolean): Promise<string[]>;
}

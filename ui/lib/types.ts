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
}

export interface ScanResult {
  rows: FlipRow[];
  skippedRows: number;
  snapshotDate: string | null;
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
}

export interface Backend {
  scan(onProgress: (step: number) => void): Promise<ScanResult>;
  itemHistory(itemId: number): Promise<HistoryPoint[]>;
  getFavorites(): Promise<FavoritesSnapshot>;
  setFavorite(name: string, on: boolean): Promise<string[]>;
}

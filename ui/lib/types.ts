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
  strategy: StrategyQuote | null; // null when the hourly strategy would not trade the item
}

/** The hourly strategy's offers for an item; the UI ranks them (strategyPicks in flips.ts). */
export interface StrategyQuote {
  buy: number; // bid
  sell: number; // ask
  margin: number; // per item after tax
  liquidity: number; // median units per hour on the thinner side
}

/** One held-out test result of the strategy, top `picks` at one budget. */
export interface StrategyResult {
  budget: number;
  trips: number;
  winRate: number; // profitable trips / trips, 0-1
  winCi: [number, number]; // 95% block-bootstrap interval
  profitDay: number; // mean realised gp per signal day
  profitCi: [number, number];
}

/** What the backend says about the hourly strategy (StrategyInfo in commands.rs). */
export interface StrategyInfo {
  name: string;
  description: string;
  validated: boolean; // met the bar on the held-out test
  signalDate: string | null; // UTC day the quotes are for; null without recent hourly data
  dataThrough: string | null; // newest stored bucket, 'yyyy-mm-dd hh:00' UTC
  signalHour: number; // UTC hour the signal is taken
  entryHours: number; // bids can fill in the buckets this many hours after the signal
  horizonHours: number; // sales run to this many hours after it; the last bucket takes the forced exit
  picks: number; // items traded per day, each with budget / picks
  testDays: string; // held-out test period behind `results`
  results: StrategyResult[];
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
  strategy: StrategyInfo;
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

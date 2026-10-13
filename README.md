# RS3 Grand Exchange Analyzer

A desktop application for analyzing RuneScape 3 Grand Exchange market data to identify profitable flipping opportunities.

## Features

- **Advanced Flip Analysis**: Multi-factor scoring system that evaluates ROI, trading volume, profit margins, price trends, and data reliability
- **Smart Filtering**: Automatically filters out unrealistic flips with low volume, extreme ROI, or suspicious pricing
- **Tier System**: Items ranked S, A, B or C by profitability, plus a Crash tier for flips that lose money after tax
- **Risk Detection**: Flags items that are crashing, have crashed, have spiked, or had outliers removed
- **Price Trends**: Trend indicators showing Rising++, Rising+, Stable, Falling-, and Falling-- price movements
- **Trading Desk**: Sortable, virtualized flip table with a detail panel (interactive price chart, buy/sell/tax/trend, risk notes, copy-to-clipboard summary)
- **Live margins**: Real-time instant-buy and instant-sell averages from the RuneScape Wiki for each item, and an expected profit per day from the item's recent record at those prices, shown next to the guide-price scoring (which stays the default ranking; see [docs/backtest.md](docs/backtest.md))
- **Watchlist**: Favourites shown as cards with sparklines and total profit at your budget
- **Hourly Strategy**: A separate tab with the day's top 10 picks of a pre-registered hourly flip strategy: buy and sell targets at your budget, the schedule in UTC and your local time, its held-out backtest results and the risks. It passed its held-out test (86% of trades profitable, about 0.94M gp a day at 50M over 19 days), but that is a short sample with estimated fills, and the Score ranking stays the default (see [docs/strategy.md](docs/strategy.md))
- **Customizable Filters**: Filter by tier, minimum profit, ROI, daily volume, budget, and search terms
- **Neutral stone UI**: Dark RuneScape-inspired theme, bundled fonts, keyboard navigation

## Requirements

- Rust 1.90 or later (the crate uses the 2024 edition; Tauri 2.12 needs 1.90)
- Node.js 22 and pnpm 10 (for the frontend)
- Microsoft Edge WebView2 Runtime (preinstalled on Windows 10/11)
- Python 3 with `requests` (only for collecting/rebuilding the database)

The app is built with [Tauri 2](https://v2.tauri.app/): a Svelte 5 + TypeScript frontend (Vite, uPlot for charts) over a Rust core that reads the data with `rusqlite` (bundled SQLite).

### Project layout

- `src-tauri/` - Rust: Tauri commands (`lib.rs`, `commands.rs`), favourites store (`favorites.rs`), the analysis core (`loader.rs`, `stats.rs`, `flips.rs`, `model.rs`, `trade.rs`, and the hourly strategy rules in `hourly.rs`) and the backtests (`backtest.rs`, `bin/backtest.rs`)
- `ui/` - Svelte frontend (`App.svelte`, `components/`, `lib/`)
- `index.html`, `vite.config.ts`, `package.json` - frontend build

## Installation

1. Clone the repository:
```bash
git clone https://github.com/AlexMollard/rs3_analyzer.git
cd rs3_analyzer
```

2. Get a database (see [Database](#database)). The database is **not stored in git**.

3. Install the frontend dependencies and run the app in development mode:
```bash
pnpm install
pnpm tauri dev
```

4. Build a release executable (and installers):
```bash
pnpm tauri build
```
The executable is written to `src-tauri/target/release/`. Use `pnpm tauri build --no-bundle` to skip the installers.

### Checks

Build the frontend first: the Rust crate embeds `dist/` at compile time, so `cargo` fails on a fresh clone until it exists.

```bash
pnpm install
pnpm build    # production frontend build (creates dist/)
pnpm check    # svelte-check + the gp formatting and live price parity checks
cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings   # also lints the backtest binary
```

### Backtest

`docs/backtest.md` describes the walk-forward backtest of the flip scoring and its results. The harness is a separate binary behind the `backtest` feature, so `pnpm tauri build` does not bundle it:

```bash
cd src-tauri
cargo run --release --features backtest --bin backtest -- --help
cargo run --release --features backtest --bin backtest -- --db ../rs3_market.db     # prints the markdown report
cargo run --release --features backtest --bin backtest -- --db ../rs3_market.db --hourly   # hourly strategy study
```

`docs/strategy.md` describes the hourly strategy search on `rt_hourly` and how to re-run it (its plan, fixed before it was run, is `docs/strategy-plan.md`). Release builds look for the database only in the current directory and next to the executable, hence `--db` when running from `src-tauri`.

## Database

The application expects a SQLite database named `rs3_market.db`. It looks in the current directory first, then next to the executable (development builds also check the repository root, since `pnpm tauri dev` runs from `src-tauri/`). The database is opened read-only. If it is missing, the app lists the paths it searched and how to get the data. It is not stored in git (it is large and gitignored), so you either rebuild it locally or receive a snapshot from someone who has one.

### Schema

Created by `rs3_collector.py` (`backfill_history.py` uses the same code):

```sql
CREATE TABLE items (
    id INTEGER PRIMARY KEY,
    name TEXT,
    ge_limit INTEGER          -- NULL when the buy limit is unknown
);

CREATE TABLE history (
    item_id INTEGER,
    record_date TEXT,         -- 'YYYY-MM-DD', UTC
    price INTEGER,
    volume INTEGER,           -- NULL when unknown
    PRIMARY KEY (item_id, record_date)
);

CREATE TABLE rt_daily (           -- real-time trades, one row per item per UTC day
    item_id INTEGER,
    record_date TEXT,         -- 'YYYY-MM-DD', UTC
    avg_high REAL,            -- volume-weighted instant-buy price, NULL if none that day
    avg_low REAL,             -- volume-weighted instant-sell price, NULL if none that day
    high_volume INTEGER,
    low_volume INTEGER,
    PRIMARY KEY (item_id, record_date)
);

CREATE TABLE rt_hourly (          -- real-time trades, one row per item per hour it traded
    item_id INTEGER,
    ts INTEGER,               -- hour start, unix seconds UTC
    avg_high REAL,            -- average instant-buy price, NULL if none that hour
    avg_low REAL,             -- average instant-sell price, NULL if none that hour
    high_volume INTEGER,
    low_volume INTEGER,
    PRIMARY KEY (item_id, ts)
) WITHOUT ROWID;              -- clustered by item, about 40% smaller than a rowid table

CREATE INDEX idx_history_record_date ON history(record_date);
CREATE INDEX idx_items_name ON items(name);
CREATE INDEX idx_rt_daily_record_date ON rt_daily(record_date);
CREATE INDEX idx_rt_hourly_ts ON rt_hourly(ts);
```

`rt_daily` is folded from `rt_hourly`: each side's price is the volume-weighted average of its hours.

### Building the database with the Python tooling

```bash
pip install -r requirements.txt
```

- `rs3_collector.py` - downloads today's Grand Exchange snapshot and stores one row per item for today's UTC date. Run it daily (for example from Task Scheduler). If the snapshot is unavailable it falls back to the batched latest-price API for items already in the database. It creates `rs3_market.db` if missing, retries transient failures with backoff, appends to `collector_log.txt`, and exits non-zero on failure. It then stores the real-time hours it is missing in `rt_hourly`: one bulk `/1h` request to the RuneScape Wiki real-time API per hour, for at most the last 7 days plus today, up to the last hour that closed at least an hour ago. Each missing day whose 24 hours are all stored is then folded into `rt_daily`. A real-time failure is logged as a warning and does not fail the run.
- `backfill_rt.py` - fills `rt_hourly` from the first hour of real-time data (2026-07-23 17:00 UTC) to the last settled hour, one bulk `/1h` request per hour with at least a second between requests (about 2,000 requests from scratch, roughly an hour). Each hour is written in one transaction and stored hours are skipped, so it is safe to stop and re-run. It then folds every missing complete day (from 2026-07-24) into `rt_daily`, and lists any hour or day that failed.
- `backfill_history.py` - fetches price history for every item already in the `items` table and inserts the missing days, keeping only the last 365 days (`HISTORY_DAYS`), the longest window the app uses. Run `rs3_collector.py` once first so `items` is populated, then run the backfill. `python backfill_history.py --prune` deletes rows older than that window from an existing database and compacts it (keep a copy first if you want the old history). Requests are rate limited (shared across workers), transient errors are retried, and any item ids that still fail are printed with a non-zero exit code. It is safe to re-run.

The scripts resolve `rs3_market.db` and `collector_log.txt` relative to the script directory, not the current directory. Daily guide prices come from the Weird Gloop Grand Exchange APIs; real-time prices come from the [RuneScape Wiki real-time API](https://runescape.wiki/w/RuneScape:Real-time_Prices) (prices.runescape.wiki), which asks for a descriptive User-Agent and bulk requests instead of one per item.

### Sharing a snapshot (outside GitHub)

Two PowerShell scripts (Windows PowerShell 5.1 or PowerShell 7) pack and unpack a gzip snapshot, `rs3_market.db.gz`, which is also gitignored. Share that file through whatever storage you like, but not through GitHub.

```powershell
.\compress_db.ps1             # rs3_market.db -> rs3_market.db.gz (prints size and SHA256)
.\decompress_db.ps1           # rs3_market.db.gz -> rs3_market.db
.\decompress_db.ps1 -Force    # same, overwriting an existing rs3_market.db
```

Both write to a temporary file and only replace the target when the operation succeeds. `decompress_db.ps1` refuses to overwrite an existing `rs3_market.db` unless `-Force` is given. Compare the printed SHA256 against the sender's to confirm the transfer.

## Usage

1. **Scan Market**: The app scans on startup; click "Scan market" to rescan after the database changes
2. **Set Budget**: Type your budget in the filter rail (`50m`, `1.5b` and `50,000,000` all work). Quantities and totals update instantly without rescanning
3. **Filter Results**: Use the filter rail for tier, minimum profit, ROI and daily volume; press `/` to jump to search
4. **Sort Data**: Click a column header to sort; click again to reverse
5. **Inspect an Item**: Click a row (or use the arrow keys) to open the detail panel with the price chart: scroll to zoom, drag to pan
6. **Check Live Prices**: The Live margin and Exp/day columns and tiles use the last 3 days of real-time trades, and are filled in while the newest real-time day in the database is at most 3 days old. Exp/day is the item's recent record at live prices times the live quantity at your budget (also capped at 10% of the live daily volume); the backtest found it overstates profit, so treat it as a comparison between items rather than a forecast
7. **Mark Favourites**: Click a star. Favourites appear on the Watchlist tab and are stored in `favorites.json` in the app's config directory (`%APPDATA%\com.alexmollard.rs3analyzer` on Windows). A `favorites.json` from an older build in the current or executable directory is imported on first run
8. **Copy Details**: Click "Copy flip summary" in the detail panel
9. **Try the Hourly Strategy**: The Strategy tab lists the day's top 10 picks at your budget (each gets a tenth of it) with Buy at and Sell at prices, the day's schedule in UTC and your local time, the held-out backtest results and the risks. A day's targets need the real-time hours through 23:00 UTC the day before, which the collector stores from 01:00 UTC; hourly data more than 3 days old shows no picks

## Filtering Logic

The analyzer automatically filters out:
- Items below the minimum average daily volume (500 by default, adjustable in the filter rail)
- Items with ROI exceeding 150% (likely data anomalies)
- Items with buy prices below 100gp (vendor trash/data errors)

## Grand Exchange Tax

RS3 charges a 2% tax per item sold, rounded down, with no cap; bonds are exempt. The analyzer applies the same rule per item when computing profit.

## Scoring Algorithm

The scoring system evaluates:
- **ROI Score**: Return on investment (weighted 2x)
- **Volume Score**: Trading volume (logarithmic scale)
- **Profit Score**: Absolute profit per item
- **Volatility Score**: Price volatility (higher = better flip potential)
- **Reliability Score**: Amount of historical data available
- **Spread Penalty**: Penalizes tight spreads (<2%)
- **Trend Score**: Bonus for rising prices, penalty for falling

The score is a heuristic. The walk-forward backtest in [docs/backtest.md](docs/backtest.md) found that its top picks rarely fill and do not make a reliable profit. None of the alternatives tested did better at a 50M budget, though, so the score stays the default sort, and the live columns are there for information.

## Data attribution

Price data from the RuneScape Wiki and Weird Gloop, CC BY-NC-SA 3.0.

## License

MIT

## Disclaimer

This tool is for educational purposes. Always verify market data before making trades.

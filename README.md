# RS3 Grand Exchange Analyzer

A desktop application for analyzing RuneScape 3 Grand Exchange market data to identify profitable flipping opportunities.

## Features

- **Advanced Flip Analysis**: Multi-factor scoring system that evaluates ROI, trading volume, profit margins, price trends, and data reliability
- **Smart Filtering**: Automatically filters out unrealistic flips with low volume, extreme ROI, or suspicious pricing
- **Tier System**: Items categorized as Diamond, Gold, Good, Normal, or Crash based on profitability
- **Price Trends**: Real-time trend indicators showing Rising++, Rising+, Stable, Falling-, and Falling-- price movements
- **Persistent Favorites**: Save your favorite flips across sessions
- **Customizable Filters**: Filter by tier, minimum profit, ROI, budget, and search terms
- **Modern RS3 UI**: Dark brown/gold theme inspired by the Grand Exchange interface

## Requirements

- Rust 1.85 or later (the crate uses the 2024 edition)
- Python 3 with `requests` (only for collecting/rebuilding the database)
- Windows (for Segoe UI font support)

The UI is built with egui/eframe 0.36 and reads the data with `rusqlite` (bundled SQLite).

## Installation

1. Clone the repository:
```bash
git clone https://github.com/AlexMollard/rs3_analyzer.git
cd rs3_analyzer
```

2. Get a database (see [Database](#database)). The database is **not stored in git**.

3. Build and run:
```bash
cargo run --release
```

## Database

The application expects a SQLite database named `rs3_market.db` in the project directory. It is not stored in git (it is large and gitignored), so you either rebuild it locally or receive a snapshot from someone who has one.

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

CREATE INDEX idx_history_record_date ON history(record_date);
CREATE INDEX idx_items_name ON items(name);
```

### Building the database with the Python tooling

```bash
pip install -r requirements.txt
```

- `rs3_collector.py` - downloads today's Grand Exchange snapshot and stores one row per item for today's UTC date. Run it daily (for example from Task Scheduler). If the snapshot is unavailable it falls back to the batched latest-price API for items already in the database. It creates `rs3_market.db` if missing, retries transient failures with backoff, appends to `collector_log.txt`, and exits non-zero on failure.
- `backfill_history.py` - fetches price history for every item already in the `items` table and inserts the missing days, keeping only the last 365 days (`HISTORY_DAYS`), the longest window the app uses. Run `rs3_collector.py` once first so `items` is populated, then run the backfill. `python backfill_history.py --prune` deletes rows older than that window from an existing database and compacts it (keep a copy first if you want the old history). Requests are rate limited (shared across workers), transient errors are retried, and any item ids that still fail are printed with a non-zero exit code. It is safe to re-run.

Both scripts resolve `rs3_market.db` and `collector_log.txt` relative to the script directory, not the current directory. Data comes from the Weird Gloop Grand Exchange APIs.

### Sharing a snapshot (outside GitHub)

Two PowerShell scripts (Windows PowerShell 5.1 or PowerShell 7) pack and unpack a gzip snapshot, `rs3_market.db.gz`, which is also gitignored. Share that file through whatever storage you like, but not through GitHub.

```powershell
.\compress_db.ps1             # rs3_market.db -> rs3_market.db.gz (prints size and SHA256)
.\decompress_db.ps1           # rs3_market.db.gz -> rs3_market.db
.\decompress_db.ps1 -Force    # same, overwriting an existing rs3_market.db
```

Both write to a temporary file and only replace the target when the operation succeeds. `decompress_db.ps1` refuses to overwrite an existing `rs3_market.db` unless `-Force` is given. Compare the printed SHA256 against the sender's to confirm the transfer.

## Usage

1. **Scan Market**: Click the "Scan Market" button to load and analyze data from your database
2. **Set Budget**: Adjust your available GP budget using the slider
3. **Filter Results**: Use the side panel to filter by tier, minimum profit, ROI, or search for specific items
4. **Sort Data**: Click column headers or use the sort dropdown to organize results
5. **Mark Favorites**: Click the star button to save items to your favorites list (stored in `favorites.json`)
6. **Copy Details**: Click the clipboard button to copy flip details to your clipboard

## Filtering Logic

The analyzer automatically filters out:
- Items with less than 500 average daily volume (insufficient liquidity)
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

## License

MIT

## Disclaimer

This tool is for educational purposes. Always verify market data before making trades.

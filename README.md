# RS3 Grand Exchange Analyzer

A desktop application for analyzing RuneScape 3 Grand Exchange market data to identify profitable flipping opportunities.

## Features

- **Advanced Flip Analysis**: Multi-factor scoring system that evaluates ROI, trading volume, profit margins, price trends, and data reliability
- **Smart Filtering**: Automatically filters out unrealistic flips with low volume, extreme ROI, or suspicious pricing
- **Tier System**: Items categorized as Diamond 💎, Gold ⭐, Good ✅, Normal ⚪, or Crash 📉 based on profitability
- **Price Trends**: Real-time trend indicators showing Rising++, Rising+, Stable, Falling-, and Falling-- price movements
- **Persistent Favorites**: Save your favorite flips across sessions
- **Customizable Filters**: Filter by tier, minimum profit, ROI, budget, and search terms
- **Modern RS3 UI**: Dark brown/gold theme inspired by the Grand Exchange interface

## Requirements

- Rust (edition 2021 or later)
- SQLite database with market history data
- Windows (for Segoe UI font support)

## Installation

1. Clone the repository:
```bash
git clone https://github.com/AlexMollard/rs3_analyzer.git
cd rs3_analyzer
```

2. Build and run:
```bash
cargo run --release
```

## Usage

1. **Scan Market**: Click the "🔍 Scan Market" button to load and analyze data from your database
2. **Set Budget**: Adjust your available GP budget using the slider
3. **Filter Results**: Use the side panel to filter by tier, minimum profit, ROI, or search for specific items
4. **Sort Data**: Click column headers or use the sort dropdown to organize results
5. **Mark Favorites**: Click the ★ button to save items to your favorites list
6. **Copy Details**: Click the 📋 button to copy flip details to your clipboard

## Database Setup

The application expects a SQLite database at `rs3_market.db` with the following schema:

```sql
CREATE TABLE items (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    ge_limit INTEGER NOT NULL
);

CREATE TABLE history (
    item_id INTEGER NOT NULL,
    record_date TEXT NOT NULL,
    price INTEGER NOT NULL,
    volume INTEGER NOT NULL,
    FOREIGN KEY (item_id) REFERENCES items(id)
);
```

Data should be collected daily from the Weirdgloop API or similar sources.

## Filtering Logic

The analyzer automatically filters out:
- Items with less than 500 average daily volume (insufficient liquidity)
- Items with ROI exceeding 150% (likely data anomalies)
- Items with buy prices below 100gp (vendor trash/data errors)

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

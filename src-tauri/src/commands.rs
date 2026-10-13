// Plain functions behind the Tauri commands, so they can be tested without a webview
use crate::{
    backtest::date_string,
    flips::{LIVE_LOOKBACK, LIVE_QUOTE_DAYS, LiveEstimate, analyze, ge_tax, live_estimate},
    hourly::{self, APP_RULE, HourlyQuote},
    loader::{HistoryRow, RtRow, first_rt_hours, load_item_history, load_rt, load_rt_hourly, load_snapshots, newest_rt_hour},
    model::{Risk, RtDay, RtHour},
    stats::{build_stats, day_number},
};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const DB_FILE: &str = "rs3_market.db";
pub const MISSING_DB_MSG: &str = "rs3_market.db not found — run decompress_db.ps1 or backfill_history.py";
const SPARK_POINTS: usize = 30;
/// Real-time data whose newest day is older than this many days is not shown as live.
const LIVE_MAX_AGE: i64 = 3;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlipRow {
    pub item_id: i32,
    pub name: String,
    pub score: i32,
    pub tier: &'static str,  // S, A, B, C, or X (net loss)
    pub buy: i64,
    pub sell: i64,
    pub ge_limit: Option<i32>,  // None = unknown, qty not capped
    pub unit_profit: f64,  // Net profit per item after tax
    pub roi: f64,
    pub avg_volume: f64,
    pub trend: f64,  // % change per daily sample
    pub tax: i64,  // GE tax per item at the sell price
    pub risk: Risk,
    pub outliers: usize,
    pub data_points: usize,
    pub band: (u8, u8),  // Percentiles used for buy/sell
    pub notes: String,
    pub spark: Vec<f64>,  // Last prices in chronological order
    pub live: Option<LiveRow>,  // None without fresh real-time data for the item
    pub strategy: Option<StrategyQuote>,  // None when the hourly strategy would not trade the item
}

/// The hourly strategy's offers for an item (hourly::daily_pick with hourly::APP_RULE). The UI
/// ranks them, because each pick's capacity depends on the budget.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyQuote {
    pub buy: i64,  // bid
    pub sell: i64,  // ask
    pub margin: f64,  // per item after tax
    pub liquidity: f64,  // median units per hour on the thinner side
}

impl From<HourlyQuote> for StrategyQuote {
    fn from(q: HourlyQuote) -> Self {
        StrategyQuote { buy: q.buy, sell: q.sell, margin: q.margin, liquidity: q.liquidity }
    }
}

/// One held-out test result of the strategy (docs/strategy.md), top PICKS at one budget.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyResult {
    pub budget: f64,
    pub trips: usize,
    pub win_rate: f64,  // profitable trips / trips
    pub win_ci: (f64, f64),  // 95% block-bootstrap interval
    pub profit_day: f64,  // mean realised gp per signal day
    pub profit_ci: (f64, f64),
}

/// What the UI shows about the hourly strategy. The day's quotes are on the rows.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub validated: bool,  // met the bar on the held-out test (hourly::APP_VALIDATED)
    pub signal_date: Option<String>,  // UTC day the quotes are for; None without recent hourly data
    pub data_through: Option<String>,  // newest stored bucket, "yyyy-mm-dd hh:00" UTC
    pub signal_hour: i64,  // UTC hour the signal is taken
    pub entry_hours: i64,  // bids can fill in the buckets this many hours after the signal
    pub horizon_hours: i64,  // sales run to this many hours after it; the last bucket takes the forced exit
    pub picks: usize,  // items traded per day, each with budget / picks
    pub test_days: &'static str,  // held-out test period behind `results`
    pub results: &'static [StrategyResult],
}

const STRATEGY_DESCRIPTION: &str = "Bid at the 25th percentile of the last 24 hourly instant-sell prices and ask at the 40th \
     percentile of the instant-buy prices, on items that trade both ways most hours, clear at least 1% after tax and pass \
     the market-health checks.";
const STRATEGY_TEST_DAYS: &str = "2026-09-24 to 2026-10-12";
/// The one held-out test run of APP_RULE, as docs/strategy.md reports it.
const STRATEGY_RESULTS: &[StrategyResult] = &[
    StrategyResult {
        budget: 10_000_000.0,
        trips: 223,
        win_rate: 0.843,
        win_ci: (0.818, 0.874),
        profit_day: 392_700.0,
        profit_ci: (239_000.0, 586_600.0),
    },
    StrategyResult {
        budget: 50_000_000.0,
        trips: 209,
        win_rate: 0.861,
        win_ci: (0.816, 0.89),
        profit_day: 935_800.0,
        profit_ci: (592_800.0, 1_260_000.0),
    },
    StrategyResult {
        budget: 250_000_000.0,
        trips: 195,
        win_rate: 0.862,
        win_ci: (0.818, 0.893),
        profit_day: 1_290_000.0,
        profit_ci: (296_900.0, 2_100_000.0),
    },
];

/// The item's live quote and its recent track record (flips::live_estimate, docs/backtest.md).
/// Informational: buy, sell and score above stay on the daily guide prices.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveRow {
    pub buy: i64,  // instant-sell average: what a buy offer pays
    pub sell: i64,  // instant-buy average: what a sell offer receives
    pub margin: f64,  // per item after tax
    pub volume: f64,  // median daily units on the thinner side; caps the UI's quantity
    pub unit_day: Option<f64>,  // realised profit/day of one unit at past live quotes, minus one standard error
    pub samples: usize,  // past signals behind unit_day
}

impl From<LiveEstimate> for LiveRow {
    fn from(e: LiveEstimate) -> Self {
        let q = e.quote;
        LiveRow { buy: q.buy, sell: q.sell, margin: q.margin, volume: q.volume, unit_day: e.unit_day, samples: e.samples }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub rows: Vec<FlipRow>,
    pub skipped_rows: usize,
    pub snapshot_date: Option<String>,
    pub live_date: Option<String>,  // real-time day the live quotes are as of; None when missing or stale
    pub item_count: usize,
    pub db_path: String,
    pub strategy: StrategyInfo,
}

/// `searched` is non-empty only when the database is missing, so the UI can show where it looked.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanError {
    pub message: String,
    pub searched: Vec<String>,
}

impl ScanError {
    pub fn other(message: String) -> Self {
        Self { message, searched: vec![] }
    }
}

pub(crate) fn tier_code(tier: &str) -> &'static str {
    match tier {
        "DIAMOND" => "S",
        "GOLD" => "A",
        "GREEN" => "B",
        "CRASH" => "X",
        _ => "C",
    }
}

/// Directories searched for the database and a legacy favorites.json: the current directory,
/// then the executable's directory. Debug builds also try the repo root, because
/// `pnpm tauri dev` runs the app from src-tauri/.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd);
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        dirs.push(dir);
    }
    #[cfg(debug_assertions)]
    if let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        dirs.push(root.to_path_buf());
    }
    dirs.dedup();
    dirs
}

pub fn find_db() -> Result<PathBuf, ScanError> {
    let candidates: Vec<PathBuf> = search_dirs().into_iter().map(|d| d.join(DB_FILE)).collect();
    candidates.iter().find(|p| p.is_file()).cloned().ok_or_else(|| ScanError {
        message: MISSING_DB_MSG.to_string(),
        searched: candidates.iter().map(|p| p.display().to_string()).collect(),
    })
}

/// Days since 1970-01-01 (UTC), as stats::day_number counts them.
fn today() -> i64 {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    (secs / 86_400) as i64
}

/// Each item's real-time days and the newest day as (date, day number), or None when there is
/// no real-time data or its newest day is more than LIVE_MAX_AGE days before `today`.
fn live_days(rows: &[RtRow], today: i64) -> Option<(String, i64, HashMap<i32, Vec<RtDay>>)> {
    let date = rows.iter().map(|r| r.record_date.as_str()).max()?.to_string();
    let t = day_number(&date)?;
    if today - t > LIVE_MAX_AGE {
        return None;
    }
    let mut by_item: HashMap<i32, Vec<RtDay>> = HashMap::new();
    // Rows arrive ordered by item then date, so each item's days stay chronological
    for r in rows {
        if let Some(day) = r.rt_day() {
            by_item.entry(r.item_id).or_default().push(day);
        }
    }
    Some((date, t, by_item))
}

/// The hourly buckets the strategy's daily pick reads.
struct StrategyHours {
    day: i64,  // signal day: the newest UTC day whose signal bucket (23:00 the day before) is stored
    newest: i64,  // newest stored bucket (hour index)
    by_item: HashMap<i32, Vec<RtHour>>,  // from hourly::window_hours buckets before the signal bucket on
    first: HashMap<i32, i64>,  // each item's first stored bucket
}

/// None without hourly data, or when the signal day is more than LIVE_MAX_AGE days before `today`.
/// Also returns the number of unreadable rows.
fn strategy_hours(path: &Path, today: i64) -> rusqlite::Result<(Option<StrategyHours>, usize)> {
    let Some(newest) = newest_rt_hour(path)? else { return Ok((None, 0)) };
    let day = (newest + 1).div_euclid(24);
    if today - day > LIVE_MAX_AGE {
        return Ok((None, 0));
    }
    let from = hourly::signal_bucket(day) - hourly::window_hours(&APP_RULE) + 1;
    let (rows, skipped) = load_rt_hourly(path, Some(from * 3600))?;
    let first = first_rt_hours(path)?;
    Ok((Some(StrategyHours { day, newest, by_item: rows.into_iter().collect(), first }), skipped))
}

/// Scan steps reported through `progress`: 1 = reading history, 2 = building stats, 3 = scoring.
/// Quantities depend on the budget and are computed by the UI.
pub fn scan_db(path: &Path, progress: &dyn Fn(u8)) -> Result<ScanResult, ScanError> {
    progress(1);
    let (snaps, skipped_history) = load_snapshots(path).map_err(|e| {
        // An empty file (e.g. created by an older build) opens fine but has no tables
        if e.to_string().contains("no such table") {
            ScanError { message: MISSING_DB_MSG.to_string(), searched: vec![path.display().to_string()] }
        } else {
            ScanError::other(format!("Failed to load {}: {}", path.display(), e))
        }
    })?;
    // Rows arrive ordered by record_date
    let snapshot_date = snaps.last().map(|s| s.record_date.clone());
    // Optional: without readable real-time data the rows just have no live quote
    let (rt_rows, skipped_rt) = load_rt(path, Some(LIVE_LOOKBACK + LIVE_QUOTE_DAYS + LIVE_MAX_AGE)).unwrap_or_else(|e| {
        eprintln!("load_rt: {}", e);
        (Vec::new(), 0)
    });
    let live = live_days(&rt_rows, today());
    // Optional too: without recent hourly data the rows have no strategy quote
    let (hours, skipped_hourly) = strategy_hours(path, today()).unwrap_or_else(|e| {
        eprintln!("rt_hourly: {}", e);
        (None, 0)
    });

    progress(2);
    let stats = build_stats(&snaps);

    progress(3);
    let rows: Vec<FlipRow> = stats
        .iter()
        .filter(|s| s.data_points >= 1)
        .map(|s| {
            let f = analyze(s);
            let spark_from = s.chrono_prices.len().saturating_sub(SPARK_POINTS);
            let live = live.as_ref().and_then(|(_, t, by_item)| {
                live_estimate(by_item.get(&s.item_id)?, &s.name, *t, LIVE_QUOTE_DAYS, LIVE_LOOKBACK, true)
            });
            // Health check (c) uses this scan's guide window, which ends today rather than the day before
            let crashed = matches!(f.risk, Risk::Crashing | Risk::Crashed);
            let strategy = hours.as_ref().and_then(|h| {
                let first = h.first.get(&s.item_id).copied();
                hourly::daily_pick(h.by_item.get(&s.item_id)?, first, h.day, &APP_RULE, &s.name, || crashed)
            });
            FlipRow {
                item_id: s.item_id,
                name: s.name.clone(),
                score: f.score,
                tier: tier_code(&f.tier),
                buy: f.buy,
                sell: f.sell,
                ge_limit: s.ge_limit,
                unit_profit: f.profit,
                roi: f.roi,
                avg_volume: f.avg_volume,
                trend: s.price_trend,
                tax: ge_tax(f.sell, &s.name),
                risk: f.risk,
                outliers: s.outliers_removed,
                data_points: s.data_points,
                band: f.band,
                notes: f.notes,
                spark: s.chrono_prices[spark_from..].to_vec(),
                live: live.map(LiveRow::from),
                strategy: strategy.map(StrategyQuote::from),
            }
        })
        .collect();

    Ok(ScanResult {
        item_count: rows.len(),
        rows,
        skipped_rows: skipped_history + skipped_rt + skipped_hourly,
        snapshot_date,
        live_date: live.map(|(date, _, _)| date),
        db_path: path.display().to_string(),
        strategy: StrategyInfo {
            name: APP_RULE.name,
            description: STRATEGY_DESCRIPTION,
            validated: hourly::APP_VALIDATED,
            signal_date: hours.as_ref().map(|h| date_string(h.day)),
            data_through: hours.as_ref().map(|h| format!("{} {:02}:00", date_string(h.newest.div_euclid(24)), h.newest.rem_euclid(24))),
            signal_hour: hourly::SIGNAL_HOUR,
            entry_hours: hourly::ENTRY_HOURS,
            horizon_hours: hourly::HORIZON_HOURS,
            picks: hourly::PICKS,
            test_days: STRATEGY_TEST_DAYS,
            results: STRATEGY_RESULTS,
        },
    })
}

pub fn item_history(item_id: i32) -> Result<Vec<HistoryRow>, String> {
    let path = find_db().map_err(|e| e.message)?;
    load_item_history(&path, item_id).map_err(|e| format!("Failed to load history for item {}: {}", item_id, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_db_builds_rows_with_spark_and_tier() {
        let path = std::env::temp_dir().join("rs3_analyzer_scan_test.db");
        let _ = std::fs::remove_file(&path);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, ge_limit INTEGER);
                 CREATE TABLE history (item_id INTEGER, record_date TEXT, price INTEGER, volume INTEGER,
                                       PRIMARY KEY (item_id, record_date));
                 INSERT INTO items VALUES (7, 'Test', 100);",
            )
            .unwrap();
            for day in 1..=40 {
                let price = 10_000 + (day % 5) * 400;
                conn.execute(
                    "INSERT INTO history VALUES (7, date('now', ?1), ?2, 5000)",
                    rusqlite::params![format!("-{} days", 41 - day), price],
                )
                .unwrap();
            }
        }
        let steps = std::cell::RefCell::new(Vec::new());
        let result = scan_db(&path, &|s| steps.borrow_mut().push(s));
        let history = load_item_history(&path, 7);
        let _ = std::fs::remove_file(&path);

        let result = result.unwrap();
        assert_eq!(*steps.borrow(), vec![1, 2, 3]);
        assert_eq!(result.item_count, 1);
        assert!(result.snapshot_date.is_some());
        let row = &result.rows[0];
        assert_eq!(row.spark.len(), SPARK_POINTS);
        assert!(["S", "A", "B", "C", "X"].contains(&row.tier));
        assert_eq!(row.tax, row.sell / 50);
        assert_eq!(history.unwrap().len(), 40);
        // No rt_daily or rt_hourly table: no live or strategy quotes
        assert!(row.live.is_none() && result.live_date.is_none());
        assert!(row.strategy.is_none() && result.strategy.signal_date.is_none());
    }

    /// live_db's item (one real-time day, which the strategy ignores) with 8 days of hourly buckets
    /// ending at 23:00 UTC `days_ago` days before today, instant-selling at 9,800 and instant-buying
    /// at 10,500.
    fn hourly_db(name: &str, days_ago: i64) -> PathBuf {
        let path = live_db(name, 1..=1);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE rt_hourly (item_id INTEGER, ts INTEGER, avg_high REAL, avg_low REAL,
                                     high_volume INTEGER, low_volume INTEGER, PRIMARY KEY (item_id, ts)) WITHOUT ROWID;",
        )
        .unwrap();
        let last = (today() - days_ago) * 24 - 1;
        for hour in last - 191..=last {
            conn.execute("INSERT INTO rt_hourly VALUES (7, ?1, 10500, 9800, 1000, 1000)", [hour * 3600]).unwrap();
        }
        path
    }

    #[test]
    fn scan_db_adds_strategy_quotes_from_recent_hours() {
        use serde_json::json;

        let path = hourly_db("rs3_analyzer_strategy_test.db", 0);
        let result = scan_db(&path, &|_| {});
        let _ = std::fs::remove_file(&path);
        let result = result.unwrap();
        // Buckets through 23:00 yesterday: today's signal
        let info = &result.strategy;
        assert_eq!(info.signal_date, Some(date_string(today())));
        assert_eq!(info.data_through, Some(format!("{} 23:00", date_string(today() - 1))));
        assert_eq!((info.name, info.picks, info.entry_hours, info.horizon_hours), (APP_RULE.name, 10, 12, 24));
        let q = result.rows[0].strategy.as_ref().unwrap();
        assert_eq!((q.buy, q.sell, q.margin, q.liquidity), (9800, 10_500, 490.0, 1000.0));
        // The field names ui/lib/types.ts reads
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["strategy"]["signalDate"], json!(info.signal_date));
        assert_eq!(json["strategy"]["dataThrough"], json!(info.data_through));
        assert_eq!(json["strategy"]["validated"], json!(hourly::APP_VALIDATED));
        assert_eq!(json["strategy"]["testDays"], json!(STRATEGY_TEST_DAYS));
        assert_eq!(json["strategy"]["results"][1]["winCi"], json!([0.816, 0.89]));
        assert_eq!(json["strategy"]["results"][1]["profitDay"], json!(935_800.0));
        assert_eq!(json["rows"][0]["strategy"]["liquidity"], json!(1000.0));

        // Hourly data that stopped 5 days ago is too old to trade on
        let stale = hourly_db("rs3_analyzer_strategy_stale_test.db", 5);
        let result = scan_db(&stale, &|_| {});
        let _ = std::fs::remove_file(&stale);
        let result = result.unwrap();
        assert!(result.rows[0].strategy.is_none() && result.strategy.signal_date.is_none());
    }

    /// A database with 40 days of history and real-time days `rt_days` ago for item 7, which
    /// always trades at 9,800 (instant-sell) and 10,500 (instant-buy).
    fn live_db(name: &str, rt_days: std::ops::RangeInclusive<i32>) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        let _ = std::fs::remove_file(&path);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, ge_limit INTEGER);
             CREATE TABLE history (item_id INTEGER, record_date TEXT, price INTEGER, volume INTEGER,
                                   PRIMARY KEY (item_id, record_date));
             CREATE TABLE rt_daily (item_id INTEGER, record_date TEXT, avg_high REAL, avg_low REAL,
                                    high_volume INTEGER, low_volume INTEGER, PRIMARY KEY (item_id, record_date));
             INSERT INTO items VALUES (7, 'Test', 100);",
        )
        .unwrap();
        for day in 1..=40 {
            conn.execute("INSERT INTO history VALUES (7, date('now', ?1), 10000, 5000)", [format!("-{} days", day)]).unwrap();
        }
        for day in rt_days {
            let sql = "INSERT INTO rt_daily VALUES (7, date('now', ?1), 10500, 9800, 1000, 1000)";
            conn.execute(sql, [format!("-{} days", day)]).unwrap();
        }
        path
    }

    #[test]
    fn scan_db_adds_live_quotes_only_when_fresh() {
        use serde_json::json;

        let fresh = live_db("rs3_analyzer_live_test.db", 1..=35);
        let yesterday: String =
            rusqlite::Connection::open(&fresh).unwrap().query_row("SELECT date('now', '-1 days')", [], |r| r.get(0)).unwrap();
        let result = scan_db(&fresh, &|_| {});
        let _ = std::fs::remove_file(&fresh);
        let result = result.unwrap();
        assert_eq!(result.live_date, Some(yesterday));
        let live = result.rows[0].live.as_ref().unwrap();
        assert_eq!((live.buy, live.sell, live.margin, live.volume), (9800, 10_500, 490.0, 1000.0));
        // Every past quote filled the next day and sold the day after: 490 gp over 2 days
        assert_eq!((live.unit_day, live.samples), (Some(245.0), 21));
        // The field names ui/lib/types.ts reads
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["liveDate"], json!(result.live_date));
        assert_eq!(json["rows"][0]["live"]["unitDay"], json!(245.0));
        assert_eq!(json["rows"][0]["live"]["samples"], json!(21));

        let stale = live_db("rs3_analyzer_stale_test.db", 10..=35);
        let result = scan_db(&stale, &|_| {});
        let _ = std::fs::remove_file(&stale);
        let result = result.unwrap();
        assert!(result.rows[0].live.is_none() && result.live_date.is_none());
    }

    #[test]
    fn empty_db_file_reports_missing_db() {
        let path = std::env::temp_dir().join("rs3_analyzer_empty_test.db");
        let _ = std::fs::remove_file(&path);
        rusqlite::Connection::open(&path).unwrap();
        let err = scan_db(&path, &|_| {}).err().unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(err.message, MISSING_DB_MSG);
        assert_eq!(err.searched.len(), 1);
    }
}

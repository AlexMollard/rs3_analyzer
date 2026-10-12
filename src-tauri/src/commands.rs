// Plain functions behind the Tauri commands, so they can be tested without a webview
use crate::{
    flips::{analyze, ge_tax},
    loader::{HistoryRow, load_item_history, load_snapshots},
    model::Risk,
    stats::build_stats,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const DB_FILE: &str = "rs3_market.db";
pub const MISSING_DB_MSG: &str = "rs3_market.db not found — run decompress_db.ps1 or backfill_history.py";
const SPARK_POINTS: usize = 30;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlipRow {
    pub item_id: i32,
    pub name: String,
    pub score: i32,
    pub tier: &'static str,  // S, A, B, C, or X (net loss)
    pub buy: i32,
    pub sell: i32,
    pub ge_limit: Option<i32>,  // None = unknown, qty not capped
    pub unit_profit: f64,  // Net profit per item after tax
    pub roi: f64,
    pub avg_volume: f64,
    pub trend: f64,  // % change per daily sample
    pub tax: i32,  // GE tax per item at the sell price
    pub risk: Risk,
    pub outliers: usize,
    pub data_points: usize,
    pub band: (u8, u8),  // Percentiles used for buy/sell
    pub notes: String,
    pub spark: Vec<f64>,  // Last prices in chronological order
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub rows: Vec<FlipRow>,
    pub skipped_rows: usize,
    pub snapshot_date: Option<String>,
    pub item_count: usize,
    pub db_path: String,
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

fn tier_code(tier: &str) -> &'static str {
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

/// Scan steps reported through `progress`: 1 = reading history, 2 = building stats, 3 = scoring.
/// Quantities depend on the budget and are computed by the UI.
pub fn scan_db(path: &Path, progress: &dyn Fn(u8)) -> Result<ScanResult, ScanError> {
    progress(1);
    let (snaps, skipped_rows) = load_snapshots(path).map_err(|e| {
        // An empty file (e.g. created by an older build) opens fine but has no tables
        if e.to_string().contains("no such table") {
            ScanError { message: MISSING_DB_MSG.to_string(), searched: vec![path.display().to_string()] }
        } else {
            ScanError::other(format!("Failed to load {}: {}", path.display(), e))
        }
    })?;
    // Rows arrive ordered by record_date
    let snapshot_date = snaps.last().map(|s| s.record_date.clone());

    progress(2);
    let stats = build_stats(&snaps);

    progress(3);
    let rows: Vec<FlipRow> = stats
        .iter()
        .filter(|s| s.data_points >= 1)
        .map(|s| {
            let f = analyze(s);
            let spark_from = s.chrono_prices.len().saturating_sub(SPARK_POINTS);
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
            }
        })
        .collect();

    Ok(ScanResult {
        item_count: rows.len(),
        rows,
        skipped_rows,
        snapshot_date,
        db_path: path.display().to_string(),
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

pub mod backtest;
mod commands;
mod favorites;
mod flips;
#[cfg(test)]
mod format;
mod loader;
mod model;
mod stats;
mod trade;

use commands::{ScanError, ScanResult};
use favorites::Favorites;
use loader::HistoryRow;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State, async_runtime::spawn_blocking, ipc::Channel};

struct FavoritesState(Arc<Mutex<Favorites>>);

#[derive(Serialize)]
struct FavoritesSnapshot {
    names: Vec<String>,
    warning: Option<String>,
}

fn lock(state: &Mutex<Favorites>) -> std::sync::MutexGuard<'_, Favorites> {
    // A panic while holding the lock cannot leave the map half-written, so keep going
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// Loads and scores the last 90 days. Progress steps go to `on_progress` (see `commands::scan_db`).
#[tauri::command]
async fn scan(on_progress: Channel<u8>) -> Result<ScanResult, ScanError> {
    spawn_blocking(move || {
        let path = commands::find_db()?;
        commands::scan_db(&path, &|step| {
            let _ = on_progress.send(step);
        })
    })
    .await
    .map_err(|e| ScanError::other(format!("Market scan failed unexpectedly: {}", e)))?
}

#[tauri::command]
async fn item_history(item_id: i32) -> Result<Vec<HistoryRow>, String> {
    spawn_blocking(move || commands::item_history(item_id))
        .await
        .map_err(|e| format!("History load failed unexpectedly: {}", e))?
}

#[tauri::command]
async fn get_favorites(state: State<'_, FavoritesState>) -> Result<FavoritesSnapshot, String> {
    let favs = lock(&state.0);
    Ok(FavoritesSnapshot { names: favs.names(), warning: favs.startup_warning.clone() })
}

/// Returns the updated list. On a save error the change still applies for this session.
#[tauri::command]
async fn set_favorite(state: State<'_, FavoritesState>, name: String, on: bool) -> Result<Vec<String>, String> {
    let favs = state.0.clone();
    spawn_blocking(move || {
        let mut favs = lock(&favs);
        favs.set(name, on).map(|()| favs.names())
    })
    .await
    .map_err(|e| format!("Favourites update failed unexpectedly: {}", e))?
}

fn load_favorites(app: &tauri::App) -> Favorites {
    let dir = match app.path().app_config_dir() {
        Ok(dir) => dir,
        Err(e) => return Favorites::unsaved(format!("No config directory ({}) — favourites will not be saved", e)),
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Favorites::unsaved(format!("Could not create {} ({}) — favourites will not be saved", dir.display(), e));
    }
    let path = dir.join(favorites::FAVORITES_FILE);
    let migrated = favorites::migrate_legacy(&path, &commands::search_dirs());
    let mut favs = Favorites::load(path);
    // A load problem matters more than the migration note
    if favs.startup_warning.is_none() {
        favs.startup_warning = migrated;
    }
    favs
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .setup(|app| {
            let favs = load_favorites(app);
            app.manage(FavoritesState(Arc::new(Mutex::new(favs))));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![scan, item_history, get_favorites, set_favorite])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("error while running the application: {}", e);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        flips::ge_tax,
        format::format_gp,
        loader::{load_rt, load_snapshots},
        model::ItemSnapshot,
        stats::build_stats,
    };

    fn snap(date: String, price: i64) -> ItemSnapshot {
        ItemSnapshot {
            item_id: 1,
            name: "Test".to_string(),
            ge_limit: None,
            record_date: date,
            price,
            volume: 1000,
        }
    }

    fn trend_of(prices: &[i64]) -> f64 {
        let snaps: Vec<ItemSnapshot> = prices
            .iter()
            .enumerate()
            .map(|(i, &price)| snap(format!("2026-01-{:02}", i + 1), price))
            .collect();
        build_stats(&snaps)[0].price_trend
    }

    #[test]
    fn recent_window_is_last_14_days_across_month_boundary() {
        // Jan 20 .. Feb 10 daily (22 records): the window is Jan 28 .. Feb 10
        let mut snaps: Vec<ItemSnapshot> = (20..=31).map(|d| snap(format!("2026-01-{:02}", d), 100)).collect();
        snaps.extend((1..=10).map(|d| snap(format!("2026-02-{:02}", d), 200)));
        assert_eq!(build_stats(&snaps)[0].recent_prices_chrono.len(), 14);
    }

    #[test]
    fn pipeline_reads_null_limit_and_volume() {
        let path = std::env::temp_dir().join("rs3_analyzer_pipeline_test.db");
        let _ = std::fs::remove_file(&path);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, ge_limit INTEGER);
                 CREATE TABLE history (item_id INTEGER, record_date TEXT, price INTEGER, volume INTEGER,
                                       PRIMARY KEY (item_id, record_date));
                 INSERT INTO items VALUES (1, 'Test', NULL);
                 INSERT INTO history VALUES (1, date('now', '-3 days'), 1000, 500);
                 INSERT INTO history VALUES (1, date('now', '-2 days'), 1100, NULL);
                 INSERT INTO history VALUES (1, date('now', '-1 days'), 1200, 700);",
            ).unwrap();
        }
        let (snaps, skipped) = load_snapshots(path.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(skipped, 0);
        assert_eq!(snaps.len(), 3);
        assert!(snaps.iter().all(|s| s.ge_limit.is_none()));
        assert_eq!(snaps[1].volume, 0);
        let stats = build_stats(&snaps);
        assert!(stats[0].price_trend > 0.0);
        let _ = crate::flips::analyze(&stats[0]);
    }

    #[test]
    fn rt_daily_loads_and_is_optional() {
        let path = std::env::temp_dir().join("rs3_analyzer_rt_test.db");
        let _ = std::fs::remove_file(&path);
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, ge_limit INTEGER);")
            .unwrap();
        // Databases from before the real-time table still scan
        let before = load_rt(&path, Some(40)).unwrap().0.len();
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE rt_daily (item_id INTEGER, record_date TEXT, avg_high REAL, avg_low REAL,
                                        high_volume INTEGER, low_volume INTEGER, PRIMARY KEY (item_id, record_date));
                 INSERT INTO rt_daily VALUES (1, date('now', '-50 days'), 10.0, 9.0, 5, 5);
                 INSERT INTO rt_daily VALUES (1, date('now', '-2 days'), 10.5, NULL, 7, 0);
                 INSERT INTO rt_daily VALUES (1, date('now', '-1 days'), 11, 9.5, 8, 6);",
            )
            .unwrap();
        let (recent, skipped) = load_rt(&path, Some(40)).unwrap();
        let all = load_rt(&path, None).unwrap().0;
        let _ = std::fs::remove_file(&path);
        assert_eq!((before, recent.len(), skipped, all.len()), (0, 2, 0, 3));
        assert_eq!(recent[0].avg_low, None);
        assert_eq!(recent[1].avg_high, Some(11.0));  // an integer in a REAL column reads as f64
    }

    #[test]
    fn missing_db_is_not_created() {
        let path = std::env::temp_dir().join("rs3_analyzer_missing_test.db");
        let _ = std::fs::remove_file(&path);
        assert!(load_snapshots(path.to_str().unwrap()).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn descending_prices_give_negative_trend() {
        let prices: Vec<i64> = (0..20).map(|i| 10_000 - i * 100).collect();
        assert!(trend_of(&prices) < 0.0);
    }

    #[test]
    fn ascending_prices_give_positive_trend() {
        let prices: Vec<i64> = (0..20).map(|i| 10_000 + i * 100).collect();
        assert!(trend_of(&prices) > 0.0);
    }

    #[test]
    fn ge_tax_rounds_down_and_exempts_bonds() {
        assert_eq!(ge_tax(49, "Coal"), 0);
        assert_eq!(ge_tax(4992, "Coal"), 99);
        assert_eq!(ge_tax(100_000_000, "Bond"), 0);
        assert_eq!(ge_tax(5_000_000_000, "Coal"), 100_000_000);
        assert_eq!(ge_tax(i64::MAX, "Coal"), i64::MAX / 50);
    }

    #[test]
    fn prices_above_i32_are_read_and_scored() {
        let path = std::env::temp_dir().join("rs3_analyzer_big_price_test.db");
        let _ = std::fs::remove_file(&path);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, ge_limit INTEGER);
                 CREATE TABLE history (item_id INTEGER, record_date TEXT, price INTEGER, volume INTEGER,
                                       PRIMARY KEY (item_id, record_date));
                 INSERT INTO items VALUES (1, 'Rare', 1);
                 INSERT INTO history VALUES (1, date('now', '-3 days'), 5000000000, 3000000000);
                 INSERT INTO history VALUES (1, date('now', '-2 days'), 5500000000, 500);
                 INSERT INTO history VALUES (1, date('now', '-1 days'), 6000000000, 700);",
            ).unwrap();
        }
        let (snaps, skipped) = load_snapshots(path.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((snaps.len(), skipped), (3, 0));
        assert_eq!(snaps[0].price, 5_000_000_000);
        let stats = build_stats(&snaps);
        let r = crate::flips::analyze(&stats[0]);
        assert!(r.buy > i64::from(i32::MAX) && r.sell > r.buy, "{} {}", r.buy, r.sell);
        assert_eq!(r.profit, (r.sell - r.buy - ge_tax(r.sell, "Rare")) as f64);
    }

    /// Real scan path against the live database: cargo test real_db_scan -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_db_scan() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../rs3_market.db");
        let (snaps, skipped) = load_snapshots(&path).unwrap();
        let stats = build_stats(&snaps);
        let big = stats.iter().filter(|s| s.prices.last().is_some_and(|&p| p > f64::from(i32::MAX))).count();
        let scored = stats.iter().map(crate::flips::analyze).filter(|r| r.buy > i64::from(i32::MAX)).count();
        println!("snapshots={} skipped={} items={} items_above_i32={} scored_above_i32={}", snaps.len(), skipped, stats.len(), big, scored);
        assert_eq!(skipped, 0);
    }

    #[test]
    fn format_gp_handles_negatives() {
        assert!(format_gp(-2_500_000.0).contains("-2.5"));
    }
}

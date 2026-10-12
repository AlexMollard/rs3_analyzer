mod loader;
mod stats;
mod flips;
mod model;
mod ui;

use eframe::egui;
use ui::RS3App;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 1000.0])
            .with_min_inner_size([1200.0, 700.0]),
        ..Default::default()
    };

    eframe::run_native(
        "RS3 Market Analyzer",
        options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            
            if let Ok(segoe_data) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
                fonts.font_data.insert(
                    "SegoeUI".to_owned(),
                    egui::FontData::from_owned(segoe_data).into(),
                );
                if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                    family.insert(0, "SegoeUI".to_owned());
                }
            }
            
            cc.egui_ctx.set_fonts(fonts);
            ui::set_custom_style(&cc.egui_ctx);
            Ok(Box::new(RS3App::new()))
        }),
    )
}

#[cfg(test)]
mod tests {
    use crate::{flips::ge_tax, loader::load_snapshots, model::ItemSnapshot, stats::build_stats, ui::format_gp};

    fn snap(date: String, price: i32) -> ItemSnapshot {
        ItemSnapshot {
            item_id: 1,
            name: "Test".to_string(),
            ge_limit: None,
            record_date: date,
            price,
            volume: 1000,
        }
    }

    fn trend_of(prices: &[i32]) -> f64 {
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
    fn missing_db_is_not_created() {
        let path = std::env::temp_dir().join("rs3_analyzer_missing_test.db");
        let _ = std::fs::remove_file(&path);
        assert!(load_snapshots(path.to_str().unwrap()).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn descending_prices_give_negative_trend() {
        let prices: Vec<i32> = (0..20).map(|i| 10_000 - i * 100).collect();
        assert!(trend_of(&prices) < 0.0);
    }

    #[test]
    fn ascending_prices_give_positive_trend() {
        let prices: Vec<i32> = (0..20).map(|i| 10_000 + i * 100).collect();
        assert!(trend_of(&prices) > 0.0);
    }

    #[test]
    fn ge_tax_rounds_down_and_exempts_bonds() {
        assert_eq!(ge_tax(49, "Coal"), 0);
        assert_eq!(ge_tax(4992, "Coal"), 99);
        assert_eq!(ge_tax(100_000_000, "Bond"), 0);
    }

    #[test]
    fn format_gp_handles_negatives() {
        assert!(format_gp(-2_500_000.0).contains("-2.5"));
    }
}

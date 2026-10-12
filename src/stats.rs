use statrs::statistics::Statistics;
use crate::model::{ItemSnapshot, ItemStats};
use std::collections::HashMap;

const RECENT_DAYS: i64 = 14;

pub fn build_stats(data: &[ItemSnapshot]) -> Vec<ItemStats> {
    let mut map: HashMap<i32, Vec<&ItemSnapshot>> = HashMap::new();

    for snap in data {
        map.entry(snap.item_id).or_default().push(snap);
    }

    let mut results = Vec::new();

    for (id, records) in map {
        // Records arrive in chronological order (ORDER BY record_date)
        let chrono_prices: Vec<f64> = records.iter().map(|x| x.price as f64).collect();
        let volumes: Vec<f64> = records.iter().map(|x| x.volume as f64).collect();

        let current = records.last().unwrap();

        // Get recent prices (last 14 days) for time-weighted analysis
        // Using 14 days instead of 30 to catch rapid crashes/spikes
        let recent_cutoff = match day_number(&current.record_date) {
            Some(latest) => records
                .iter()
                .position(|x| day_number(&x.record_date).is_some_and(|d| latest - d < RECENT_DAYS))
                .unwrap_or(records.len()),
            None => records.len().saturating_sub(RECENT_DAYS as usize),
        };
        let recent_prices_chrono: Vec<f64> = chrono_prices[recent_cutoff..].to_vec();
        let mut recent_prices = recent_prices_chrono.clone();
        recent_prices.sort_by(f64::total_cmp);

        let mut prices = chrono_prices.clone();
        prices.sort_by(f64::total_cmp);

        // Remove outliers using IQR method (removes DXP/update spikes), keeping chronological order
        let (filtered_prices, outliers_removed) = remove_outliers(&chrono_prices);

        // remove_outliers falls back to the full series, so this is never sparser than the raw data
        let price_trend = if filtered_prices.len() >= 3 {
            calculate_trend(&filtered_prices)
        } else {
            0.0
        };

        let stats = ItemStats {
            item_id: id,
            name: current.name.clone(),

            avg_volume: volumes.mean(),

            data_points: prices.len(),
            ge_limit: current.ge_limit,
            prices,
            price_trend,
            filtered_prices,
            outliers_removed,
            recent_prices,
            recent_prices_chrono,
        };

        results.push(stats);
    }

    results
}


// Days since 1970-01-01 for an ISO "yyyy-mm-dd" date (proleptic Gregorian)
fn day_number(date: &str) -> Option<i64> {
    let y: i64 = date.get(0..4)?.parse().ok()?;
    let m: i64 = date.get(5..7)?.parse().ok()?;
    let d: i64 = date.get(8..10)?.parse().ok()?;
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() { return 0.0; }
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[idx]
}

// Remove outliers using IQR method (filters DXP/update spikes)
// Input and output are chronological; only the bounds use a sorted copy
fn remove_outliers(prices: &[f64]) -> (Vec<f64>, usize) {
    if prices.len() < 10 {
        return (prices.to_vec(), 0);
    }

    let mut sorted = prices.to_vec();
    sorted.sort_by(f64::total_cmp);
    let q1 = quantile(&sorted, 0.25);
    let q3 = quantile(&sorted, 0.75);
    let iqr = q3 - q1;

    // Use 1.5 * IQR for outlier detection (standard method)
    let lower_bound = q1 - (1.5 * iqr);
    let upper_bound = q3 + (1.5 * iqr);

    let original_len = prices.len();
    let filtered: Vec<f64> = prices.iter()
        .copied()
        .filter(|&p| p >= lower_bound && p <= upper_bound)
        .collect();

    let removed = original_len - filtered.len();

    // Return original if we filtered too much (>30%)
    if filtered.len() < (original_len * 7 / 10) {
        (prices.to_vec(), 0)
    } else {
        (filtered, removed)
    }
}

// Linear regression slope over a chronological series, relative to its mean:
// % price change per sample (positive = rising, negative = falling)
fn calculate_trend(prices: &[f64]) -> f64 {
    let n = prices.len() as f64;
    if n < 2.0 { return 0.0; }

    let x_mean = (n - 1.0) / 2.0;
    let y_mean = prices.iter().sum::<f64>() / n;

    let mut numerator = 0.0;
    let mut denominator = 0.0;

    for (i, &price) in prices.iter().enumerate() {
        let x_diff = i as f64 - x_mean;
        numerator += x_diff * (price - y_mean);
        denominator += x_diff * x_diff;
    }

    if denominator != 0.0 && y_mean != 0.0 {
        numerator / denominator / y_mean * 100.0
    } else {
        0.0
    }
}

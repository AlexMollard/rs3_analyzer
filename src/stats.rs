use statrs::statistics::Statistics;
use crate::model::{ItemSnapshot, ItemStats};
use std::collections::HashMap;

pub fn build_stats(data: &[ItemSnapshot]) -> Vec<ItemStats> {
    let mut map: HashMap<i32, Vec<&ItemSnapshot>> = HashMap::new();

    for snap in data {
        map.entry(snap.item_id).or_default().push(snap);
    }

    let mut results = Vec::new();

    for (id, records) in map {
        let mut prices: Vec<f64> = records.iter().map(|x| x.price as f64).collect();
        let volumes: Vec<f64> = records.iter().map(|x| x.volume as f64).collect();

        prices.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let current = records.last().unwrap();
        let prev = if records.len() > 1 {
            records[records.len() - 2].price as f64
        } else {
            current.price as f64
        };

        let std_dev = prices.clone().std_dev();

        let price_trend = if prices.len() >= 3 {
            calculate_trend(&prices)
        } else {
            0.0
        };

        let stats = ItemStats {
            item_id: id,
            name: current.name.clone(),

            current_price: *prices.last().unwrap(),
            prev_price: prev,

            avg_volume: volumes.mean(),
            std_dev,

            q10: quantile(&prices, 0.10),
            q50: quantile(&prices, 0.50),
            q90: quantile(&prices, 0.90),

            data_points: prices.len(),
            ge_limit: current.ge_limit,
            current_volume: current.volume as f64,
            prices: prices.clone(),
            price_trend,
        };

        results.push(stats);
    }

    results
}


fn quantile(sorted: &Vec<f64>, q: f64) -> f64 {
    if sorted.is_empty() { return 0.0; }
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[idx]
}

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
    
    if denominator != 0.0 {
        numerator / denominator
    } else {
        0.0
    }
}

use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ItemSnapshot {
    pub item_id: i32,
    pub name: String,
    pub ge_limit: i32,
    pub record_date: String,
    pub price: i32,
    pub volume: i32,
}

pub struct ItemStats {
    pub item_id: i32,
    pub name: String,

    pub current_price: f64,
    pub prev_price: f64,

    pub avg_volume: f64,
    pub std_dev: f64,

    pub q10: f64,
    pub q50: f64,
    pub q90: f64,

    pub data_points: usize,
    pub ge_limit: i32,

    pub current_volume: f64,
    pub prices: Vec<f64>,
    pub price_trend: f64,  // Positive = rising, negative = falling
    pub filtered_prices: Vec<f64>,  // Prices with outliers removed
    pub outliers_removed: usize,
    pub recent_prices: Vec<f64>,  // Last 14 records (sorted) for time-weighted analysis
    pub recent_prices_chrono: Vec<f64>,  // Last 14 records in chronological order
}


#[derive(Debug, Clone)]
pub struct FlipResult {
    pub score: i32,
    pub tier: String,

    pub buy: i32,
    pub sell: i32,

    pub qty: i32,
    pub profit: i32,
    pub roi: f64,
    pub avg_volume: f64,

    pub notes: String,
}

impl FlipResult {
    pub fn empty() -> Self {
        FlipResult {
            score: 0,
            tier: "NONE".to_string(),
            buy: 0,
            sell: 0,
            qty: 0,
            profit: 0,
            roi: 0.0,
            avg_volume: 0.0,
            notes: String::new(),
        }
    }
}

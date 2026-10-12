#[derive(Debug)]
pub struct ItemSnapshot {
    pub item_id: i32,
    pub name: String,
    pub ge_limit: Option<i32>,  // None = unknown limit
    pub record_date: String,
    pub price: i32,
    pub volume: i32,
}

pub struct ItemStats {
    pub item_id: i32,
    pub name: String,

    pub avg_volume: f64,

    pub data_points: usize,
    pub ge_limit: Option<i32>,  // None = unknown limit

    pub prices: Vec<f64>,  // Sorted ascending
    pub chrono_prices: Vec<f64>,  // Raw prices in chronological order
    pub price_trend: f64,  // % change per sample; positive = rising, negative = falling
    pub filtered_prices: Vec<f64>,  // Prices with outliers removed (chronological)
    pub outliers_removed: usize,
    pub recent_prices: Vec<f64>,  // Last 14 days of records (sorted) for time-weighted analysis
    pub recent_prices_chrono: Vec<f64>,  // Last 14 days of records in chronological order
}


/// Headline risk, in priority order (same order the notes prefix uses)
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Stable,
    Outliers,
    Spiked,
    Crashed,
    Crashing,
}

#[derive(Debug, Clone)]
pub struct FlipResult {
    pub score: i32,
    pub tier: String,

    pub buy: i32,
    pub sell: i32,

    pub profit: f64,  // Net profit per item after GE tax
    pub roi: f64,
    pub avg_volume: f64,
    pub band: (u8, u8),  // Percentiles used for buy/sell, e.g. (10, 90)
    pub risk: Risk,

    pub notes: String,
}

impl FlipResult {
    pub fn empty() -> Self {
        FlipResult {
            score: 0,
            tier: "NONE".to_string(),
            buy: 0,
            sell: 0,
            profit: 0.0,
            roi: 0.0,
            avg_volume: 0.0,
            band: (10, 90),
            risk: Risk::Stable,
            notes: String::new(),
        }
    }
}

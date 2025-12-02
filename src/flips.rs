use crate::model::ItemStats;
use crate::model::FlipResult;

pub fn analyze(stats: &ItemStats, tax: f64) -> FlipResult {
    if stats.prices.is_empty() {
        return FlipResult::empty();
    }

    let mut prices = stats.prices.clone();
    prices.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let q05 = quantile(&prices, 0.05);
    let q10 = quantile(&prices, 0.10);
    let _q25 = quantile(&prices, 0.25);
    let q50 = quantile(&prices, 0.50);
    let _q75 = quantile(&prices, 0.75);
    let q90 = quantile(&prices, 0.90);
    let q95 = quantile(&prices, 0.95);

    let buy = q10.round() as i32;
    let sell = q90.round() as i32;

    let price_range = q90 - q10;
    let volatility = if q50 > 0.0 { (price_range / q50) * 100.0 } else { 0.0 };

    let gross = (sell - buy) as f64;
    let tax_loss = sell as f64 * tax;
    let net = gross - tax_loss;

    let roi = if buy > 0 { (net / buy as f64) * 100.0 } else { 0.0 };

    let tier = if net < 0.0 {
        "CRASH"
    } else if roi > 35.0 || net > 5_000_000.0 {
        "DIAMOND"
    } else if roi > 20.0 || net > 1_000_000.0 {
        "GOLD"
    } else if roi > 8.0 || net > 200_000.0 {
        "GREEN"
    } else {
        "NORMAL"
    }.to_string();

    let roi_score = (roi * 2.0).max(i32::MIN as f64).min(i32::MAX as f64) as i32;
    
    let volume_score = if stats.avg_volume > 0.0 {
        (stats.avg_volume.log10() * 15.0).max(0.0).min(100.0) as i32
    } else {
        0
    };
    
    let profit_score = if net > 0.0 {
        ((net / 100_000.0).min(50.0)) as i32
    } else {
        ((net / 100_000.0).max(-50.0)) as i32
    };
    
    // Volatility bonus - items with price swings are better for flipping
    let volatility_score = (volatility.min(100.0) / 2.0) as i32;
    
    let reliability_score = ((stats.data_points as f64 / 5.0).min(10.0)) as i32;
    
    let spread_penalty = if price_range < (buy as f64 * 0.02) {
        -20
    } else {
        0
    };
    
    let trend_score = if stats.price_trend > 0.0 {
        (stats.price_trend.min(50.0) / 2.0) as i32
    } else {
        (stats.price_trend.max(-50.0) / 2.0) as i32
    };

    let score = roi_score
        .saturating_add(volume_score)
        .saturating_add(profit_score)
        .saturating_add(volatility_score)
        .saturating_add(reliability_score)
        .saturating_add(spread_penalty)
        .saturating_add(trend_score);

    FlipResult {
        score,
        tier,
        buy,
        sell,
        qty: 1,
        profit: net.round() as i32,
        roi,
        avg_volume: stats.avg_volume,
        notes: format!(
            "Vol:{:.0}% | Spread:{}gp | Q5-Q95:{:.0}-{:.0} | Data:{}pts",
            volatility,
            (sell - buy),
            q05.round(),
            q95.round(),
            stats.data_points
        ),
    }
}

fn quantile(v: &Vec<f64>, q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let idx = ((v.len() - 1) as f64 * q).round() as usize;
    v[idx]
}

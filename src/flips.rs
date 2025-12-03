use crate::model::ItemStats;
use crate::model::FlipResult;

pub fn analyze(stats: &ItemStats, tax: f64) -> FlipResult {
    if stats.prices.is_empty() {
        return FlipResult::empty();
    }

    // Check if price has crashed (recent vs overall using multiple indicators)
    let overall_median = quantile(&stats.prices, 0.50);
    let overall_q75 = quantile(&stats.prices, 0.75);
    let recent_median = if !stats.recent_prices.is_empty() {
        quantile(&stats.recent_prices, 0.50)
    } else {
        overall_median
    };
    
    // More sensitive crash detection: compare recent median to overall Q75
    // This catches items that crashed from high prices even if overall median is mid-range
    let price_crashed = recent_median < (overall_median * 0.80) || recent_median < (overall_q75 * 0.65);
    let price_spiked = recent_median > (overall_median * 1.25);
    
    // Check for post-spike crash: recent prices much lower than peak (Q75+)
    let recent_avg_all = if !stats.recent_prices_chrono.is_empty() {
        stats.recent_prices_chrono.iter().sum::<f64>() / stats.recent_prices_chrono.len() as f64
    } else {
        recent_median
    };
    
    // Detect if item peaked and is now crashing (high volatility item)
    let post_spike_crash = recent_avg_all < (overall_q75 * 0.82);
    
    // Also check for very recent downtrend within the recent window
    let recent_downtrend = if stats.recent_prices_chrono.len() >= 6 {
        let mid = stats.recent_prices_chrono.len() / 2;
        let first_half_avg = stats.recent_prices_chrono[..mid].iter().sum::<f64>() / mid as f64;
        let second_half_avg = stats.recent_prices_chrono[mid..].iter().sum::<f64>() 
            / (stats.recent_prices_chrono.len() - mid) as f64;
        second_half_avg < (first_half_avg * 0.90)
    } else {
        false
    };
    
    let recent_trend_crash = post_spike_crash || recent_downtrend;
    
    // Use time-weighted approach: prioritize recent prices if market has changed significantly
    let use_recent = (price_crashed || price_spiked) && stats.recent_prices.len() >= 10;
    let use_filtered = !use_recent && !stats.filtered_prices.is_empty() && stats.outliers_removed > 0;
    
    let analysis_prices = if use_recent {
        stats.recent_prices.clone()
    } else if use_filtered {
        stats.filtered_prices.clone()
    } else {
        stats.prices.clone()
    };
    
    let mut prices = analysis_prices;
    prices.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let q05 = quantile(&prices, 0.05);
    let q10 = quantile(&prices, 0.10);
    let q15 = quantile(&prices, 0.15);
    let _q25 = quantile(&prices, 0.25);
    let q50 = quantile(&prices, 0.50);
    let _q75 = quantile(&prices, 0.75);
    let q85 = quantile(&prices, 0.85);
    let q90 = quantile(&prices, 0.90);
    let q95 = quantile(&prices, 0.95);

    // Use more conservative percentiles for crashed/spiked items
    let (buy, sell) = if use_recent {
        // For crashed items, use tighter range (Q15-Q85) to avoid old extremes
        (q15.round() as i32, q85.round() as i32)
    } else {
        // Normal items use standard Q10-Q90
        (q10.round() as i32, q90.round() as i32)
    };

    let price_range = q90 - q10;
    let volatility = if q50 > 0.0 { (price_range / q50) * 100.0 } else { 0.0 };

    let gross = (sell - buy) as f64;
    let tax_loss = sell as f64 * tax;
    let net = gross - tax_loss;

    let roi = if buy > 0 { (net / buy as f64) * 100.0 } else { 0.0 };

    // Keep tier calculation normal - don't force CRASH for volatile items
    let tier = if net < 0.0 {
        "CRASH".to_string()
    } else if roi > 35.0 || net > 5_000_000.0 {
        "DIAMOND".to_string()
    } else if roi > 20.0 || net > 1_000_000.0 {
        "GOLD".to_string()
    } else if roi > 8.0 || net > 200_000.0 {
        "GREEN".to_string()
    } else {
        "NORMAL".to_string()
    };

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

    // Penalize if many outliers were removed (indicates unstable price)
    let outlier_penalty = if stats.outliers_removed > (stats.data_points / 5) {
        -30
    } else if stats.outliers_removed > 0 {
        -10
    } else {
        0
    };
    
    // Heavy penalty for crashed items (risky - price falling)
    // But not SO heavy that it completely removes good volatile opportunities
    let crash_penalty = if recent_trend_crash {
        -80  // VERY recent crash - very risky but might be opportunity
    } else if price_crashed {
        -50
    } else if price_spiked {
        -30  // Spikes also risky - might crash back down
    } else {
        0
    };
    
    let score = roi_score
        .saturating_add(volume_score)
        .saturating_add(profit_score)
        .saturating_add(volatility_score)
        .saturating_add(reliability_score)
        .saturating_add(spread_penalty)
        .saturating_add(trend_score)
        .saturating_add(outlier_penalty)
        .saturating_add(crash_penalty);

    let mut analysis_notes = String::new();
    
    // Show crash warnings prominently at the start
    if recent_trend_crash {
        analysis_notes.push_str("🚨VOLATILE-CRASHING | ");
    } else if use_recent {
        if price_crashed {
            analysis_notes.push_str("📉Crashed | ");
        } else if price_spiked {
            analysis_notes.push_str("📈Spiked | ");
        }
    }
    
    if stats.outliers_removed > 0 {
        analysis_notes.push_str(&format!("{}⚠outliers | ", stats.outliers_removed));
    }

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
            "{}Vol:{:.0}% | Spread:{}gp | Q5-Q95:{:.0}-{:.0} | Data:{}pts",
            analysis_notes,
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

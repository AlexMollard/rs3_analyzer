use crate::model::ItemStats;
use crate::model::{FlipResult, Risk, RtDay};
use crate::trade::{HORIZON, Order, rt_path, rt_window, simulate};

/// RS3 GE tax per item: 2% of the sale price, rounded down per item, no cap.
/// Bonds are exempt; items under 50gp therefore pay nothing.
pub fn ge_tax(price_per_item: i64, item_name: &str) -> i64 {
    if item_name == "Bond" {
        0
    } else {
        price_per_item.max(0) / 50  // == floor(2%), without overflow
    }
}

/// Offer prices from recent real-time days: bid at the instant-sell (low) average, ask at the
/// instant-buy (high) average, both the median over the days where both sides traded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveQuote {
    pub buy: i64,
    pub sell: i64,
    pub margin: f64,  // per unit after GE tax
    pub volume: f64,  // median daily min(instant-buy, instant-sell) volume
    pub days: usize,  // two-sided days used
}

/// Median; NaN when empty.
pub(crate) fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let m = v.len() / 2;
    if v.len().is_multiple_of(2) { (v[m - 1] + v[m]) / 2.0 } else { v[m] }
}

/// None when no day in `days` traded on both sides.
pub fn live_quote(days: &[RtDay], name: &str) -> Option<LiveQuote> {
    let both: Vec<(f64, f64, f64)> = days
        .iter()
        .filter_map(|d| Some((d.low?, d.high?, d.low_volume.min(d.high_volume))))
        .collect();
    if both.is_empty() {
        return None;
    }
    let gp = |v: f64| v.round().clamp(0.0, i64::MAX as f64) as i64;
    let buy = gp(median(both.iter().map(|x| x.0).collect()));
    let sell = gp(median(both.iter().map(|x| x.1).collect()));
    Some(LiveQuote {
        buy,
        sell,
        margin: sell as f64 - buy as f64 - ge_tax(sell, name) as f64,
        volume: median(both.iter().map(|x| x.2).collect()),
        days: both.len(),
    })
}

/// Real-time days behind the app's live quote, and the track-record lookback (docs/backtest.md).
pub const LIVE_QUOTE_DAYS: i64 = 3;
pub const LIVE_LOOKBACK: i64 = 28;

/// Mean of past per-day results, minus one standard error with `lcb`. None with fewer than two.
pub(crate) fn track_record(past: &[f64], lcb: bool) -> Option<f64> {
    if past.len() < 2 {
        return None;
    }
    let n = past.len() as f64;
    let m = past.iter().sum::<f64>() / n;
    let sd = (past.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
    Some(if lcb { m - sd / n.sqrt() } else { m })
}

/// An item's live quote and how the same quote rule has done for it lately.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveEstimate {
    pub quote: LiveQuote,
    pub unit_day: Option<f64>,  // expected realised profit/day of one unit; None with < 2 past signals
    pub samples: usize,  // past signals behind unit_day
}

/// The live quote as of day `t` and its track record: the quote rule replayed for one unit on
/// each day in [t - lookback, t - HORIZON - 1], whose outcomes are all known by day `t`.
/// None when there is no quote at `t`.
pub fn live_estimate(rt: &[RtDay], name: &str, t: i64, quote_days: i64, lookback: i64, lcb: bool) -> Option<LiveEstimate> {
    let quote = live_quote(rt_window(rt, t, quote_days), name)?;
    let past: Vec<f64> = (t - lookback..t - HORIZON)
        .filter_map(|tp| {
            let q = live_quote(rt_window(rt, tp, quote_days), name)?;
            let order = Order { buy: q.buy, sell: q.sell, qty: 1, name };
            Some(simulate(&order, tp, &rt_path(rt, tp), 1.0).per_day())
        })
        .collect();
    Some(LiveEstimate { quote, unit_day: track_record(&past, lcb), samples: past.len() })
}

pub fn analyze(stats: &ItemStats) -> FlipResult {
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
    prices.sort_by(f64::total_cmp);

    let q05 = quantile(&prices, 0.05);
    let q50 = quantile(&prices, 0.50);
    let q95 = quantile(&prices, 0.95);

    // Use more conservative percentiles for crashed/spiked items:
    // crashed/spiked items use a tighter range (Q15-Q85) to avoid old extremes,
    // normal items use the standard Q10-Q90.
    // The same pair drives buy/sell, volatility and the spread penalty so they all
    // describe the same price band.
    let (lo_q, hi_q) = if use_recent { (0.15, 0.85) } else { (0.10, 0.90) };
    let lo = quantile(&prices, lo_q);
    let hi = quantile(&prices, hi_q);
    let (buy, sell) = (lo.round() as i64, hi.round() as i64);

    let price_range = hi - lo;
    let volatility = if q50 > 0.0 { (price_range / q50) * 100.0 } else { 0.0 };

    let gross = (sell - buy) as f64;
    let tax_loss = ge_tax(sell, &stats.name) as f64;
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

    let roi_score = (roi * 2.0).clamp(i32::MIN as f64, i32::MAX as f64) as i32;

    let volume_score = if stats.avg_volume > 0.0 {
        (stats.avg_volume.log10() * 15.0).clamp(0.0, 100.0) as i32
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
    
    // price_trend is % per sample; saturates at +/-2% (twice the strong-trend threshold) -> +/-25
    let trend_score = (stats.price_trend.clamp(-2.0, 2.0) * 12.5) as i32;

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
    let mut risk = Risk::Stable;

    // Show crash warnings prominently at the start
    if recent_trend_crash {
        analysis_notes.push_str("🚨VOLATILE-CRASHING | ");
        risk = Risk::Crashing;
    } else if use_recent {
        if price_crashed {
            analysis_notes.push_str("📉Crashed | ");
            risk = Risk::Crashed;
        } else if price_spiked {
            analysis_notes.push_str("📈Spiked | ");
            risk = Risk::Spiked;
        }
    }

    if stats.outliers_removed > 0 {
        analysis_notes.push_str(&format!("{}⚠outliers | ", stats.outliers_removed));
        if risk == Risk::Stable {
            risk = Risk::Outliers;
        }
    }

    FlipResult {
        score,
        tier,
        buy,
        sell,
        profit: net,
        roi,
        avg_volume: stats.avg_volume,
        risk,
        band: ((lo_q * 100.0).round() as u8, (hi_q * 100.0).round() as u8),
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

fn quantile(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let idx = ((v.len() - 1) as f64 * q).round() as usize;
    v[idx]
}

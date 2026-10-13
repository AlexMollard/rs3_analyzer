//! Hourly flip strategies (docs/strategy.md): a bid and an ask from an item's recent real-time
//! buckets, judged by trade::simulate_hourly. The candidates and every setting below were fixed in
//! docs/strategy-plan.md before the search was run.

use crate::flips::{ge_tax, median};
use crate::model::RtHour;
use crate::trade::{VOLUME_SHARE, order_qty};

/// UTC hour of day the daily signal is taken: it sees buckets up to the one starting an hour earlier.
pub const SIGNAL_HOUR: i64 = 0;
/// The bid can fill in the 12 buckets after the signal (00:00-11:00 UTC).
pub const ENTRY_HOURS: i64 = 12;
/// Sales run to the 24th bucket after the signal (23:00 UTC), which also takes the forced exit.
pub const HORIZON_HOURS: i64 = 24;
/// Share of lookback buckets in which both sides must have traded.
pub const MIN_ACTIVE: f64 = 0.75;
/// The app's filters: buy price at least 100 gp (this also keeps out untaxed items under 50 gp) and
/// ROI at most 150%.
pub const MIN_BUY: i64 = 100;
pub const MAX_ROI: f64 = 1.5;

/// How the bid and the ask come from the lookback buckets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pricing {
    /// Bid at the median hourly instant-sell average, ask at the median instant-buy average.
    Median,
    /// Bid `k` sample standard deviations below the mean hourly instant-sell average, ask at that mean.
    Dip { k: f64 },
    /// Bid and ask at these nearest-rank quantiles of the hourly instant-sell and instant-buy averages.
    Quantiles { bid: f64, ask: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HourlyRule {
    pub name: &'static str,
    pub pricing: Pricing,
    pub lookback: i64,  // buckets, ending with the signal bucket
    pub min_margin: f64,  // margin after tax as a share of the bid
    pub health: bool,  // market-health checks: `healthy` plus the guide-price crash flag
}

const fn rule(name: &'static str, pricing: Pricing, lookback: i64, min_margin: f64, health: bool) -> HourlyRule {
    HourlyRule { name, pricing, lookback, min_margin, health }
}

const CONS: Pricing = Pricing::Quantiles { bid: 0.25, ask: 0.40 };

/// The frozen candidate grid (docs/strategy-plan.md).
pub const RULES: [HourlyRule; 12] = [
    rule("flip24-m1", Pricing::Median, 24, 0.01, false),
    rule("flip72-m1", Pricing::Median, 72, 0.01, false),
    rule("flip24-m3", Pricing::Median, 24, 0.03, false),
    rule("flip72-m3", Pricing::Median, 72, 0.03, false),
    rule("flip24-m1-h", Pricing::Median, 24, 0.01, true),
    rule("flip72-m1-h", Pricing::Median, 72, 0.01, true),
    rule("flip24-m3-h", Pricing::Median, 24, 0.03, true),
    rule("flip72-m3-h", Pricing::Median, 72, 0.03, true),
    rule("dip1-h", Pricing::Dip { k: 1.0 }, 168, 0.01, true),
    rule("dip2-h", Pricing::Dip { k: 2.0 }, 168, 0.01, true),
    rule("cons24-h", CONS, 24, 0.01, true),
    rule("cons72-h", CONS, 72, 0.01, true),
];

/// Items traded per day, each with budget / PICKS.
pub const PICKS: usize = 10;

/// The strategy the app shows: the one the pre-registered selection rule chose on train and
/// validation (docs/strategy.md). It stays the choice whatever the held-out test said.
pub const APP_RULE: HourlyRule = RULES[10];
/// Whether APP_RULE met the bar on the held-out test (docs/strategy-plan.md, "Test and decision"):
/// it did, with 86.1% of trades profitable and 935.8K a day at 50M over 19 days.
pub const APP_VALIDATED: bool = true;

/// The last bucket a signal for UTC day `day` (days since 1970-01-01) may see.
pub fn signal_bucket(day: i64) -> i64 {
    day * 24 + SIGNAL_HOUR - 1
}

/// Buckets dated in [h - n + 1, h]; `hours` is chronological.
pub fn hour_window(hours: &[RtHour], h: i64, n: i64) -> &[RtHour] {
    let lo = hours.partition_point(|b| b.hour <= h - n);
    let hi = hours.partition_point(|b| b.hour <= h);
    &hours[lo..hi]
}

/// One rule's offer prices for an item as of signal bucket `h`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HourlyQuote {
    pub buy: i64,
    pub sell: i64,
    pub margin: f64,  // per unit after GE tax
    pub liquidity: f64,  // median units per lookback bucket on the thinner side (no trade = 0)
    pub active: f64,  // share of lookback buckets in which both sides traded
}

fn nearest_rank(mut v: Vec<f64>, q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

/// The rule's quote from buckets h-lookback+1 ..= h only. None when a side never traded in them.
pub fn hourly_quote(hours: &[RtHour], h: i64, rule: &HourlyRule, name: &str) -> Option<HourlyQuote> {
    let w = hour_window(hours, h, rule.lookback);
    let lows: Vec<f64> = w.iter().filter_map(|b| b.low).collect();
    let highs: Vec<f64> = w.iter().filter_map(|b| b.high).collect();
    if lows.is_empty() || highs.is_empty() {
        return None;
    }
    let (buy, sell) = match rule.pricing {
        Pricing::Median => (median(lows), median(highs)),
        Pricing::Dip { k } => {
            let n = lows.len() as f64;
            let mean = lows.iter().sum::<f64>() / n;
            let var = if lows.len() > 1 { lows.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0) } else { 0.0 };
            (mean - k * var.sqrt(), mean)
        }
        Pricing::Quantiles { bid, ask } => (nearest_rank(lows, bid), nearest_rank(highs, ask)),
    };
    let gp = |v: f64| v.round().clamp(0.0, i64::MAX as f64) as i64;
    let (buy, sell) = (gp(buy), gp(sell));
    // A one-sided bucket has 0 volume on its other side; missing buckets add zeros
    let mut thin: Vec<f64> = w.iter().map(|b| b.low_volume.min(b.high_volume)).collect();
    thin.resize(rule.lookback.max(1) as usize, 0.0);
    let both = w.iter().filter(|b| b.low.is_some() && b.high.is_some()).count();
    Some(HourlyQuote {
        buy,
        sell,
        margin: sell as f64 - buy as f64 - ge_tax(sell, name) as f64,
        liquidity: median(thin),
        active: both as f64 / rule.lookback.max(1) as f64,
    })
}

/// The plan's filters: both sides active, worth the tax, inside the app's price and ROI limits.
pub fn tradable(q: &HourlyQuote, rule: &HourlyRule) -> bool {
    let buy = q.buy as f64;
    q.active >= MIN_ACTIVE && q.buy >= MIN_BUY && q.margin >= rule.min_margin * buy && q.margin <= MAX_ROI * buy
}

/// Units one round trip can take at `alloc` gp: the allocation, the GE limit and VOLUME_SHARE of
/// the liquidity. The rank key is margin x capacity.
pub fn capacity(q: &HourlyQuote, ge_limit: Option<i32>, alloc: f64) -> i64 {
    order_qty(q.buy, ge_limit, alloc).min((VOLUME_SHARE * q.liquidity).floor() as i64)
}

/// Buckets ending at the signal bucket that `daily_pick` reads for `rule`: its lookback, and the
/// two 72-bucket trend windows of health check (b). The app loads only these.
pub fn window_hours(rule: &HourlyRule) -> i64 {
    rule.lookback.max(if rule.health { 144 } else { 0 })
}

/// The rule's quote for an item on signal day `day` when it passes every filter, from buckets up
/// to the signal bucket only. `first_hour` is the item's first stored bucket, so `hours` may be
/// just the last `window_hours(rule)` buckets. `crashed` tells whether the guide-price model flags
/// the item crashing or crashed (health check (c)); only health rules ask.
pub fn daily_pick(
    hours: &[RtHour],
    first_hour: Option<i64>,
    day: i64,
    rule: &HourlyRule,
    name: &str,
    crashed: impl FnOnce() -> bool,
) -> Option<HourlyQuote> {
    let h = signal_bucket(day);
    let q = hourly_quote(hours, h, rule, name)?;
    if !tradable(&q, rule) || (rule.health && (!healthy(hours, first_hour, h, &q, name) || crashed())) {
        return None;
    }
    Some(q)
}

/// The real-time market-health checks (a), (b) and (d) of docs/strategy-plan.md as of bucket `h`.
/// The guide-price crash flag (c) is checked by the caller.
pub fn healthy(hours: &[RtHour], first_hour: Option<i64>, h: i64, q: &HourlyQuote, name: &str) -> bool {
    // (a) traded at least 7 days before the signal
    let old = first_hour.is_some_and(|first| first <= h - 168);
    // (b) the instant-sell median over the last 3 days is at least 95% of the 3 days before
    let lows = |end: i64| hour_window(hours, end, 72).iter().filter_map(|b| b.low).collect::<Vec<f64>>();
    let (recent, before) = (lows(h), lows(h - 72));
    let steady = !recent.is_empty() && !before.is_empty() && median(recent) >= 0.95 * median(before);
    // (d) the last day's hourly margins kept at least half the quote margin
    let margins: Vec<f64> = hour_window(hours, h, 24)
        .iter()
        .filter_map(|b| {
            let (low, high) = (b.low?, b.high?);
            Some(high - low - ge_tax(high.round() as i64, name) as f64)
        })
        .collect();
    let holding = !margins.is_empty() && median(margins) >= 0.5 * q.margin;
    old && steady && holding
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 500_000;

    fn both(hour: i64, low: f64, high: f64, volume: f64) -> RtHour {
        RtHour { hour, high: Some(high), low: Some(low), high_volume: volume, low_volume: volume }
    }

    /// Two weeks of buckets ending at H where both sides trade every hour.
    fn steady(low: impl Fn(i64) -> f64, high: impl Fn(i64) -> f64) -> Vec<RtHour> {
        (H - 335..=H).map(|h| both(h, low(h), high(h), 500.0)).collect()
    }

    #[test]
    fn quotes_use_the_lookback_only() {
        let mut hours = steady(|h| 1000.0 + (h % 3) as f64, |h| 1100.0 + (h % 5) as f64);
        let flip = RULES[0];  // median, 24 buckets
        let q = hourly_quote(&hours, H, &flip, "Coal").unwrap();
        assert_eq!((q.buy, q.sell, q.active, q.liquidity), (1001, 1102, 1.0, 500.0));
        assert_eq!(q.margin, 1102.0 - 1001.0 - 22.0);
        // Buckets after the signal bucket, or before the lookback, change nothing
        hours.extend((H + 1..H + 30).map(|h| both(h, 1.0, 1e9, 1e9)));
        hours[0] = both(H - 335, 1.0, 1e9, 1e9);
        assert_eq!(hourly_quote(&hours, H, &flip, "Coal"), Some(q));
    }

    #[test]
    fn quote_rules_and_filters() {
        // Lows alternate 990 / 1010 (mean 1000, sample sd just over 10), highs 1100
        let hours = steady(|h| if h % 2 == 0 { 990.0 } else { 1010.0 }, |_| 1100.0);
        let dip = RULES[8];  // k = 1 over 168 buckets
        let q = hourly_quote(&hours, H, &dip, "Coal").unwrap();
        assert_eq!((q.buy, q.sell), (990, 1000));
        assert!(!tradable(&q, &dip));  // 1000 - 990 - 20 tax loses money
        let cons = RULES[10];
        let q = hourly_quote(&hours, H, &cons, "Coal").unwrap();
        assert_eq!((q.buy, q.sell), (990, 1100));
        assert!(tradable(&q, &cons));
        // Thin or one-sided markets: activity below 75%, and liquidity counts missing buckets as 0
        let sparse: Vec<RtHour> = hours.iter().copied().filter(|b| b.hour % 2 == 0).collect();
        let q = hourly_quote(&sparse, H, &RULES[0], "Coal").unwrap();
        assert_eq!((q.active, q.liquidity), (0.5, 250.0));
        assert!(!tradable(&q, &RULES[0]));
        // The app's limits: buy price under 100 gp, ROI over 150%
        let cheap = steady(|_| 60.0, |_| 90.0);
        assert!(!tradable(&hourly_quote(&cheap, H, &RULES[0], "Coal").unwrap(), &RULES[0]));
        let wild = steady(|_| 1000.0, |_| 2600.0);
        assert!(!tradable(&hourly_quote(&wild, H, &RULES[0], "Coal").unwrap(), &RULES[0]));
        // Capacity: allocation, GE limit and 10% of the liquidity (500 a bucket)
        let q = hourly_quote(&hours, H, &RULES[0], "Coal").unwrap();
        assert_eq!(capacity(&q, None, 1e9), 50);
        assert_eq!(capacity(&q, Some(20), 1e9), 20);
        assert_eq!(capacity(&q, None, 10_000.0), 10);
    }

    fn first(hours: &[RtHour]) -> Option<i64> {
        hours.first().map(|b| b.hour)
    }

    #[test]
    fn health_checks() {
        let flat = steady(|_| 1000.0, |_| 1100.0);
        let q = hourly_quote(&flat, H, &RULES[4], "Coal").unwrap();
        assert!(healthy(&flat, first(&flat), H, &q, "Coal"));
        // (a) a new item: first traded 6 days before the signal
        let young: Vec<RtHour> = flat.iter().copied().filter(|b| b.hour > H - 144).collect();
        assert!(!healthy(&young, first(&young), H, &q, "Coal"));
        // (b) the last 3 days 6% below the 3 days before
        let falling = steady(|h| if h > H - 72 { 940.0 } else { 1000.0 }, |_| 1100.0);
        assert!(!healthy(&falling, first(&falling), H, &q, "Coal"));
        // (d) the last day's margins collapsed to under half the quote margin
        let squeezed = steady(|_| 1000.0, |h| if h > H - 24 { 1050.0 } else { 1100.0 });
        assert!(!healthy(&squeezed, first(&squeezed), H, &q, "Coal"));
        assert_eq!(signal_bucket(20_000), 20_000 * 24 - 1);
    }

    #[test]
    fn app_rule_is_the_selected_candidate() {
        assert_eq!(APP_RULE.name, "cons24-h");  // docs/strategy.md, "Selection"
    }

    #[test]
    fn order_size_and_capacity_match_the_ui() {
        // Shared with ui/lib/format.check.ts: the Strategy view's offer size and rank capacity
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Case {
            budget: f64,
            buy: i64,
            ge_limit: Option<i32>,
            liquidity: f64,
            qty: i64,
            capacity: i64,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!("../../ui/lib/strategy-cases.json")).unwrap();
        for c in &cases {
            let alloc = c.budget / PICKS as f64;
            let q = HourlyQuote { buy: c.buy, sell: 0, margin: 0.0, liquidity: c.liquidity, active: 1.0 };
            assert_eq!((order_qty(c.buy, c.ge_limit, alloc), capacity(&q, c.ge_limit, alloc)), (c.qty, c.capacity), "buy {}", c.buy);
        }
    }

    #[test]
    fn picks_from_the_app_window_match_the_full_series() {
        // 20 days of a noisy two-sided market with a missing bucket every 11 hours. The item
        // turns 7 days old during the tested days, so health check (a) flips too.
        let d0 = 20_000;
        let hours: Vec<RtHour> = ((d0 - 20) * 24..(d0 + 2) * 24)
            .filter(|h| h % 11 != 0)
            .map(|h| both(h, 1000.0 + ((h * 37) % 120) as f64 - 60.0, 1100.0 + ((h * 53) % 80) as f64, 500.0))
            .collect();
        let mut picked = [0; RULES.len()];
        for (ri, rule) in RULES.iter().enumerate() {
            for day in d0 - 15..=d0 + 1 {
                let full = daily_pick(&hours, first(&hours), day, rule, "Coal", || false);
                // What the app loads: buckets from window_hours(rule) before the signal bucket on
                let from = signal_bucket(day) - window_hours(rule) + 1;
                let window = &hours[hours.partition_point(|b| b.hour < from)..];
                assert_eq!(daily_pick(window, first(&hours), day, rule, "Coal", || false), full, "{} on day {}", rule.name, day);
                picked[ri] += usize::from(full.is_some());
            }
        }
        // Not vacuous: every rule picks on some days, health rules only once the item is 7 days old
        assert!(picked.iter().all(|&n| n > 0), "{:?}", picked);
        assert!(RULES.iter().zip(picked).all(|(r, n)| !r.health || n < 17), "{:?}", picked);
    }
}

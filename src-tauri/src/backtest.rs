//! Walk-forward backtest of the flip scoring (method and limitations: docs/backtest.md).
//!
//! A signal for evaluation day `t` only sees rows dated on or before `t` (the app's 90-day window
//! ending at `t`). Its trade is simulated on days t+1 ..= t+HORIZON, with the daily guide price or
//! the real-time averages as the fill proxy.

use crate::commands::tier_code;
use crate::flips::{analyze, ge_tax, live_estimate, live_quote, median, track_record};
use crate::loader::{RtRow, load_all_snapshots, load_rt};
use crate::model::{ItemSnapshot, Risk};
use crate::stats::{day_number, item_stats};
use crate::trade::{rt_path, rt_window};
use std::collections::HashMap;
use std::path::Path;

/// Same window as the app's scan: rows dated within 90 days of the evaluation day.
pub const WINDOW_DAYS: i64 = 90;
/// An item counts as live when it has real-time data in the last 7 days.
pub const RT_LOOKBACK: i64 = 7;
pub const TIERS: [&str; 5] = ["S", "A", "B", "C", "X"];

// ---------------------------------------------------------------- data

pub use crate::model::RtDay;
pub use crate::trade::{HORIZON, Order, Outcome, PathDay, VOLUME_SHARE, order_qty, simulate};

/// One item's full history in chronological order.
pub struct Series {
    pub item_id: i32,
    pub name: String,
    pub ge_limit: Option<i32>,
    pub snaps: Vec<ItemSnapshot>,
    pub days: Vec<i64>,  // day number of each snapshot
    pub rt: Vec<RtDay>,
}

impl Series {
    /// Guide rows dated in [t - WINDOW_DAYS, t], like the app's scan on day t.
    pub fn window(&self, t: i64) -> &[ItemSnapshot] {
        let lo = self.days.partition_point(|&d| d < t - WINDOW_DAYS);
        let hi = self.days.partition_point(|&d| d <= t);
        &self.snaps[lo..hi]
    }

    /// Real-time days dated in [t - days + 1, t].
    pub fn rt_window(&self, t: i64, days: i64) -> &[RtDay] {
        rt_window(&self.rt, t, days)
    }

    /// Guide-price change from day t to t+HORIZON, if there is a row after t.
    fn forward_return(&self, t: i64) -> Option<f64> {
        let now = self.days.partition_point(|&d| d <= t).checked_sub(1)?;
        let later = self.days.partition_point(|&d| d <= t + HORIZON) - 1;
        (later > now).then(|| self.snaps[later].price as f64 / self.snaps[now].price as f64 - 1.0)
    }
}

/// Groups rows ordered by item then date into series. Returns the series and the number of rows
/// dropped for unreadable dates.
pub fn build_series(snaps: Vec<ItemSnapshot>, rt: Vec<RtRow>) -> (Vec<Series>, usize) {
    let mut out: Vec<Series> = Vec::new();
    let mut bad = 0;
    for snap in snaps {
        let Some(day) = day_number(&snap.record_date) else {
            bad += 1;
            continue;
        };
        if out.last().is_none_or(|s| s.item_id != snap.item_id) {
            out.push(Series {
                item_id: snap.item_id,
                name: snap.name.clone(),
                ge_limit: snap.ge_limit,
                snaps: Vec::new(),
                days: Vec::new(),
                rt: Vec::new(),
            });
        }
        let series = out.last_mut().unwrap();
        series.days.push(day);
        series.snaps.push(snap);
    }
    let index: HashMap<i32, usize> = out.iter().enumerate().map(|(i, s)| (s.item_id, i)).collect();
    for r in rt {
        let Some(&i) = index.get(&r.item_id) else { continue };
        let Some(day) = r.rt_day() else {
            bad += 1;
            continue;
        };
        out[i].rt.push(day);
    }
    (out, bad)
}

/// Coverage of the loaded data, for the report.
pub struct Coverage {
    pub items: usize,
    pub history_rows: usize,
    pub history_days: (i64, i64),
    pub rt_items: usize,
    pub rt_rows: usize,
    pub rt_days: Option<(i64, i64)>,
    pub skipped_rows: usize,
}

/// Loads every history and real-time row (read-only).
pub fn load(db: &Path) -> Result<(Vec<Series>, Coverage), String> {
    let (snaps, skipped_history) = load_all_snapshots(db).map_err(|e| format!("history: {}", e))?;
    let (rt, skipped_rt) = load_rt(db, None).map_err(|e| format!("rt_daily: {}", e))?;
    let (history_rows, rt_rows) = (snaps.len(), rt.len());
    let (series, bad) = build_series(snaps, rt);
    let days = series.iter().flat_map(|s| [s.days.first(), s.days.last()]).flatten();
    let history_days = (days.clone().copied().min().unwrap_or(0), days.copied().max().unwrap_or(0));
    let rt_all = series.iter().flat_map(|s| s.rt.iter().map(|r| r.day));
    let rt_days = rt_all.clone().min().zip(rt_all.max());
    let coverage = Coverage {
        items: series.len(),
        history_rows,
        history_days,
        rt_items: series.iter().filter(|s| !s.rt.is_empty()).count(),
        rt_rows,
        rt_days,
        skipped_rows: skipped_history + skipped_rt + bad,
    };
    Ok((series, coverage))
}

/// Day number of a "yyyy-mm-dd" date.
pub fn parse_date(date: &str) -> Option<i64> {
    day_number(date)
}

/// The database the app would open (current directory, executable directory, repo root).
pub fn default_db() -> Result<std::path::PathBuf, String> {
    crate::commands::find_db().map_err(|e| format!("{} (looked in {})", e.message, e.searched.join(", ")))
}

/// "yyyy-mm-dd" for a day number (inverse of stats::day_number).
pub fn date_string(day: i64) -> String {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

// ---------------------------------------------------------------- signals

/// What a strategy says about one item as of one day.
#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub score: f64,  // budget-independent score; negative rows are hidden
    pub tier: &'static str,
    pub buy: i32,  // bid
    pub sell: i32,  // ask
    pub roi: f64,
    pub volume: f64,  // 90-day average guide volume, checked by the min-volume filter
    pub liquidity: f64,  // units/day the order size is capped from (VOLUME_SHARE of it)
    pub crash: bool,  // crashing or crashed flag
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Strategy {
    /// The app's scoring: flips::analyze over the 90-day guide-price window.
    Baseline,
    /// Bid/ask at the `lo`/`hi` percentiles of the last `days` guide prices, ranked by the
    /// expected profit of one flip at the budget.
    GuideBand { days: i64, lo: u8, hi: u8 },
    /// Real-time quote (flips::live_quote) over the last `days` days, ranked by the expected
    /// profit of one flip at the budget. `avoid_crash` drops crash-flagged items.
    Live { days: i64, avoid_crash: bool },
    /// The live quote, scored by the item's own mean realised profit/day per unit over the
    /// signals of the last `lookback` days whose outcomes closed by t. `lcb` subtracts one
    /// standard error, penalising items with erratic or short records.
    Empirical { days: i64, lookback: i64, lcb: bool },
    /// The baseline's guide quote, scored like Empirical but from guide-fill outcomes.
    EmpiricalGuide { lookback: i64, lcb: bool },
}

impl Strategy {
    pub fn name(&self) -> String {
        match self {
            Strategy::Baseline => "baseline".to_string(),
            Strategy::GuideBand { days, lo, hi } => format!("band-{}d-q{}-q{}", days, lo, hi),
            Strategy::Live { days, avoid_crash } => format!("live-{}d{}", days, if *avoid_crash { "-nocrash" } else { "" }),
            Strategy::Empirical { days, lookback, lcb } => {
                format!("emp-{}d-{}{}", days, lookback, if *lcb { "-lcb" } else { "" })
            }
            Strategy::EmpiricalGuide { lookback, lcb } => {
                format!("emp-guide-{}{}", lookback, if *lcb { "-lcb" } else { "" })
            }
        }
    }

    /// The signal from data dated on or before `t` only.
    pub fn signal(&self, s: &Series, t: i64) -> Option<Signal> {
        match *self {
            Strategy::Baseline => baseline_signal(s, t),
            Strategy::GuideBand { days, lo, hi } => band_signal(s, t, days, lo, hi),
            Strategy::Live { days, avoid_crash } => live_signal(s, t, days, avoid_crash),
            Strategy::Empirical { days, lookback, lcb } => empirical_signal(s, t, days, lookback, lcb),
            Strategy::EmpiricalGuide { lookback, lcb } => empirical_guide_signal(s, t, lookback, lcb),
        }
    }

    /// Units to order with `alloc` gp. The baseline keeps the app's current rule (budget and GE
    /// limit); the others also cap at VOLUME_SHARE of the daily liquidity.
    pub fn qty(&self, sig: &Signal, ge_limit: Option<i32>, alloc: f64) -> i64 {
        let qty = order_qty(sig.buy, ge_limit, alloc);
        match self {
            Strategy::Baseline => qty,
            _ => qty.min((VOLUME_SHARE * sig.liquidity).floor() as i64),
        }
    }

    /// The key the table sorts by (highest first) for an order of `qty` units.
    pub fn rank(&self, sig: &Signal, qty: i64) -> f64 {
        match self {
            Strategy::Baseline => sig.score,
            _ => sig.score * qty as f64,  // score is per unit
        }
    }
}

fn baseline_signal(s: &Series, t: i64) -> Option<Signal> {
    let window = s.window(t);
    if window.is_empty() {
        return None;
    }
    let refs: Vec<&ItemSnapshot> = window.iter().collect();
    let f = analyze(&item_stats(&refs));
    Some(Signal {
        score: f.score as f64,
        tier: tier_code(&f.tier),
        buy: f.buy,
        sell: f.sell,
        roi: f.roi,
        volume: f.avg_volume,
        liquidity: f.avg_volume,
        crash: matches!(f.risk, Risk::Crashing | Risk::Crashed),
    })
}

/// Tier from per-unit margin and ROI, with the app's current thresholds.
fn margin_tier(margin: f64, roi: f64) -> &'static str {
    if margin < 0.0 {
        "X"
    } else if roi > 35.0 || margin > 5_000_000.0 {
        "S"
    } else if roi > 20.0 || margin > 1_000_000.0 {
        "A"
    } else if roi > 8.0 || margin > 200_000.0 {
        "B"
    } else {
        "C"
    }
}

/// Tier from expected profit per day as a share of the buy price (budget-independent).
pub fn rate_tier(unit_day: f64, buy: i32) -> &'static str {
    let rate = if buy > 0 { unit_day / f64::from(buy) } else { 0.0 };
    if rate < 0.0 {
        "X"
    } else if rate >= 0.01 {
        "S"
    } else if rate >= 0.005 {
        "A"
    } else if rate >= 0.002 {
        "B"
    } else {
        "C"
    }
}

fn margin_signal(base: Signal, buy: i32, sell: i32, margin: f64, liquidity: f64) -> Signal {
    let roi = if buy > 0 { margin / f64::from(buy) * 100.0 } else { 0.0 };
    Signal { score: margin, tier: margin_tier(margin, roi), buy, sell, roi, liquidity, ..base }
}

fn band_signal(s: &Series, t: i64, days: i64, lo: u8, hi: u8) -> Option<Signal> {
    let base = baseline_signal(s, t)?;
    let from = s.days.partition_point(|&d| d <= t - days);
    let to = s.days.partition_point(|&d| d <= t);
    let mut recent: Vec<f64> = s.snaps[from..to].iter().map(|x| x.price as f64).collect();
    if recent.is_empty() {
        return None;
    }
    recent.sort_by(f64::total_cmp);
    let q = |p: u8| recent[((recent.len() - 1) as f64 * f64::from(p) / 100.0).round() as usize].round() as i32;
    let (buy, sell) = (q(lo), q(hi));
    let margin = f64::from(sell) - f64::from(buy) - f64::from(ge_tax(sell, &s.name));
    let volume = base.volume;
    Some(margin_signal(base, buy, sell, margin, volume))
}

fn live_signal(s: &Series, t: i64, days: i64, avoid_crash: bool) -> Option<Signal> {
    let quote = live_quote(s.rt_window(t, days), &s.name)?;
    let base = baseline_signal(s, t)?;
    if avoid_crash && base.crash {
        return None;
    }
    Some(margin_signal(base, quote.buy, quote.sell, quote.margin, quote.volume))
}

/// Mean realised profit/day of one unit when a rule was applied on each day in
/// [t - lookback, t - HORIZON - 1], so every one of those outcomes is known by day t. With `lcb`
/// one standard error is subtracted. None with fewer than two past signals.
fn empirical_score(
    s: &Series,
    t: i64,
    lookback: i64,
    lcb: bool,
    mode: FillMode,
    quote: impl Fn(i64) -> Option<(i32, i32)>,
) -> Option<f64> {
    let past: Vec<f64> = (t - lookback..t - HORIZON)
        .filter_map(|tp| {
            let (buy, sell) = quote(tp)?;
            let order = Order { buy, sell, qty: 1, name: &s.name };
            Some(simulate(&order, tp, &forward_path(s, tp, mode), 1.0).per_day())
        })
        .collect();
    track_record(&past, lcb)
}

fn empirical_signal(s: &Series, t: i64, days: i64, lookback: i64, lcb: bool) -> Option<Signal> {
    let est = live_estimate(&s.rt, &s.name, t, days, lookback, lcb)?;
    let score = est.unit_day?;
    let base = baseline_signal(s, t)?;
    let q = est.quote;
    let mut sig = margin_signal(base, q.buy, q.sell, q.margin, q.volume);
    sig.score = score;
    sig.tier = rate_tier(score, q.buy);
    Some(sig)
}

/// The baseline's guide quote, scored by its own past outcomes under guide fills.
fn empirical_guide_signal(s: &Series, t: i64, lookback: i64, lcb: bool) -> Option<Signal> {
    let base = baseline_signal(s, t)?;
    let score = empirical_score(s, t, lookback, lcb, FillMode::Guide, |tp| baseline_signal(s, tp).map(|b| (b.buy, b.sell)))?;
    let margin = f64::from(base.sell) - f64::from(base.buy) - f64::from(ge_tax(base.sell, &s.name));
    let (buy, sell, volume) = (base.buy, base.sell, base.volume);
    let mut sig = margin_signal(base, buy, sell, margin, volume);
    sig.score = score;
    Some(sig)
}

// ---------------------------------------------------------------- simulation

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillMode {
    /// Daily guide price for every side, guide volume.
    Guide,
    /// Real-time averages: buys need the instant-sell (low) average at or below the bid, sells
    /// the instant-buy (high) average at or above the ask; forced exits at the low average.
    Rt,
}

/// Days t+1 ..= t+HORIZON of the fill proxy (days without data are absent).
pub fn forward_path(s: &Series, t: i64, mode: FillMode) -> Vec<PathDay> {
    match mode {
        FillMode::Guide => {
            let lo = s.days.partition_point(|&d| d <= t);
            let hi = s.days.partition_point(|&d| d <= t + HORIZON);
            (lo..hi)
                .map(|i| {
                    let px = Some(s.snaps[i].price as f64);
                    let volume = s.snaps[i].volume as f64;
                    PathDay { day: s.days[i], buy_px: px, sell_px: px, exit_px: px, buy_volume: volume, sell_volume: volume }
                })
                .collect()
        }
        FillMode::Rt => rt_path(&s.rt, t),
    }
}

// ---------------------------------------------------------------- evaluation

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Filters {
    pub min_volume: f64,
    pub max_roi: f64,
    pub min_buy: i32,
}

/// The app's default filters (ui/lib/flips.ts); negative scores are hidden too.
pub const APP_FILTERS: Filters = Filters { min_volume: 500.0, max_roi: 150.0, min_buy: 100 };

pub struct EvalConfig {
    pub budgets: Vec<f64>,
    pub ks: Vec<usize>,
    pub vol_share: f64,
    pub filters: Filters,
    pub mode: FillMode,
    pub dates: Vec<i64>,
    pub live_only: bool,  // only items with real-time data in the last RT_LOOKBACK days
}

/// A top-K portfolio opened on one day, budget split evenly across the picks.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cohort {
    pub pnl: f64,
    pub rate: f64,  // sum of per-pick profit/day
    pub picks: usize,
    pub filled: usize,
    pub hits: usize,
    pub units: i64,
    pub forced: i64,  // units dumped at the horizon
}

/// Outcome totals over a set of trades.
#[derive(Debug, Clone, Copy, Default)]
pub struct Acc {
    pub n: usize,
    pub filled: usize,
    pub hits: usize,
    pub rate: f64,
    pub profit: f64,
    pub cost: f64,  // gp spent on filled buys
}

impl Acc {
    fn add(&mut self, o: &Outcome, buy: i32) {
        self.n += 1;
        self.rate += o.per_day();
        self.profit += o.profit;
        self.cost += o.qty as f64 * f64::from(buy);
        if o.qty > 0 {
            self.filled += 1;
            self.hits += usize::from(o.profit > 0.0);
        }
    }

    fn merge(&mut self, o: &Acc) {
        self.n += o.n;
        self.filled += o.filled;
        self.hits += o.hits;
        self.rate += o.rate;
        self.profit += o.profit;
        self.cost += o.cost;
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FallAcc {
    pub n: usize,
    pub fell: usize,
}

/// One strategy at one budget on one evaluation day.
#[derive(Debug, Clone, Default)]
pub struct DayResult {
    pub day: i64,
    pub candidates: usize,
    pub cohorts: Vec<Cohort>,  // one per K
    pub ic: Option<f64>,  // Spearman of rank key vs realised profit/day, all candidates
    pub tiers: [Acc; 5],  // all candidates at the full budget, by tier (TIERS order)
    pub all: Acc,
    pub crash: [FallAcc; 2],  // [flagged, not flagged], items passing every filter but the score
}

/// Results indexed [strategy][budget][date].
pub fn evaluate(series: &[Series], strategies: &[Strategy], cfg: &EvalConfig) -> Vec<Vec<Vec<DayResult>>> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = cfg.dates.len().div_ceil(threads).max(1);
    let mut by_date: Vec<Vec<Vec<DayResult>>> = Vec::with_capacity(cfg.dates.len());
    std::thread::scope(|scope| {
        let handles: Vec<_> = cfg
            .dates
            .chunks(chunk)
            .map(|days| {
                scope.spawn(move || days.iter().map(|&t| evaluate_day(series, strategies, cfg, t)).collect::<Vec<_>>())
            })
            .collect();
        for h in handles {
            by_date.extend(h.join().expect("evaluation thread panicked"));
        }
    });
    (0..strategies.len())
        .map(|si| (0..cfg.budgets.len()).map(|bi| by_date.iter().map(|d| d[si][bi].clone()).collect()).collect())
        .collect()
}

fn evaluate_day(series: &[Series], strategies: &[Strategy], cfg: &EvalConfig, t: i64) -> Vec<Vec<DayResult>> {
    strategies
        .iter()
        .map(|strategy| {
            let signals: Vec<(usize, Signal)> = series
                .iter()
                .enumerate()
                .filter(|(_, s)| !cfg.live_only || !s.rt_window(t, RT_LOOKBACK).is_empty())
                .filter_map(|(i, s)| strategy.signal(s, t).map(|sig| (i, sig)))
                .collect();
            cfg.budgets.iter().map(|&budget| evaluate_budget(series, strategy, cfg, t, budget, &signals)).collect()
        })
        .collect()
}

fn evaluate_budget(
    series: &[Series],
    strategy: &Strategy,
    cfg: &EvalConfig,
    t: i64,
    budget: f64,
    signals: &[(usize, Signal)],
) -> DayResult {
    let f = &cfg.filters;
    let mut r = DayResult { day: t, ..Default::default() };
    let mut cands: Vec<(f64, usize, &Signal, i64)> = Vec::new();  // (rank key, series, signal, qty)
    for (i, sig) in signals {
        let s = &series[*i];
        let qty = strategy.qty(sig, s.ge_limit, budget);
        if qty <= 0 || sig.volume < f.min_volume || sig.roi > f.max_roi || sig.buy < f.min_buy {
            continue;
        }
        if let Some(ret) = s.forward_return(t) {
            let acc = &mut r.crash[usize::from(!sig.crash)];
            acc.n += 1;
            acc.fell += usize::from(ret < 0.0);
        }
        if sig.score >= 0.0 {
            cands.push((strategy.rank(sig, qty), *i, sig, qty));
        }
    }
    // Highest key first; the item id breaks ties so runs are reproducible
    cands.sort_by(|a, b| b.0.total_cmp(&a.0).then(series[a.1].item_id.cmp(&series[b.1].item_id)));
    r.candidates = cands.len();

    let trade = |i: usize, sig: &Signal, qty: i64| {
        let s = &series[i];
        simulate(&Order { buy: sig.buy, sell: sig.sell, qty, name: &s.name }, t, &forward_path(s, t, cfg.mode), cfg.vol_share)
    };
    let mut keys = Vec::with_capacity(cands.len());
    let mut rates = Vec::with_capacity(cands.len());
    for &(key, i, sig, qty) in &cands {
        let o = trade(i, sig, qty);
        keys.push(key);
        rates.push(o.per_day());
        r.all.add(&o, sig.buy);
        if let Some(ti) = TIERS.iter().position(|x| *x == sig.tier) {
            r.tiers[ti].add(&o, sig.buy);
        }
    }
    r.ic = spearman(&keys, &rates);

    r.cohorts = cfg
        .ks
        .iter()
        .map(|&k| {
            let alloc = budget / k as f64;
            let mut c = Cohort::default();
            for &(_, i, sig, _) in &cands {
                if c.picks == k {
                    break;
                }
                let qty = strategy.qty(sig, series[i].ge_limit, alloc);
                if qty <= 0 {
                    continue;  // unaffordable at this slot size; the next item takes the slot
                }
                let o = trade(i, sig, qty);
                c.picks += 1;
                c.pnl += o.profit;
                c.rate += o.per_day();
                if o.qty > 0 {
                    c.filled += 1;
                    c.hits += usize::from(o.profit > 0.0);
                    c.units += o.qty;
                    c.forced += o.forced;
                }
            }
            c
        })
        .collect();
    r
}

// ---------------------------------------------------------------- statistics

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() { f64::NAN } else { v.iter().sum::<f64>() / v.len() as f64 }
}

/// mean / sample standard deviation; 0 when there is no spread.
fn sharpe(v: &[f64]) -> Option<f64> {
    if v.len() < 2 {
        return None;
    }
    let m = mean(v);
    let sd = (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len() - 1) as f64).sqrt();
    Some(if sd > 0.0 { m / sd } else { 0.0 })
}

fn max_drawdown(pnl: impl Iterator<Item = f64>) -> f64 {
    let (mut equity, mut peak, mut worst) = (0.0_f64, 0.0_f64, 0.0_f64);
    for x in pnl {
        equity += x;
        peak = peak.max(equity);
        worst = worst.max(peak - equity);
    }
    worst
}

/// Splits a daily series into `stride` interleaved phases: each phase is a run of
/// non-overlapping cohorts (a new one every HORIZON days).
fn phases(v: &[f64], stride: usize) -> Vec<Vec<f64>> {
    (0..stride.min(v.len())).map(|p| v[p..].iter().step_by(stride).copied().collect()).collect()
}

fn phase_sharpe(v: &[f64], stride: usize) -> f64 {
    let s: Vec<f64> = phases(v, stride).iter().filter_map(|p| sharpe(p)).collect();
    mean(&s)
}

fn ranks(v: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..v.len()).collect();
    idx.sort_by(|&a, &b| v[a].total_cmp(&v[b]));
    let mut out = vec![0.0; v.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && v[idx[j + 1]] == v[idx[i]] {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0;  // ties share their average rank
        for &k in &idx[i..=j] {
            out[k] = avg;
        }
        i = j + 1;
    }
    out
}

/// Spearman rank correlation; None when either side has no spread or there are < 3 points.
pub fn spearman(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() < 3 || x.len() != y.len() {
        return None;
    }
    let (rx, ry) = (ranks(x), ranks(y));
    let (mx, my) = (mean(&rx), mean(&ry));
    let (mut cov, mut vx, mut vy) = (0.0, 0.0, 0.0);
    for (a, b) in rx.iter().zip(&ry) {
        cov += (a - mx) * (b - my);
        vx += (a - mx).powi(2);
        vy += (b - my).powi(2);
    }
    (vx > 0.0 && vy > 0.0).then(|| cov / (vx * vy).sqrt())
}

/// xorshift64*: a fixed-seed generator so bootstrap intervals are reproducible.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) % n as u64) as usize
    }
}

/// 95% interval of `stat` over moving-block resamples of the date indices 0..n. Blocks are
/// `block` days long and land at multiples of `block`, so phases stay aligned.
fn bootstrap(n: usize, block: usize, stat: impl Fn(&[usize]) -> f64) -> (f64, f64) {
    const REPS: usize = 2000;
    if block == 0 || n < 2 * block {
        return (f64::NAN, f64::NAN);
    }
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut out = Vec::with_capacity(REPS);
    let mut idx = Vec::with_capacity(n + block);
    for _ in 0..REPS {
        idx.clear();
        while idx.len() < n {
            let start = rng.below(n - block + 1);
            idx.extend(start..start + block);
        }
        idx.truncate(n);
        let v = stat(&idx);
        if v.is_finite() {
            out.push(v);
        }
    }
    if out.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    out.sort_by(f64::total_cmp);
    (out[out.len() * 25 / 1000], out[(out.len() * 975 / 1000).min(out.len() - 1)])
}

/// Number of evaluation dates between non-overlapping cohorts.
pub fn stride(step: i64) -> usize {
    (HORIZON as usize).div_ceil(step.max(1) as usize)
}

pub struct KSummary {
    pub k: usize,
    pub pnl_day: f64,  // mean cohort P&L / HORIZON: one budget, a new cohort every HORIZON days
    pub pnl_day_ci: (f64, f64),
    pub mean_rate: f64,  // mean over dates of the summed per-trade profit / days held
    pub median_rate: f64,
    pub mean_pnl: f64,  // mean cohort P&L
    pub sharpe: f64,  // phase-averaged mean/sd of cohort P&L (non-overlapping cohorts)
    pub sharpe_ci: (f64, f64),
    pub max_drawdown: f64,  // phase-averaged max drawdown of the cumulative cohort P&L
    pub worst_drawdown: f64,
    pub picks: f64,  // mean picks per cohort
    pub fill_rate: f64,
    pub hit_rate: f64,  // profitable / filled
    pub forced_rate: f64,  // share of bought units dumped at the horizon
}

pub struct Summary {
    pub dates: usize,
    pub cohorts: usize,  // non-overlapping cohorts per phase
    pub candidates: f64,
    pub ks: Vec<KSummary>,
    pub ic: f64,
    pub ic_positive: f64,
    pub ic_dates: usize,
    pub tiers: [Acc; 5],
    pub all: Acc,
    pub crash: [FallAcc; 2],
}

fn ratio(a: usize, b: usize) -> f64 {
    if b == 0 { f64::NAN } else { a as f64 / b as f64 }
}

/// `ks` are the portfolio sizes the days were evaluated with (EvalConfig::ks).
pub fn summarize(days: &[DayResult], ks: &[usize], step: i64) -> Summary {
    let stride = stride(step);
    let ks = ks
        .iter()
        .enumerate()
        .map(|(ki, &k)| {
            let pnl: Vec<f64> = days.iter().map(|d| d.cohorts[ki].pnl).collect();
            let rate: Vec<f64> = days.iter().map(|d| d.cohorts[ki].rate).collect();
            let sum = |f: fn(&Cohort) -> usize| days.iter().map(|d| f(&d.cohorts[ki])).sum::<usize>();
            let (picks, filled) = (sum(|c| c.picks), sum(|c| c.filled));
            let drawdowns: Vec<f64> = phases(&pnl, stride).iter().map(|p| max_drawdown(p.iter().copied())).collect();
            let pick = |idx: &[usize], v: &[f64]| idx.iter().map(|&i| v[i]).collect::<Vec<_>>();
            KSummary {
                k,
                pnl_day: mean(&pnl) / HORIZON as f64,
                pnl_day_ci: bootstrap(days.len(), stride, |idx| mean(&pick(idx, &pnl)) / HORIZON as f64),
                mean_rate: mean(&rate),
                median_rate: median(rate.clone()),
                mean_pnl: mean(&pnl),
                sharpe: phase_sharpe(&pnl, stride),
                sharpe_ci: bootstrap(days.len(), stride, |idx| phase_sharpe(&pick(idx, &pnl), stride)),
                max_drawdown: mean(&drawdowns),
                worst_drawdown: drawdowns.iter().copied().fold(0.0, f64::max),
                picks: picks as f64 / days.len().max(1) as f64,
                fill_rate: ratio(filled, picks),
                hit_rate: ratio(sum(|c| c.hits), filled),
                forced_rate: days.iter().map(|d| d.cohorts[ki].forced).sum::<i64>() as f64
                    / days.iter().map(|d| d.cohorts[ki].units).sum::<i64>() as f64,
            }
        })
        .collect();
    let ics: Vec<f64> = days.iter().filter_map(|d| d.ic).collect();
    let mut tiers = [Acc::default(); 5];
    let mut all = Acc::default();
    let mut crash = [FallAcc::default(); 2];
    for d in days {
        for (t, x) in tiers.iter_mut().zip(&d.tiers) {
            t.merge(x);
        }
        all.merge(&d.all);
        for (c, x) in crash.iter_mut().zip(&d.crash) {
            c.n += x.n;
            c.fell += x.fell;
        }
    }
    Summary {
        dates: days.len(),
        cohorts: days.len() / stride.max(1),
        candidates: days.iter().map(|d| d.candidates).sum::<usize>() as f64 / days.len().max(1) as f64,
        ks,
        ic: mean(&ics),
        ic_positive: ratio(ics.iter().filter(|&&x| x > 0.0).count(), ics.len()),
        ic_dates: ics.len(),
        tiers,
        all,
        crash,
    }
}

/// Candidate minus base over the same dates, with weekly-block bootstrap intervals.
pub struct Delta {
    pub sharpe: f64,
    pub sharpe_ci: (f64, f64),
    pub pnl_day: f64,
    pub pnl_day_ci: (f64, f64),
}

pub fn compare(base: &[DayResult], cand: &[DayResult], ki: usize, step: i64) -> Delta {
    assert!(base.len() == cand.len() && base.iter().zip(cand).all(|(a, b)| a.day == b.day), "compare needs the same dates");
    let stride = stride(step);
    let col = |d: &[DayResult], f: fn(&Cohort) -> f64| d.iter().map(|x| f(&x.cohorts[ki])).collect::<Vec<f64>>();
    let (base_pnl, cand_pnl) = (col(base, |c| c.pnl), col(cand, |c| c.pnl));
    let pick = |idx: &[usize], v: &[f64]| idx.iter().map(|&i| v[i]).collect::<Vec<_>>();
    Delta {
        sharpe: phase_sharpe(&cand_pnl, stride) - phase_sharpe(&base_pnl, stride),
        sharpe_ci: bootstrap(base.len(), stride, |idx| {
            phase_sharpe(&pick(idx, &cand_pnl), stride) - phase_sharpe(&pick(idx, &base_pnl), stride)
        }),
        pnl_day: (mean(&cand_pnl) - mean(&base_pnl)) / HORIZON as f64,
        pnl_day_ci: bootstrap(base.len(), stride, |idx| {
            (mean(&pick(idx, &cand_pnl)) - mean(&pick(idx, &base_pnl))) / HORIZON as f64
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(item_id: i32, days: i64, price: impl Fn(i64) -> i32) -> Series {
        let start = day_number("2026-01-01").unwrap();
        let snaps = (0..days)
            .map(|i| ItemSnapshot {
                item_id,
                name: format!("Item {}", item_id),
                ge_limit: Some(100),
                record_date: date_string(start + i),
                price: price(i),
                volume: 5000,
            })
            .collect();
        build_series(snaps, vec![]).0.pop().unwrap()
    }

    #[test]
    fn rt_path_buys_on_low_and_sells_on_high() {
        let mut s = synthetic(1, 30, |_| 1000);
        let t = s.days[10];
        s.rt = vec![
            RtDay { day: t, high: Some(1.0), low: Some(1.0), high_volume: 9.0, low_volume: 9.0 },
            RtDay { day: t + 1, high: Some(1100.0), low: None, high_volume: 70.0, low_volume: 0.0 },
            RtDay { day: t + 8, high: Some(1.0), low: Some(1.0), high_volume: 9.0, low_volume: 9.0 },
        ];
        let path = forward_path(&s, t, FillMode::Rt);
        let sale_only =
            PathDay { day: t + 1, buy_px: None, sell_px: Some(1100.0), exit_px: None, buy_volume: 0.0, sell_volume: 70.0 };
        assert_eq!(path, vec![sale_only]);
        assert_eq!(forward_path(&s, t, FillMode::Guide).len(), HORIZON as usize);
    }

    #[test]
    fn signals_never_see_rows_after_t() {
        let mut s = synthetic(1, 120, |i| 10_000 + (i % 9) as i32 * 250 - (i / 30) as i32 * 300);
        let t = s.days[100];
        s.rt = (t - 10..=t + 5)
            .map(|d| {
                let high = Some(10_500.0 + (d % 3) as f64);
                RtDay { day: d, high, low: Some(9_800.0), high_volume: 900.0, low_volume: 800.0 }
            })
            .collect();
        let strategies = [
            Strategy::Baseline,
            Strategy::GuideBand { days: 14, lo: 25, hi: 75 },
            Strategy::Live { days: 3, avoid_crash: false },
            Strategy::Empirical { days: 3, lookback: 28, lcb: true },
            Strategy::EmpiricalGuide { lookback: 28, lcb: true },
        ];
        let before: Vec<Option<Signal>> = strategies.iter().map(|st| st.signal(&s, t)).collect();
        assert!(before.iter().all(Option::is_some));
        for (snap, &d) in s.snaps.iter_mut().zip(&s.days) {
            if d > t {
                snap.price *= 3;
                snap.volume = 1;
            }
        }
        for r in s.rt.iter_mut().filter(|r| r.day > t) {
            *r = RtDay { day: r.day, high: Some(1e6), low: Some(1.0), high_volume: 1e9, low_volume: 1e9 };
        }
        let after: Vec<Option<Signal>> = strategies.iter().map(|st| st.signal(&s, t)).collect();
        assert_eq!(after, before);
        // The window ends at t, like the app's scan on day t
        assert_eq!(s.window(t).last().map(|x| x.record_date.clone()), Some(date_string(t)));
        assert_eq!(s.window(t).len(), 91);
    }

    #[test]
    fn spearman_handles_order_and_ties() {
        assert_eq!(spearman(&[1.0, 2.0, 3.0, 4.0], &[10.0, 20.0, 30.0, 40.0]), Some(1.0));
        assert_eq!(spearman(&[1.0, 2.0, 3.0], &[3.0, 2.0, 1.0]), Some(-1.0));
        assert_eq!(spearman(&[1.0, 2.0, 3.0], &[0.0, 0.0, 0.0]), None);
        let r = spearman(&[1.0, 2.0, 3.0, 4.0], &[0.0, 0.0, 1.0, 1.0]).unwrap();
        assert!((r - 4.0 / 20f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn drawdown_phases_and_stride() {
        assert_eq!(max_drawdown([5.0, -3.0, -4.0, 10.0, -1.0].into_iter()), 7.0);
        assert_eq!(phases(&[1.0, 2.0, 3.0, 4.0, 5.0], 2), vec![vec![1.0, 3.0, 5.0], vec![2.0, 4.0]]);
        assert_eq!((stride(1), stride(3), stride(7)), (7, 3, 1));
    }

    #[test]
    fn date_string_round_trips() {
        for d in ["1970-01-01", "2000-03-01", "2024-02-29", "2026-07-24", "2026-12-31"] {
            assert_eq!(date_string(day_number(d).unwrap()), d);
        }
    }

    #[test]
    fn evaluate_runs_end_to_end() {
        // An item that swings 10,000..11,600 every 5 days fills and sells; a flat one never does
        let series = vec![synthetic(1, 130, |i| 10_000 + (i % 5) as i32 * 400), synthetic(2, 130, |_| 5_000)];
        let start = series[0].days[0];
        let cfg = EvalConfig {
            budgets: vec![50_000_000.0],
            ks: vec![1, 2],
            vol_share: 0.1,
            filters: APP_FILTERS,
            mode: FillMode::Guide,
            dates: (start + 95..=start + 115).collect(),
            live_only: false,
        };
        let results = evaluate(&series, &[Strategy::Baseline], &cfg);
        let days = &results[0][0];
        assert_eq!(days.len(), 21);
        assert!(days.iter().all(|d| d.cohorts.len() == 2 && d.cohorts[0].picks <= 1));
        let summary = summarize(days, &cfg.ks, 1);
        let (swing, flat) = (&summary.tiers[2], &summary.tiers[4]);  // B and X
        assert!(swing.filled > 0 && swing.hits == swing.filled);
        // The flat item loses the tax on every trade, yet its baseline score is positive
        assert!(flat.filled > 0 && flat.hits == 0);
        assert!(summary.ks[0].mean_rate > 0.0);  // K=1 picks the higher-scored swing item
        let delta = compare(days, days, 0, 1);
        assert_eq!((delta.sharpe, delta.pnl_day), (0.0, 0.0));
    }

    #[test]
    fn live_qty_and_exp_day_match_the_ui() {
        // Shared with ui/lib/format.check.ts: the UI's live quantity and Exp/day are this
        // strategy's order size and rank key
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Case {
            budget: f64,
            buy: i32,
            ge_limit: Option<i32>,
            volume: f64,
            unit_day: f64,
            qty: i64,
            exp_day: f64,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!("../../ui/lib/price-cases.json")).unwrap();
        let (days, lookback) = (crate::flips::LIVE_QUOTE_DAYS, crate::flips::LIVE_LOOKBACK);
        let strategy = Strategy::Empirical { days, lookback, lcb: true };
        for c in &cases {
            let sig = Signal {
                score: c.unit_day,
                tier: "S",
                buy: c.buy,
                sell: 0,
                roi: 0.0,
                volume: 0.0,
                liquidity: c.volume,
                crash: false,
            };
            let qty = strategy.qty(&sig, c.ge_limit, c.budget);
            assert_eq!((qty, strategy.rank(&sig, qty)), (c.qty, c.exp_day), "buy {} volume {}", c.buy, c.volume);
        }
    }
}

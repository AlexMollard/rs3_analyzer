//! Walk-forward backtest of the flip scoring; the method and its limits are in docs/backtest.md.
//! From src-tauri/: cargo run --release --features backtest --bin backtest -- [options]

use rs3_analyzer_lib::backtest::{
    self as bt, APP_FILTERS, Coverage, DayResult, EvalConfig, FillMode, HORIZON, Series, Strategy, Summary, TIERS,
    WINDOW_DAYS,
};
use std::fmt::Write as _;
use std::ops::RangeInclusive;
use std::path::PathBuf;

const HELP: &str = "\
Walk-forward backtest of the flip scoring (see docs/backtest.md).

Usage: cargo run --release --features backtest --bin backtest -- [options]

Options:
  --db PATH            database (default: the one the app opens)
  --budgets LIST       budgets in gp, k/m/b suffixes allowed (default 10m,50m,250m)
  --k LIST             top-K portfolio sizes (default 10,20)
  --step DAYS          days between evaluation dates (default 1)
  --vol-share F        share of a day's volume one trade may fill (default 0.10)
  --end DATE           last day of data to use (default: the day before the newest row,
                       which the daily collector stamps on the day it runs)
  --test-from DATE     first held-out signal date, guide fills (default: end - 120 days)
  --rt-test-from DATE  first held-out signal date, real-time fills (default: first rt day + 50)
  --strategies LIST    strategies to run (default: all)
  --trace ITEM_ID      print every signal and simulated trade for one item instead
                       (first budget, full budget per trade)
  --out PATH           also write the markdown report to PATH
  -h, --help           show this help
";

/// The portfolio the adoption decision is judged on (docs/backtest.md).
const ADOPT_BUDGET: f64 = 50_000_000.0;
const ADOPT_K: usize = 10;

/// Candidates, chosen before looking at results. The first of each list is the reference.
const GUIDE_STRATEGIES: &[Strategy] = &[
    Strategy::Baseline,
    Strategy::GuideBand { days: 90, lo: 10, hi: 90 },
    Strategy::GuideBand { days: 30, lo: 10, hi: 90 },
    Strategy::GuideBand { days: 14, lo: 25, hi: 75 },
    Strategy::GuideBand { days: 7, lo: 25, hi: 75 },
    Strategy::EmpiricalGuide { lookback: 28, lcb: true },
    Strategy::EmpiricalGuide { lookback: 56, lcb: true },
];
const RT_STRATEGIES: &[Strategy] = &[
    Strategy::Baseline,
    Strategy::GuideBand { days: 14, lo: 25, hi: 75 },
    Strategy::Live { days: 1, avoid_crash: false },
    Strategy::Live { days: 3, avoid_crash: false },
    Strategy::Live { days: 7, avoid_crash: false },
    Strategy::Live { days: 3, avoid_crash: true },
    Strategy::Empirical { days: 3, lookback: 28, lcb: false },
    Strategy::Empirical { days: 3, lookback: 28, lcb: true },
    Strategy::Empirical { days: 1, lookback: 28, lcb: true },
];

struct Args {
    db: Option<PathBuf>,
    budgets: Vec<f64>,
    ks: Vec<usize>,
    step: i64,
    vol_share: f64,
    end: Option<i64>,
    test_from: Option<i64>,
    rt_test_from: Option<i64>,
    strategies: Option<Vec<String>>,
    trace: Option<i32>,
    out: Option<PathBuf>,
}

fn parse_gp(s: &str) -> Option<f64> {
    let s = s.to_ascii_lowercase();
    let (num, mult) = match s.chars().last()? {
        'k' => (&s[..s.len() - 1], 1e3),
        'm' => (&s[..s.len() - 1], 1e6),
        'b' => (&s[..s.len() - 1], 1e9),
        _ => (&s[..], 1.0),
    };
    num.parse::<f64>().ok().filter(|v| *v > 0.0).map(|v| (v * mult).round())
}

fn parse_list<T>(s: &str, f: impl Fn(&str) -> Option<T>) -> Option<Vec<T>> {
    s.split(',').map(|x| f(x.trim())).collect()
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        db: None,
        budgets: vec![10_000_000.0, 50_000_000.0, 250_000_000.0],
        ks: vec![10, 20],
        step: 1,
        vol_share: 0.10,
        end: None,
        test_from: None,
        rt_test_from: None,
        strategies: None,
        trace: None,
        out: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{} needs a value", flag));
        let date = |s: String| bt::parse_date(&s).ok_or_else(|| format!("bad date {}", s));
        match flag.as_str() {
            "--db" => a.db = Some(PathBuf::from(value()?)),
            "--budgets" => a.budgets = parse_list(&value()?, parse_gp).ok_or("bad --budgets")?,
            "--k" => a.ks = parse_list(&value()?, |x| x.parse().ok().filter(|k| *k > 0)).ok_or("bad --k")?,
            "--step" => a.step = value()?.parse().ok().filter(|s| *s > 0).ok_or("bad --step")?,
            "--vol-share" => a.vol_share = value()?.parse().ok().filter(|v| *v > 0.0).ok_or("bad --vol-share")?,
            "--end" => a.end = Some(date(value()?)?),
            "--test-from" => a.test_from = Some(date(value()?)?),
            "--rt-test-from" => a.rt_test_from = Some(date(value()?)?),
            "--strategies" => a.strategies = Some(value()?.split(',').map(|s| s.trim().to_string()).collect()),
            "--trace" => a.trace = Some(value()?.parse().map_err(|_| "bad --trace")?),
            "--out" => a.out = Some(PathBuf::from(value()?)),
            "-h" | "--help" => {
                print!("{}", HELP);
                std::process::exit(0);
            }
            other => return Err(format!("unknown option {} (see --help)", other)),
        }
    }
    Ok(a)
}

fn main() {
    if let Err(e) = run() {
        eprintln!("backtest: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let db = match &args.db {
        Some(p) => p.clone(),
        None => bt::default_db()?,
    };
    let started = std::time::Instant::now();
    let (series, cov) = bt::load(&db)?;
    eprintln!(
        "Loaded {} items, {} history rows, {} real-time rows in {:.1}s",
        cov.items,
        cov.history_rows,
        cov.rt_rows,
        started.elapsed().as_secs_f64()
    );
    let end = args.end.unwrap_or(cov.history_days.1 - 1);
    let pick = |all: &[Strategy]| -> Vec<Strategy> {
        all.iter().copied().filter(|s| args.strategies.as_ref().is_none_or(|names| names.contains(&s.name()))).collect()
    };

    let mut studies = vec![(
        Study {
            title: "Guide-price fills",
            mode: FillMode::Guide,
            live_only: false,
            first: cov.history_days.0 + WINDOW_DAYS,
            last: end - HORIZON,
            test_from: args.test_from.unwrap_or(end - 120),
        },
        pick(GUIDE_STRATEGIES),
    )];
    if let Some((rt_first, rt_last)) = cov.rt_days {
        studies.push((
            Study {
                title: "Real-time fills",
                mode: FillMode::Rt,
                live_only: true,
                // From the first day every candidate can signal: the empirical ones need two
                // past signals whose outcome windows have closed
                first: rt_first + HORIZON + 2,
                last: rt_last.min(end) - HORIZON,
                test_from: args.rt_test_from.unwrap_or(rt_first + 50),
            },
            pick(RT_STRATEGIES),
        ));
    }

    let mut out = String::new();
    if let Some(id) = args.trace {
        let s = series.iter().find(|s| s.item_id == id).ok_or_else(|| format!("item {} has no history", id))?;
        trace(&mut out, s, &studies, &args);
    } else {
        header(&mut out, &db, &cov, &args, end);
        for (study, strategies) in &studies {
            study.run(&mut out, &series, strategies, &args);
        }
    }
    eprintln!("Done in {:.1}s", started.elapsed().as_secs_f64());

    print!("{}", out);
    if let Some(path) = &args.out {
        std::fs::write(path, out.replace('\n', "\r\n")).map_err(|e| format!("writing {}: {}", path.display(), e))?;
    }
    Ok(())
}

fn trace(out: &mut String, s: &Series, studies: &[(Study, Vec<Strategy>)], args: &Args) {
    let budget = args.budgets[0];
    let limit = s.ge_limit.map_or("?".to_string(), |l| l.to_string());
    let _ = writeln!(out, "# Trace: {} (id {}, GE limit {}, budget {})
", s.name, s.item_id, limit, gp(budget));
    for (study, strategies) in studies {
        let _ = writeln!(out, "## {}
", study.title);
        let _ = writeln!(out, "| Strategy | Signal date | Buy | Sell | Score | Tier | Qty | Bought | Profit | Days | Dumped |");
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|---|---|");
        for strategy in strategies {
            for t in study.first..=study.last {
                let Some(sig) = strategy.signal(s, t) else { continue };
                let qty = strategy.qty(&sig, s.ge_limit, budget);
                let order = bt::Order { buy: sig.buy, sell: sig.sell, qty, name: &s.name };
                let o = bt::simulate(&order, t, &bt::forward_path(s, t, study.mode), args.vol_share);
                let _ = writeln!(
                    out,
                    "| {} | {} | {} | {} | {:.1} | {} | {} | {} | {} | {} | {} |",
                    strategy.name(),
                    bt::date_string(t),
                    sig.buy,
                    sig.sell,
                    sig.score,
                    sig.tier,
                    qty,
                    o.qty,
                    gp(o.profit),
                    o.days,
                    o.forced
                );
            }
        }
        let _ = writeln!(out);
    }
}

fn header(out: &mut String, db: &std::path::Path, cov: &Coverage, args: &Args, end: i64) {
    let _ = writeln!(out, "# Backtest results\n");
    let _ = writeln!(out, "- Database: `{}`", db.display());
    let _ = writeln!(
        out,
        "- History: {} rows, {} items, {} to {} (using data up to {})",
        cov.history_rows,
        cov.items,
        bt::date_string(cov.history_days.0),
        bt::date_string(cov.history_days.1),
        bt::date_string(end)
    );
    match cov.rt_days {
        Some((a, b)) => {
            let _ = writeln!(
                out,
                "- Real-time: {} rows, {} items, {} to {}",
                cov.rt_rows,
                cov.rt_items,
                bt::date_string(a),
                bt::date_string(b)
            );
        }
        None => {
            let _ = writeln!(out, "- Real-time: none");
        }
    }
    let _ = writeln!(
        out,
        "- Horizon {} days, volume share {:.0}%, step {} day(s), budgets {}, K {:?}, unreadable rows skipped: {}\n",
        HORIZON,
        args.vol_share * 100.0,
        args.step,
        args.budgets.iter().map(|b| gp(*b)).collect::<Vec<_>>().join("/"),
        args.ks,
        cov.skipped_rows
    );
}

struct Study {
    title: &'static str,
    mode: FillMode,
    live_only: bool,
    first: i64,
    last: i64,
    test_from: i64,
}

impl Study {
    fn run(&self, out: &mut String, series: &[Series], strategies: &[Strategy], args: &Args) {
        let _ = writeln!(out, "## {}\n", self.title);
        let dates: Vec<i64> = (self.first..=self.last).step_by(args.step as usize).collect();
        if dates.is_empty() || strategies.is_empty() {
            let _ = writeln!(out, "Not enough data.\n");
            return;
        }
        let started = std::time::Instant::now();
        let cfg = EvalConfig {
            budgets: args.budgets.clone(),
            ks: args.ks.clone(),
            vol_share: args.vol_share,
            filters: APP_FILTERS,
            mode: self.mode,
            dates: dates.clone(),
            live_only: self.live_only,
        };
        let results = bt::evaluate(series, strategies, &cfg);
        let secs = started.elapsed().as_secs_f64();
        eprintln!("{}: {} dates x {} strategies in {:.1}s", self.title, dates.len(), strategies.len(), secs);

        // Train signals must have their whole outcome window before the test period
        let periods: [(&str, RangeInclusive<i64>); 2] =
            [("train", self.first..=self.test_from - HORIZON - 1), ("test", self.test_from..=self.last)];
        let _ = writeln!(
            out,
            "Signal dates {} to {}{}; train {} to {}, test {} to {}.\n",
            bt::date_string(self.first),
            bt::date_string(self.last),
            if self.live_only { " (items with real-time data in the last 7 days only)" } else { "" },
            bt::date_string(*periods[0].1.start()),
            bt::date_string(*periods[0].1.end()),
            bt::date_string(*periods[1].1.start()),
            bt::date_string(*periods[1].1.end())
        );
        let slice = |days: &Vec<DayResult>, range: &RangeInclusive<i64>| -> Vec<DayResult> {
            days.iter().filter(|d| range.contains(&d.day)).cloned().collect()
        };
        let summaries: Vec<Vec<Vec<Summary>>> = periods
            .iter()
            .map(|(_, range)| {
                results
                    .iter()
                    .map(|per_budget| {
                        per_budget.iter().map(|days| bt::summarize(&slice(days, range), &cfg.ks, args.step)).collect()
                    })
                    .collect()
            })
            .collect();

        let _ = writeln!(out, "### Top-K portfolios (budget split evenly over K picks)\n");
        let _ = writeln!(
            out,
            "| Period | Strategy | Budget | K | Dates | Cohorts | P&L/day [95% CI] | Sharpe [95% CI] | Max DD avg / worst | Per-trade profit/day mean / median | Fill | Hit | Forced units |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|---|---|---|---|");
        for (pi, (period, _)) in periods.iter().enumerate() {
            for (si, strategy) in strategies.iter().enumerate() {
                for (bi, budget) in cfg.budgets.iter().enumerate() {
                    let s = &summaries[pi][si][bi];
                    for k in &s.ks {
                        let _ = writeln!(
                            out,
                            "| {} | {} | {} | {} | {} | {} | {} {} | {} {} | {} / {} | {} / {} | {} | {} | {} |",
                            period,
                            strategy.name(),
                            gp(*budget),
                            k.k,
                            s.dates,
                            s.cohorts,
                            gp(k.pnl_day),
                            ci_gp(k.pnl_day_ci),
                            num(k.sharpe),
                            ci_num(k.sharpe_ci),
                            gp(k.max_drawdown),
                            gp(k.worst_drawdown),
                            gp(k.mean_rate),
                            gp(k.median_rate),
                            pct(k.fill_rate),
                            pct(k.hit_rate),
                            pct(k.forced_rate)
                        );
                    }
                }
            }
        }

        let _ = writeln!(out, "\n### Ranking quality (every candidate, full budget each)\n");
        let _ = writeln!(out, "| Period | Strategy | Budget | Candidates/day | IC mean | IC > 0 | Fill | Hit | Profit/day per candidate |");
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|");
        for (pi, (period, _)) in periods.iter().enumerate() {
            for (si, strategy) in strategies.iter().enumerate() {
                for (bi, budget) in cfg.budgets.iter().enumerate() {
                    let s = &summaries[pi][si][bi];
                    let _ = writeln!(
                        out,
                        "| {} | {} | {} | {:.0} | {} | {} of {} | {} | {} | {} |",
                        period,
                        strategy.name(),
                        gp(*budget),
                        s.candidates,
                        num(s.ic),
                        pct(s.ic_positive),
                        s.ic_dates,
                        pct(ratio(s.all.filled, s.all.n)),
                        pct(ratio(s.all.hits, s.all.filled)),
                        gp(s.all.rate / s.all.n.max(1) as f64)
                    );
                }
            }
        }

        let bi = cfg.budgets.iter().position(|b| *b == ADOPT_BUDGET).unwrap_or(0);
        let _ = writeln!(out, "\n### Tiers at {} (every candidate, full budget each)\n", gp(cfg.budgets[bi]));
        let _ = writeln!(out, "| Period | Strategy | Tier | Trades | Fill | Hit | Profit/day | Profit/trade | Return/day on cost |");
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|");
        for (pi, (period, _)) in periods.iter().enumerate() {
            for (si, strategy) in strategies.iter().enumerate() {
                let s = &summaries[pi][si][bi];
                for (tier, acc) in TIERS.iter().zip(&s.tiers) {
                    if acc.n == 0 {
                        continue;
                    }
                    let _ = writeln!(
                        out,
                        "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                        period,
                        strategy.name(),
                        tier,
                        acc.n,
                        pct(ratio(acc.filled, acc.n)),
                        pct(ratio(acc.hits, acc.filled)),
                        gp(acc.rate / acc.n as f64),
                        gp(acc.profit / acc.n as f64),
                        if acc.cost > 0.0 { format!("{:.2}%", acc.rate / acc.cost * 100.0) } else { "–".to_string() }
                    );
                }
            }
        }

        let _ = writeln!(out, "\n### Crash flags at {} (guide price {} days later)\n", gp(cfg.budgets[bi]), HORIZON);
        let _ = writeln!(out, "| Period | Strategy | Flagged | Fell | Not flagged | Fell |");
        let _ = writeln!(out, "|---|---|---|---|---|---|");
        for (pi, (period, _)) in periods.iter().enumerate() {
            for (si, strategy) in strategies.iter().enumerate() {
                let c = &summaries[pi][si][bi].crash;
                let _ = writeln!(
                    out,
                    "| {} | {} | {} | {} | {} | {} |",
                    period,
                    strategy.name(),
                    c[0].n,
                    pct(ratio(c[0].fell, c[0].n)),
                    c[1].n,
                    pct(ratio(c[1].fell, c[1].n))
                );
            }
        }

        if strategies.len() > 1
            && let Some(ki) = cfg.ks.iter().position(|k| *k == ADOPT_K)
        {
            let _ = writeln!(out, "\n### Paired difference vs {} (K={} at {})\n", strategies[0].name(), ADOPT_K, gp(cfg.budgets[bi]));
            let _ = writeln!(out, "| Period | Strategy | Sharpe diff [95% CI] | P&L/day diff [95% CI] |");
            let _ = writeln!(out, "|---|---|---|---|");
            for (period, range) in &periods {
                let base = slice(&results[0][bi], range);
                for (si, strategy) in strategies.iter().enumerate().skip(1) {
                    let d = bt::compare(&base, &slice(&results[si][bi], range), ki, args.step);
                    let _ = writeln!(
                        out,
                        "| {} | {} | {} {} | {} {} |",
                        period,
                        strategy.name(),
                        num(d.sharpe),
                        ci_num(d.sharpe_ci),
                        gp(d.pnl_day),
                        ci_gp(d.pnl_day_ci)
                    );
                }
            }
        }
        let _ = writeln!(out);
    }
}

fn ratio(a: usize, b: usize) -> f64 {
    if b == 0 { f64::NAN } else { a as f64 / b as f64 }
}

/// Same shape as the UI's formatGp.
fn gp(v: f64) -> String {
    if !v.is_finite() {
        return "–".to_string();
    }
    let a = v.abs();
    let sign = if v < 0.0 && a >= 0.5 { "-" } else { "" };
    if a >= 1e9 {
        format!("{}{:.2}B", sign, a / 1e9)
    } else if a >= 1e6 {
        format!("{}{:.2}M", sign, a / 1e6)
    } else if a >= 1e3 {
        format!("{}{:.1}K", sign, a / 1e3)
    } else {
        format!("{}{:.0}", sign, a)
    }
}

fn pct(v: f64) -> String {
    if v.is_finite() { format!("{:.1}%", v * 100.0) } else { "–".to_string() }
}

fn num(v: f64) -> String {
    if v.is_finite() { format!("{:.2}", v) } else { "–".to_string() }
}

fn ci_gp((lo, hi): (f64, f64)) -> String {
    if lo.is_finite() { format!("[{}, {}]", gp(lo), gp(hi)) } else { String::new() }
}

fn ci_num((lo, hi): (f64, f64)) -> String {
    if lo.is_finite() { format!("[{}, {}]", num(lo), num(hi)) } else { String::new() }
}

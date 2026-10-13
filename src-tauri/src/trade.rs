//! The trade rules shared by the app and the backtests: one buy offer and one sell offer at fixed
//! prices, filled against one day of data at a time (docs/backtest.md) or one hourly bucket at a
//! time (`simulate_hourly`, docs/strategy.md).

use crate::flips::ge_tax;
use crate::model::{RtDay, RtHour};

/// A buy may fill from the day after the signal, and everything is sold within HORIZON days.
pub const HORIZON: i64 = 7;
/// Share of a day's volume one order is assumed to fill (also caps the app's quantity).
pub const VOLUME_SHARE: f64 = 0.10;

/// One day of the fill proxy. Prices are None when that side did not trade.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathDay {
    pub day: i64,
    pub buy_px: Option<f64>,  // a bid at or above this fills
    pub sell_px: Option<f64>,  // an ask at or below this fills
    pub exit_px: Option<f64>,  // forced-exit price
    pub buy_volume: f64,
    pub sell_volume: f64,
}

/// A buy-then-sell trade at fixed offer prices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Order<'a> {
    pub buy: i64,
    pub sell: i64,
    pub qty: i64,  // units wanted (already capped by budget and GE limit)
    pub name: &'a str,  // for the GE tax (Bonds are exempt)
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Outcome {
    pub qty: i64,  // units bought; 0 = the bid never filled
    pub profit: f64,  // realised gp after tax
    pub days: i64,  // days from the signal to the last sale
    pub forced: i64,  // units dumped at the horizon
}

impl Outcome {
    /// Realised profit per day of capital lock-up (0 when the bid never filled).
    pub fn per_day(&self) -> f64 {
        if self.qty == 0 { 0.0 } else { self.profit / self.days.max(1) as f64 }
    }
}

/// Same rule as the app's priceRow: units the allocation affords, capped by the GE limit.
pub fn order_qty(buy: i64, ge_limit: Option<i32>, alloc: f64) -> i64 {
    if buy <= 0 {
        return 0;
    }
    let affordable = (alloc / buy as f64).trunc().clamp(0.0, i32::MAX as f64) as i64;
    ge_limit.map_or(affordable, |limit| affordable.min(i64::from(limit)))
}

fn net_of_tax(price: i64, name: &str) -> f64 {
    (price - ge_tax(price, name)) as f64
}

/// Simulates a trade signalled at the end of day `t`:
/// - the bid fills at `order.buy` on the first day in t+1 ..= t+HORIZON whose buy-side proxy is at
///   or below it, for at most `vol_share` of that day's volume;
/// - from the next day the units are offered at `order.sell`; on each day the sell-side proxy
///   reaches the ask, up to `vol_share` of that day's volume sells;
/// - whatever is left at t+HORIZON is dumped at the latest exit price.
///
/// GE tax applies to every sale.
pub fn simulate(order: &Order, t: i64, path: &[PathDay], vol_share: f64) -> Outcome {
    if order.buy <= 0 || order.qty <= 0 {
        return Outcome::default();
    }
    let end = t + HORIZON;
    let days: Vec<&PathDay> = path.iter().filter(|d| d.day > t && d.day <= end).collect();
    let bid = order.buy as f64;
    let entry = days.iter().enumerate().find_map(|(i, d)| {
        let qty = order.qty.min((vol_share * d.buy_volume).floor() as i64);
        (qty > 0 && d.buy_px.is_some_and(|p| p <= bid)).then_some((i, qty))
    });
    let Some((entry, qty)) = entry else {
        return Outcome::default();
    };

    let ask = order.sell as f64;
    let mut left = qty;
    let mut proceeds = 0.0;
    let mut last_sale = days[entry].day;
    for d in &days[entry + 1..] {
        if left == 0 {
            break;
        }
        if d.sell_px.is_some_and(|p| p >= ask) {
            let sold = left.min((vol_share * d.sell_volume).floor() as i64);
            if sold > 0 {
                proceeds += sold as f64 * net_of_tax(order.sell, order.name);
                left -= sold;
                last_sale = d.day;
            }
        }
    }
    let forced = left;
    if forced > 0 {
        // The entry day always has a price (its buy-side proxy reached the bid)
        let px = days[entry..].iter().rev().find_map(|d| d.exit_px).or(days[entry].buy_px).unwrap_or(bid);
        let px = px.round().clamp(0.0, i64::MAX as f64) as i64;
        proceeds += left as f64 * net_of_tax(px, order.name);
        last_sale = end;
    }
    Outcome { qty, profit: proceeds - qty as f64 * bid, days: (last_sale - t).max(1), forced }
}

/// Real-time days dated in [t - days + 1, t]; `rt` is chronological.
pub fn rt_window(rt: &[RtDay], t: i64, days: i64) -> &[RtDay] {
    let lo = rt.partition_point(|r| r.day <= t - days);
    let hi = rt.partition_point(|r| r.day <= t);
    &rt[lo..hi]
}

/// Days t+1 ..= t+HORIZON as a fill path: buys against the instant-sell average, sales against
/// the instant-buy average, forced exits at the instant-sell average.
pub fn rt_path(rt: &[RtDay], t: i64) -> Vec<PathDay> {
    rt_window(rt, t + HORIZON, HORIZON)
        .iter()
        .map(|r| PathDay {
            day: r.day,
            buy_px: r.low,
            sell_px: r.high,
            exit_px: r.low,
            buy_volume: r.low_volume,
            sell_volume: r.high_volume,
        })
        .collect()
}

/// GE buy limits count the units bought in the 4 hours from the first purchase.
pub const GE_LIMIT_HOURS: i64 = 4;

/// When an hourly trade may happen, in hours since 1970-01-01 UTC. The signal saw bucket `signal`,
/// so nothing fills in it or before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HourWindow {
    pub signal: i64,
    pub entry: i64,  // the bid can fill in buckets signal+1 ..= signal+entry
    pub horizon: i64,  // sales run to bucket signal+horizon, then the rest is dumped
}

/// Fill assumptions of the hourly simulator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HourlyFills {
    pub vol_share: f64,  // share of a bucket's volume one buy or one bucket's sales may take
    pub reenter: bool,  // re-place the bid after a position is sold (else one trip per order)
    pub dump_discount: f64,  // forced exits get this fraction below the instant-sell average
}

/// One round trip: a buy fill and the sales (or the dump) that closed it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Trip {
    pub buy_hour: i64,
    pub exit_hour: i64,  // bucket of the last sale or the dump
    pub qty: i64,
    pub profit: f64,  // realised gp after tax
    pub forced: i64,  // units dumped at the horizon
}

struct Position {
    at: usize,  // index of the buy bucket
    qty: i64,
    left: i64,
    proceeds: f64,
    last_sale: i64,
}

/// Simulates round trips at fixed offer prices against hourly buckets (`path` chronological; a
/// missing bucket had no trades):
/// - while no units are held, the bid fills at `order.buy` in the first bucket of the entry window
///   whose instant-sell average is at or below it. It takes at most `vol_share` of that bucket's
///   instant-sell volume, `order.qty` units, and what is left of the GE limit in the current
///   4-hour window. A bucket too thin for one unit does not fill;
/// - from the next bucket the units are offered at `order.sell`; in each bucket whose instant-buy
///   average reaches the ask, up to `vol_share` of its instant-buy volume sells;
/// - once everything is sold the bid is live again from the next bucket, inside the entry window
///   (one position at a time);
/// - whatever is left after bucket signal+horizon's sales is dumped at that bucket's instant-sell
///   average, or the latest one since the buy if it has none.
///
/// GE tax applies to every sale.
pub fn simulate_hourly(order: &Order, ge_limit: Option<i32>, window: &HourWindow, fills: &HourlyFills, path: &[RtHour]) -> Vec<Trip> {
    let mut trips = Vec::new();
    if order.buy <= 0 || order.qty <= 0 {
        return trips;
    }
    let last_entry = window.signal + window.entry.min(window.horizon);
    let end = window.signal + window.horizon;
    let bars = &path[path.partition_point(|b| b.hour <= window.signal)..path.partition_point(|b| b.hour <= end)];
    let (bid, ask) = (order.buy as f64, order.sell as f64);
    let net_ask = net_of_tax(order.sell, order.name);
    let share = |volume: f64| (fills.vol_share * volume).floor() as i64;
    let mut limit: Option<(i64, i64)> = None;  // (first buy hour, units bought) of the GE-limit window
    let mut open: Option<Position> = None;

    for (i, b) in bars.iter().enumerate() {
        if let Some(p) = open.as_mut() {
            if b.high.is_some_and(|px| px >= ask) {
                let sold = p.left.min(share(b.high_volume));
                if sold > 0 {
                    p.proceeds += sold as f64 * net_ask;
                    p.left -= sold;
                    p.last_sale = b.hour;
                }
            }
            if p.left == 0 {
                let profit = p.proceeds - p.qty as f64 * bid;
                trips.push(Trip { buy_hour: bars[p.at].hour, exit_hour: p.last_sale, qty: p.qty, profit, forced: 0 });
                open = None;
            }
            continue;
        }
        if b.hour > last_entry || (!fills.reenter && !trips.is_empty()) {
            break;
        }
        if !b.low.is_some_and(|px| px <= bid) {
            continue;
        }
        if limit.is_some_and(|(start, _)| b.hour >= start + GE_LIMIT_HOURS) {
            limit = None;
        }
        let (start, used) = limit.unwrap_or((b.hour, 0));
        let room = ge_limit.map_or(i64::MAX, |l| i64::from(l) - used);
        let qty = order.qty.min(room).min(share(b.low_volume));
        if qty > 0 {
            limit = Some((start, used + qty));
            open = Some(Position { at: i, qty, left: qty, proceeds: 0.0, last_sale: b.hour });
        }
    }

    if let Some(p) = open {
        // The buy bucket always has an instant-sell price (it reached the bid)
        let px = bars[p.at..].iter().rev().find_map(|b| b.low).unwrap_or(bid) * (1.0 - fills.dump_discount);
        let px = px.round().clamp(0.0, i64::MAX as f64) as i64;
        let proceeds = p.proceeds + p.left as f64 * net_of_tax(px, order.name);
        trips.push(Trip { buy_hour: bars[p.at].hour, exit_hour: end, qty: p.qty, profit: proceeds - p.qty as f64 * bid, forced: p.left });
    }
    trips
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: i64 = 20_000;

    fn guide_day(day: i64, px: f64, volume: f64) -> PathDay {
        PathDay { day, buy_px: Some(px), sell_px: Some(px), exit_px: Some(px), buy_volume: volume, sell_volume: volume }
    }

    fn order(buy: i64, sell: i64, qty: i64) -> Order<'static> {
        Order { buy, sell, qty, name: "Coal" }
    }

    #[test]
    fn fills_at_the_bid_then_sells_at_the_ask() {
        // Day T itself never fills, even below the bid; the bid fills on T+2, the ask on T+4
        let path =
            [guide_day(T, 90.0, 1e6), guide_day(T + 1, 105.0, 1e6), guide_day(T + 2, 99.0, 1e6), guide_day(T + 4, 130.0, 1e6)];
        let o = simulate(&order(100, 125, 10), T, &path, 0.1);
        assert_eq!(o, Outcome { qty: 10, profit: 10.0 * (125.0 - 2.0 - 100.0), days: 4, forced: 0 });
        assert_eq!(o.per_day(), 230.0 / 4.0);
    }

    #[test]
    fn unfilled_bid_is_no_trade() {
        // T+8 is past the horizon
        let path = [guide_day(T + 1, 101.0, 1e6), guide_day(T + 7, 150.0, 1e6), guide_day(T + 8, 50.0, 1e6)];
        let o = simulate(&order(100, 125, 10), T, &path, 0.1);
        assert_eq!(o, Outcome::default());
        assert_eq!(o.per_day(), 0.0);
    }

    #[test]
    fn forced_exit_uses_the_last_price_within_the_horizon() {
        let path = [guide_day(T + 1, 100.0, 1e6), guide_day(T + 5, 95.0, 1e6), guide_day(T + 8, 200.0, 1e6)];
        let o = simulate(&order(100, 125, 10), T, &path, 0.1);
        // Dumped at 95 (tax 1 each), capital locked until T+7
        assert_eq!(o, Outcome { qty: 10, profit: 10.0 * (95.0 - 1.0 - 100.0), days: HORIZON, forced: 10 });
    }

    #[test]
    fn never_sells_on_the_entry_day() {
        // Both real-time averages cross on the last day: the buy fills, the sale cannot happen
        let both = PathDay { buy_px: Some(98.0), exit_px: Some(98.0), ..guide_day(T + 7, 130.0, 1e6) };
        let o = simulate(&order(100, 125, 10), T, &[both], 0.1);
        assert_eq!(o, Outcome { qty: 10, profit: 10.0 * (98.0 - 1.0 - 100.0), days: HORIZON, forced: 10 });
    }

    #[test]
    fn bonds_pay_no_tax() {
        let path = [guide_day(T + 1, 100.0, 1e6), guide_day(T + 2, 200.0, 1e6)];
        let o = simulate(&Order { buy: 100, sell: 200, qty: 1, name: "Bond" }, T, &path, 0.1);
        assert_eq!(o.profit, 100.0);
    }

    #[test]
    fn volume_share_caps_buys_and_sales() {
        // 10% of 50 = 5 bought; 10% of 30 = 3 sold on T+2, the other 2 on T+3
        let path = [guide_day(T + 1, 100.0, 50.0), guide_day(T + 2, 125.0, 30.0), guide_day(T + 3, 125.0, 1e6)];
        let o = simulate(&order(100, 125, 1000), T, &path, 0.1);
        assert_eq!(o, Outcome { qty: 5, profit: 5.0 * 23.0, days: 3, forced: 0 });
        // Too thin for a single unit: no fill
        assert_eq!(simulate(&order(100, 125, 1000), T, &[guide_day(T + 1, 100.0, 9.0)], 0.1), Outcome::default());
    }

    #[test]
    fn partial_sale_then_forced_exit() {
        let path = [guide_day(T + 1, 100.0, 1e6), guide_day(T + 2, 125.0, 40.0), guide_day(T + 6, 90.0, 1e6)];
        let o = simulate(&order(100, 125, 10), T, &path, 0.1);
        // 4 sold at 125 (net 123), 6 dumped at 90 (net 89)
        assert_eq!(o, Outcome { qty: 10, profit: 4.0 * 23.0 - 6.0 * 11.0, days: HORIZON, forced: 6 });
    }

    #[test]
    fn qty_matches_the_app() {
        assert_eq!(order_qty(300, Some(25_000), 50_000_000.0), 25_000);
        assert_eq!(order_qty(300, None, 1_000.0), 3);
        assert_eq!(order_qty(0, None, 1_000.0), 0);
        assert_eq!(order_qty(1, None, 1e12), i64::from(i32::MAX));
    }

    // ------------------------------------------------------------ hourly

    const H: i64 = 500_000;
    const WINDOW: HourWindow = HourWindow { signal: H, entry: 12, horizon: 24 };
    const FILLS: HourlyFills = HourlyFills { vol_share: 0.1, reenter: true, dump_discount: 0.0 };

    fn bucket(hour: i64, low: Option<f64>, high: Option<f64>, low_volume: f64, high_volume: f64) -> RtHour {
        RtHour { hour, high, low, high_volume, low_volume }
    }

    /// A bucket where both sides trade at `low` / `high` with plenty of volume.
    fn both(hour: i64, low: f64, high: f64) -> RtHour {
        bucket(hour, Some(low), Some(high), 1e6, 1e6)
    }

    #[test]
    fn hourly_fills_need_volume_and_never_happen_in_the_signal_bucket() {
        let path = [
            both(H, 90.0, 130.0),  // seen by the signal: ignored even though it crosses
            bucket(H + 1, Some(95.0), None, 9.0, 0.0),  // 10% of 9 is not one unit
            bucket(H + 2, Some(99.0), None, 50.0, 0.0),  // buys 5 (10% of 50) at the bid
            bucket(H + 3, None, Some(130.0), 0.0, 30.0),  // sells 3 at the ask
            bucket(H + 4, None, Some(125.0), 0.0, 1e6),  // sells the other 2
        ];
        let trips = simulate_hourly(&order(100, 125, 10), None, &WINDOW, &FILLS, &path);
        assert_eq!(trips, vec![Trip { buy_hour: H + 2, exit_hour: H + 4, qty: 5, profit: 5.0 * (125.0 - 2.0 - 100.0), forced: 0 }]);
        // An ask above every instant-buy average and a bid below every instant-sell average never trade
        assert_eq!(simulate_hourly(&order(100, 131, 10), None, &WINDOW, &FILLS, &path)[0].forced, 5);
        assert!(simulate_hourly(&order(94, 125, 10), None, &WINDOW, &FILLS, &path).is_empty());
    }

    #[test]
    fn hourly_never_sells_in_the_buy_bucket() {
        // Both sides cross in the buy bucket; nothing else trades, so the units are dumped
        let trips = simulate_hourly(&order(100, 125, 10), None, &WINDOW, &FILLS, &[both(H + 1, 99.0, 200.0)]);
        assert_eq!(trips, vec![Trip { buy_hour: H + 1, exit_hour: H + 24, qty: 10, profit: 10.0 * (99.0 - 1.0 - 100.0), forced: 10 }]);
    }

    #[test]
    fn hourly_forced_exit_at_the_horizon_bucket() {
        let mut path = vec![
            both(H + 1, 100.0, 110.0),
            bucket(H + 20, Some(97.0), None, 5.0, 0.0),
            bucket(H + 24, Some(95.0), Some(120.0), 5.0, 5.0),  // the ask is never reached
            both(H + 25, 300.0, 300.0),  // after the horizon: ignored
        ];
        let o = order(100, 125, 10);
        let forced = |profit: f64| vec![Trip { buy_hour: H + 1, exit_hour: H + 24, qty: 10, profit, forced: 10 }];
        // Dumped at the horizon bucket's instant-sell average (95, tax 1)
        assert_eq!(simulate_hourly(&o, None, &WINDOW, &FILLS, &path), forced(10.0 * (94.0 - 100.0)));
        // A discount lowers the dump price: 95 * 0.98 = 93.1, rounded to 93 (tax 1)
        let haircut = HourlyFills { dump_discount: 0.02, ..FILLS };
        assert_eq!(simulate_hourly(&o, None, &WINDOW, &haircut, &path), forced(10.0 * (92.0 - 100.0)));
        // Without an instant sale in the horizon bucket: the latest one since the buy (97)
        path[2].low = None;
        assert_eq!(simulate_hourly(&o, None, &WINDOW, &FILLS, &path), forced(10.0 * (96.0 - 100.0)));
    }

    #[test]
    fn hourly_ge_limit_counts_four_hours_from_the_first_buy() {
        // Every bucket would fill and sell; the limit of 10 allows one trip per 4-hour window
        let path: Vec<RtHour> = (H + 1..=H + 24).map(|h| both(h, 100.0, 130.0)).collect();
        let o = order(100, 125, 1000);
        let buys = |trips: Vec<Trip>| trips.iter().map(|t| (t.buy_hour - H, t.qty)).collect::<Vec<_>>();
        assert_eq!(buys(simulate_hourly(&o, Some(10), &WINDOW, &FILLS, &path)), vec![(1, 10), (5, 10), (9, 10)]);
        // Without a limit: buy, sell, buy again, until the entry window closes after bucket 12
        let unlimited = simulate_hourly(&o, None, &WINDOW, &FILLS, &path);
        assert_eq!(buys(unlimited.clone()), vec![(1, 1000), (3, 1000), (5, 1000), (7, 1000), (9, 1000), (11, 1000)]);
        assert!(unlimited.iter().all(|t| t.exit_hour == t.buy_hour + 1 && t.profit == 1000.0 * 23.0));
        // One trip per order when re-entry is off
        let once = HourlyFills { reenter: false, ..FILLS };
        assert_eq!(buys(simulate_hourly(&o, Some(10), &WINDOW, &once, &path)), vec![(1, 10)]);
        // A partly used window leaves the rest of the limit: 6 bought, 4 left within the window
        let thin = [bucket(H + 1, Some(100.0), None, 60.0, 0.0), both(H + 2, 100.0, 130.0), both(H + 3, 100.0, 130.0)];
        assert_eq!(buys(simulate_hourly(&o, Some(10), &WINDOW, &FILLS, &thin)), vec![(1, 6), (3, 4)]);
    }

    #[test]
    fn hourly_entry_window_and_sales_after_it() {
        let path = [bucket(H + 13, Some(90.0), None, 1e6, 0.0)];
        assert!(simulate_hourly(&order(100, 125, 10), None, &WINDOW, &FILLS, &path).is_empty());
        // Bought in the last entry bucket, sold well after it
        let path = [bucket(H + 12, Some(100.0), None, 1e6, 0.0), bucket(H + 20, None, Some(126.0), 0.0, 1e6)];
        let trips = simulate_hourly(&order(100, 125, 10), None, &WINDOW, &FILLS, &path);
        assert_eq!(trips, vec![Trip { buy_hour: H + 12, exit_hour: H + 20, qty: 10, profit: 230.0, forced: 0 }]);
    }

    #[test]
    fn hourly_tax_applies_to_sales_and_dumps_except_bonds() {
        let path = [both(H + 1, 100.0, 100.0), both(H + 2, 150.0, 200.0)];
        let sell = |name| simulate_hourly(&Order { buy: 100, sell: 200, qty: 1, name }, None, &WINDOW, &FILLS, &path)[0].profit;
        assert_eq!((sell("Coal"), sell("Bond")), (96.0, 100.0));
        let dump = |name| simulate_hourly(&Order { buy: 100, sell: 900, qty: 1, name }, None, &WINDOW, &FILLS, &path)[0].profit;
        assert_eq!((dump("Coal"), dump("Bond")), (147.0 - 100.0, 150.0 - 100.0));
    }

    #[test]
    fn hourly_ignores_buckets_outside_the_window() {
        let inside: Vec<RtHour> = (1..=24)
            .map(|i| bucket(H + i, Some(98.0 + (i % 5) as f64), Some(120.0 + (i % 7) as f64 * 2.0), 40.0 + i as f64, 35.0))
            .collect();
        let o = order(100, 125, 1000);
        let before = simulate_hourly(&o, Some(20), &WINDOW, &FILLS, &inside);
        assert!(!before.is_empty());
        let mut path = vec![both(H - 1, 1.0, 1e9), both(H, 1.0, 1e9)];
        path.extend(inside.iter().copied());
        path.extend((25..30).map(|i| both(H + i, 1.0, 1e9)));
        assert_eq!(simulate_hourly(&o, Some(20), &WINDOW, &FILLS, &path), before);
    }
}

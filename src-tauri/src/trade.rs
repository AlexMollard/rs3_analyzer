//! The trade rule shared by the app's live estimate and the backtest: one buy offer and one sell
//! offer at fixed prices, filled against one day of data at a time (docs/backtest.md).

use crate::flips::ge_tax;
use crate::model::RtDay;

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
    pub buy: i32,
    pub sell: i32,
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
pub fn order_qty(buy: i32, ge_limit: Option<i32>, alloc: f64) -> i64 {
    if buy <= 0 {
        return 0;
    }
    let affordable = (alloc / buy as f64).trunc().clamp(0.0, i32::MAX as f64) as i64;
    ge_limit.map_or(affordable, |limit| affordable.min(i64::from(limit)))
}

fn net_of_tax(price: i32, name: &str) -> f64 {
    f64::from(price - ge_tax(price, name))
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
    let bid = f64::from(order.buy);
    let entry = days.iter().enumerate().find_map(|(i, d)| {
        let qty = order.qty.min((vol_share * d.buy_volume).floor() as i64);
        (qty > 0 && d.buy_px.is_some_and(|p| p <= bid)).then_some((i, qty))
    });
    let Some((entry, qty)) = entry else {
        return Outcome::default();
    };

    let ask = f64::from(order.sell);
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
        let px = px.round().clamp(0.0, f64::from(i32::MAX)) as i32;
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

#[cfg(test)]
mod tests {
    use super::*;

    const T: i64 = 20_000;

    fn guide_day(day: i64, px: f64, volume: f64) -> PathDay {
        PathDay { day, buy_px: Some(px), sell_px: Some(px), exit_px: Some(px), buy_volume: volume, sell_volume: volume }
    }

    fn order(buy: i32, sell: i32, qty: i64) -> Order<'static> {
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
}

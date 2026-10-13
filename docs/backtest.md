# Flip scoring backtest

How well does the flip table's ranking turn into realised profit? This document describes a walk-forward backtest of the scoring, the real-time data added to improve it, what was tested, what was adopted, and how to re-run it.

## Summary

- **No scoring change was adopted.** Under the rule below, fixed before the results, the current scoring (`baseline`) had the best training Sharpe in both studies, so no candidate reached the held-out check. The held-out rows, reported for every candidate, agree: for a top-10 portfolio at 50M, no candidate beats the baseline on both Sharpe and P&L/day.
- **The baseline does not reliably make money either.** Top 10 at 50M: with guide-price fills it lost 1.6K/day in training and 11.9K/day on held-out data; with real-time fills it lost 14.1K/day in training and made 6.4K/day on held-out data, with a 95% interval of [-44.5K, 11.7K]. It loses the least mainly because it barely trades: 0.7% (training) and 3.9% (held out) of its top-10 picks fill against guide prices, about 16-19% against real-time prices, and nearly every unit that does fill is dumped at day 7 because the price never reaches its sell target. Its top-10 orders are capped by the GE limit, so its 50M and 250M guide-price rows are identical.
- **Real-time data is now in the app**, as information next to the unchanged ranking: a Live margin column and tile, and an expected profit per day (Exp/day) from the item's own record at live prices. The score, buy and sell prices, tiers and default sort are unchanged.
- **What the data supports:**
  - Live quotes fill far more often (about 70-87% of picks) than guide-price targets, and the high-ROI live trades were profitable per trade in both periods.
  - The tested live candidates ranked by expected gp per order, which puts full-size orders on the wide live spreads that can absorb them. A handful of those were items whose price collapsed within the week, losing 30-95% of the order. Those few trades decided the result: in the held-out period, the 10 worst of `live-3d`'s 230 top-10 trades lost 32.6M, more than its total loss of 31.7M.
  - The Exp/day estimate puts items in the right order by tier in both periods, but it overstates the profit.
  - The crash flag is informative: 49-55% of flagged items had a lower guide price 7 days later, against 31-45% of the rest.
  - The baseline's S/A/B/C tiers do not order realised outcomes.
- **Most promising follow-ups:** rank live quotes by return on cost instead of absolute gp, and stay out of collapsing markets. Neither was tested here, because every held-out row had already been printed by then. Fix the candidates in advance and judge them on real-time data after 2026-10-11 (see [Follow-ups](#follow-ups)).

## Objective and adoption rule

The objective is **risk-adjusted realised profit per day** at the user's budget and GE limit, for trades that buy and then sell within **7 days** of the signal.

The rule below was fixed before any held-out number was looked at:

- **Primary metric:** Sharpe ratio of weekly cohort P&L for a top-10 portfolio at a 50M budget. A cohort is the portfolio opened on one evaluation day. "Sharpe" here is the mean cohort P&L divided by its standard deviation, computed over non-overlapping cohorts (one every 7 days) and averaged over the 7 possible start days. It is not annualised.
- **Guardrails:** the candidate's P&L/day must not be lower, and its hit rate (profitable trades ÷ filled trades) must not be more than 5 points lower.
- **Selection:** candidates are compared on the training period only. The best one is then checked once against the reference on the held-out period. It is adopted only if the held-out primary metric and P&L/day both improve. Every candidate's held-out numbers are reported, including the rejected ones.

## Data

Coverage of the database used (`rs3_market.db`, read-only):

| Table | Rows | Items | Days | Notes |
|---|---|---|---|---|
| `history` | 2,617,519 | 7,444 | 2025-10-12 to 2026-10-12 | 9,098 rows (all rows of 22 items) are unreadable: their price is above the 32-bit range, for example 214,000,000,000 gp. The app skips the same rows. |
| `rt_daily` | 435,281 | 7,081 (7,059 with readable history) | 80 days, 2026-07-24 to 2026-10-11 | Backfill finished with no failed days. Typically 5,250-5,650 items a day (median 5,450); the fewest is 4,780 on 2026-08-11. |

Each table row is an item and a UTC day:

- `history`: the daily **guide price** and volume (Weird Gloop). The guide price is a smoothed daily reference price, not a trade price.
- `rt_daily`: **real-time trades** from the RuneScape Wiki, folded from the bulk hourly `/1h` endpoint by `backfill_rt.py` and the daily collector. `avg_high` is the volume-weighted average price paid by instant buyers (what a seller's offer receives). `avg_low` is the average received by instant sellers (what a buyer's offer pays). A side with no trades that day is NULL. 2026-07-23 is not stored, because the API's data starts at 17:00 UTC that day.

Checks on the data:

- `rt_daily` matches the wiki's own daily `/timeseries` points for three items (a top-volume item, a thinly traded one, and Bond): 231 item-days, no day off by more than 0.1%, a worst price difference of 0.03% (rounding), and identical volumes.
- The newest `history` day, 2026-10-12, is an exact copy of 2026-10-11 for all 7,362 items, prices and volumes alike. Earlier consecutive days share both only about 20% of the time. The collector stores the latest dump under the day it runs, and that dump held what the backfill had already stored for 2026-10-11. The backtest therefore stops at 2026-10-11 (`--end` defaults to the day before the newest row).

## Walk-forward method

For each evaluation day `t`, every item is scored exactly as the app would score it on day `t`:

- Only rows dated on or before `t` are used. The guide-price window is the app's 90 days (`t - 90` to `t`), and real-time windows end at `t`. A unit test changes every row after `t` (prices, volumes and real-time rows) and checks that every strategy's buy, sell, score and tier are unchanged.
- The app's default filters apply: quantity at the budget above 0, 90-day average guide volume of at least 500, ROI at most 150%, buy price at least 100 gp, and negative scores hidden. All tiers are included, as in the app.
- Items are ranked by the strategy's key: the score for the baseline (what the app sorts by), and the expected profit of the order for the others (see [Strategies tested](#strategies-tested)).

Evaluation days run daily to 7 days before the end of the data. The guide study starts on the first day with a full 90-day window. The real-time study starts on the first day that every candidate can produce a signal: the track-record candidates need two past signals whose 7-day outcomes have closed.

### Trade simulator

Each pick is a buy-then-sell trade at fixed offer prices, signalled at the end of day `t`:

1. **Entry.** A buy offer at the model's buy price fills on the first day from `t+1` to `t+7` on which the buy-side proxy is at or below it. It fills at the offer price, never at the lower proxy price. If the price is never reached, there is no trade: the pick earns 0 and still counts in every average.
2. **Quantity.** The order is `min(floor(allocation / buy), GE limit)`, as in the app. The candidates other than the baseline also cap it at 10% of the item's typical daily volume. The filled amount is also capped at **10% of that day's volume** (`--vol-share`), so a large budget cannot buy more than the market traded. A day too thin for one unit does not fill. Partial fills are not accumulated over several days.
3. **Exit.** From the day after entry, the units are offered at the model's sell price. On each day the sell-side proxy is at or above it, up to 10% of that day's volume sells at the offer price. A sale never happens on the entry day.
4. **Forced exit.** Whatever is still held at `t+7` is dumped at the latest exit price on or before `t+7`.
5. **Tax.** The 2% GE tax (rounded down per item, Bonds exempt) is charged on every sale, including forced exits.
6. **Profit per day** = realised profit ÷ (last sale day − `t`), with a minimum of 1. Days are counted from the signal because GE offers lock their coins from the moment they are placed. Unfilled picks score 0.

Two proxies stand in for intraday fills:

- **Guide-price fills** (the full year): the daily guide price on every side, with guide volume.
- **Real-time fills** (the last 80 days): a buy fills when that day's `avg_low` is at or below the bid, using `low_volume`. A sale fills when `avg_high` is at or above the ask, using `high_volume`. A forced exit gets `avg_low`.

The GE buy limit resets every 4 hours. The simulator assumes one limit load per trade, which is conservative.

### Portfolios and metrics

- **Top-K portfolio** (K = 10 and 20): each evaluation day the budget is split evenly over the first K ranked items that are affordable at budget ÷ K. That cohort's P&L is the sum of its trades.
- **P&L/day** = mean cohort P&L ÷ 7. It is what one budget earns if a new cohort is opened every 7 days, so it can actually be realised.
- **Per-trade profit/day** follows the definition above, summed over the cohort. Treat it as secondary: quick wins are divided by 2 days and forced-exit losses by 7, so it can be positive while P&L is negative.
- **Sharpe** and **max drawdown** use non-overlapping cohorts, averaged over the 7 start days. The 95% intervals come from a weekly block bootstrap (2,000 resamples in 7-day blocks, fixed seed). Paired differences between two strategies resample the same days for both.
- **Fill rate** = filled ÷ picks. **Hit rate** = profitable ÷ filled. **Forced units** = the share of bought units dumped at `t+7`.
- **Ranking quality** treats every candidate as a full-budget trade, which is what the table shows per row. **IC** is the Spearman correlation between the rank key and realised profit/day, computed per day and averaged across days.
- **Tiers:** the same full-budget trades, grouped by tier. **Return/day on cost** is their summed profit/day divided by the gp they spent on filled buys.
- **Crash flags:** among items that pass every filter except the score, the share whose guide price was lower 7 days later, for flagged items compared with unflagged ones.

### Train and test periods

| Study | Signal days | Training | Held out | Non-overlapping cohorts per start day |
|---|---|---|---|---|
| Guide-price fills | 2026-01-10 to 2026-10-04 (268) | 2026-01-10 to 2026-06-05 (147) | 2026-06-13 to 2026-10-04 (114) | 21 train, 16 test |
| Real-time fills | 2026-08-02 to 2026-10-04 (64) | 2026-08-02 to 2026-09-04 (34) | 2026-09-12 to 2026-10-04 (23) | 4 train, 3 test |

- **Guide-price fills:** the held-out test period is the last ~4 months of signals (`--test-from`, default end − 120 days). Training signals stop 8 days before the test starts, so no training outcome sees test-period prices.
- **Real-time fills:** only 80 days of real-time data exist. The first ~50 days are for training and the remaining ~30 for testing (`--rt-test-from`, default first real-time day + 50), with the same 8-day embargo. That leaves about 3 non-overlapping cohorts in the test, which is a small sample: its intervals are wide, and a single cohort can flip the sign of a result.
- The whole real-time window falls inside the guide-price test period, so only the last ~30 real-time days are a clean test of anything that uses real-time data.

## Strategies tested

| Strategy | Buy and sell prices | Score | Rank key and order size |
|---|---|---|---|
| `baseline` | The app's `flips::analyze`: Q10 and Q90 of the 90-day guide prices (Q15 and Q85 for crashed or spiked items) | The app's multi-factor score | Score; budget and GE limit only |
| `band-Nd-qLO-qHI` | The LO and HI percentiles of the last N days of guide prices (90d Q10-Q90, 30d Q10-Q90, 14d Q25-Q75, 7d Q25-Q75) | Margin after tax | Margin × quantity, the expected profit of the order; quantity also capped at 10% of the 90-day average guide volume |
| `live-Nd` | Median `avg_low` and median `avg_high` over the last N days (1, 3, 7) on which both sides traded (`flips::live_quote`) | Margin after tax | Margin × quantity; quantity also capped at 10% of the median daily volume on the thinner side |
| `live-3d-nocrash` | As `live-3d` | As `live-3d` | As `live-3d`, without items flagged crashing or crashed |
| `emp-Nd-28`, `emp-Nd-28-lcb` | As `live-Nd` (N = 3, and 1 for the `-lcb` variant) | The item's track record: mean realised profit per day of one unit when the same quote rule was applied on each of the last 28 days whose 7-day outcome has closed (`t-28` to `t-8`). `-lcb` subtracts one standard error, which penalises short or erratic records | Score × quantity, capped as for `live-Nd` |
| `emp-guide-L-lcb` | As `baseline` | Track record as above, from guide-price fills over the last L days (28, 56), minus one standard error | Score × quantity, capped as for `band` |

The track-record score is in `flips::live_estimate`, which the app also uses for Exp/day. For tiers, the `emp-*` strategies divide the score by the buy price: S is 1% or more a day, A 0.5-1%, B 0.2-0.5%, C 0-0.2%, and X is below 0. The other strategies use the app's margin and ROI tiers.

The candidates and their settings were chosen before any real-time held-out result existed, and the real-time test period was blind. The guide-price test period was not fully blind, though. Preliminary runs on the partly backfilled history printed held-out rows for the baseline and the band candidates, and `emp-guide-*` was added after those runs. No candidate was adopted, so this could not have caused a false adoption. It does mean the guide-price held-out rows are weaker evidence than the real-time ones. The real-time study's first day was moved from 2026-07-30 to 2026-08-02 before the final run, so that every candidate can signal from day one. Before, the track-record candidates scored 0 on the early days, when they could not trade yet, which flattered them.

## Results

### Adoption decision

| Study | Best training Sharpe (top 10, 50M) | Runner-up | Decision |
|---|---|---|---|
| Guide-price fills | `baseline` -0.20 | `emp-guide-56-lcb` -0.82 | Keep the baseline |
| Real-time fills | `baseline` -0.29 | `live-1d` -0.34 | Keep the baseline |

The closest held-out challenger at 50M with K = 10 is `band-14d-q25-q75` with real-time fills. Its Sharpe is higher (0.02 against -0.09), but it fails both guardrails: +3.6K/day against the baseline's +6.4K/day, and a 43.8% hit rate against 54.5%. The same candidate lost 605K/day in training. Every other candidate lost money on held-out data.

**Real-time fills: top 10 at 50M**

| Strategy | Period | P&L/day [95% CI] | Sharpe [95% CI] | Max drawdown (avg) | Fill | Hit | Forced units |
|---|---|---|---|---|---|---|---|
| `baseline` | train | -14.1K [-44.2K, 8.6K] | -0.29 [-1.16, 0.14] | 910.5K | 15.9% | 44.4% | 99.6% |
| `baseline` | test | 6.4K [-44.5K, 11.7K] | -0.09 [-0.97, 1.31] | 549.6K | 19.1% | 54.5% | 98.0% |
| `band-14d-q25-q75` | train | -605.3K [-829.9K, -343.2K] | -1.45 [-4.25, -1.29] | 20.58M | 70.9% | 21.2% | 69.4% |
| `band-14d-q25-q75` | test | 3.6K [-106.8K, 166.6K] | 0.02 [-1.36, 9.85] | 1.56M | 34.8% | 43.8% | 96.8% |
| `live-1d` | train | -134.4K [-302.6K, 14.5K] | -0.34 [-0.98, 0.25] | 6.80M | 71.2% | 43.4% | 88.1% |
| `live-1d` | test | -253.5K [-506.6K, -63.8K] | -0.75 [-3.70, -0.07] | 7.79M | 72.6% | 40.1% | 88.7% |
| `live-3d` | train | -197.3K [-320.0K, -66.2K] | -0.62 [-1.54, -0.29] | 8.12M | 69.7% | 40.1% | 81.3% |
| `live-3d` | test | -196.9K [-499.7K, 52.1K] | -0.51 [-1.70, 0.44] | 5.77M | 75.2% | 45.1% | 62.7% |
| `live-7d` | train | -172.0K [-394.5K, -20.2K] | -0.58 [-1.74, -0.07] | 7.71M | 70.3% | 36.4% | 59.6% |
| `live-7d` | test | -384.7K [-692.6K, -13.6K] | -0.67 [-2.08, 0.05] | 9.49M | 70.9% | 41.7% | 84.0% |
| `live-3d-nocrash` | train | -229.3K [-337.0K, -94.7K] | -0.71 [-1.49, -0.47] | 9.07M | 75.0% | 38.4% | 81.3% |
| `live-3d-nocrash` | test | -288.0K [-654.6K, 8.8K] | -0.54 [-1.49, 0.38] | 7.50M | 80.0% | 41.3% | 72.1% |
| `emp-3d-28` | train | -155.6K [-344.7K, 37.3K] | -0.49 [-1.38, 0.18] | 6.16M | 80.9% | 45.5% | 51.2% |
| `emp-3d-28` | test | -219.9K [-566.1K, 4.0K] | -0.59 [-2.12, 0.11] | 6.38M | 86.5% | 52.3% | 47.5% |
| `emp-3d-28-lcb` | train | -155.1K [-338.5K, 20.0K] | -0.54 [-1.48, 0.12] | 6.95M | 83.8% | 46.3% | 41.1% |
| `emp-3d-28-lcb` | test | -191.5K [-556.3K, 3.8K] | -0.69 [-3.29, 0.26] | 6.05M | 84.3% | 52.6% | 36.2% |
| `emp-1d-28-lcb` | train | -105.1K [-204.2K, -19.3K] | -0.68 [-1.69, -0.23] | 4.49M | 82.4% | 48.6% | 45.0% |
| `emp-1d-28-lcb` | test | -113.2K [-387.9K, 38.0K] | -0.22 [-2.10, 0.62] | 5.21M | 83.0% | 49.7% | 32.7% |

**Guide-price fills: top 10 at 50M**

| Strategy | Period | P&L/day [95% CI] | Sharpe [95% CI] | Max drawdown (avg) | Fill | Hit | Forced units |
|---|---|---|---|---|---|---|---|
| `baseline` | train | -1.6K [-4.2K, -126] | -0.20 [-0.32, -0.03] | 254.4K | 0.7% | 9.1% | 100.0% |
| `baseline` | test | -11.9K [-20.1K, -1.8K] | -0.32 [-0.52, -0.11] | 1.44M | 3.9% | 20.5% | 100.0% |
| `band-90d-q10-q90` | train | -155.6K [-179.2K, -125.9K] | -2.12 [-2.90, -1.73] | 22.88M | 26.1% | 10.2% | 100.0% |
| `band-90d-q10-q90` | test | -162.1K [-273.7K, -79.7K] | -0.73 [-1.09, -0.54] | 18.98M | 25.5% | 18.9% | 100.0% |
| `band-30d-q10-q90` | train | -165.1K [-201.9K, -127.0K] | -1.55 [-2.16, -1.28] | 24.28M | 31.3% | 9.3% | 100.0% |
| `band-30d-q10-q90` | test | -222.7K [-367.6K, -119.1K] | -0.81 [-1.27, -0.58] | 26.10M | 35.4% | 22.1% | 100.0% |
| `band-14d-q25-q75` | train | -286.1K [-361.5K, -228.4K] | -1.65 [-2.14, -1.39] | 42.06M | 44.1% | 9.7% | 99.9% |
| `band-14d-q25-q75` | test | -367.2K [-535.2K, -239.3K] | -1.10 [-1.67, -0.83] | 42.31M | 48.9% | 12.7% | 98.8% |
| `band-7d-q25-q75` | train | -318.4K [-393.9K, -259.0K] | -1.63 [-2.08, -1.38] | 46.84M | 49.4% | 9.6% | 97.8% |
| `band-7d-q25-q75` | test | -377.2K [-522.8K, -248.3K] | -1.14 [-1.64, -0.88] | 43.07M | 53.2% | 14.4% | 94.1% |
| `emp-guide-28-lcb` | train | -24.2K [-32.4K, -16.5K] | -0.83 [-1.12, -0.62] | 3.57M | 9.7% | 9.2% | 96.5% |
| `emp-guide-28-lcb` | test | -50.5K [-78.4K, -29.8K] | -0.73 [-1.04, -0.44] | 5.84M | 18.3% | 17.7% | 97.5% |
| `emp-guide-56-lcb` | train | -32.0K [-43.0K, -23.4K] | -0.82 [-1.07, -0.64] | 4.71M | 10.2% | 8.0% | 99.8% |
| `emp-guide-56-lcb` | test | -38.4K [-60.8K, -18.5K] | -0.57 [-1.12, -0.26] | 4.57M | 18.6% | 23.1% | 99.1% |

### Every budget

P&L/day with the Sharpe ratio in brackets. Budgets other than 50M are reported for context; they did not enter the decision.

**Real-time fills: P&L/day (Sharpe) by budget, top 10**

| Strategy | Train 10M | Train 50M | Train 250M | Test 10M | Test 50M | Test 250M |
|---|---|---|---|---|---|---|
| `baseline` | -8.1K (-0.36) | -14.1K (-0.29) | -14.7K (-0.24) | 9.1K (0.11) | 6.4K (-0.09) | -63.5K (-0.11) |
| `band-14d-q25-q75` | -119.3K (-1.60) | -605.3K (-1.45) | -2.07M (-0.97) | 7.6K (0.17) | 3.6K (0.02) | -381.9K (-0.73) |
| `live-1d` | 8.4K (0.11) | -134.4K (-0.34) | -612.9K (-0.50) | -38.6K (-0.98) | -253.5K (-0.75) | -761.2K (-0.80) |
| `live-3d` | 6.8K (0.08) | -197.3K (-0.62) | -828.2K (-0.72) | -46.0K (-0.45) | -196.9K (-0.51) | -630.0K (-0.27) |
| `live-7d` | 29.0K (0.21) | -172.0K (-0.58) | -750.6K (-0.74) | -21.4K (0.01) | -384.7K (-0.67) | -994.3K (-0.71) |
| `live-3d-nocrash` | 13.2K (0.33) | -229.3K (-0.71) | -917.7K (-0.82) | -50.9K (-0.27) | -288.0K (-0.54) | -833.4K (-0.52) |
| `emp-3d-28` | -17.3K (-0.16) | -155.6K (-0.49) | -555.3K (-0.66) | 22.4K (0.12) | -219.9K (-0.59) | -933.6K (-1.42) |
| `emp-3d-28-lcb` | -2.7K (0.05) | -155.1K (-0.54) | -524.1K (-0.61) | 26.2K (0.22) | -191.5K (-0.69) | -933.8K (-1.31) |
| `emp-1d-28-lcb` | 3.2K (0.12) | -105.1K (-0.68) | -314.7K (-0.60) | 20.2K (0.21) | -113.2K (-0.22) | -822.9K (-0.80) |

**Real-time fills: P&L/day (Sharpe) by budget, top 20**

| Strategy | Train 10M | Train 50M | Train 250M | Test 10M | Test 50M | Test 250M |
|---|---|---|---|---|---|---|
| `baseline` | 698 (0.04) | -3.8K (-0.04) | -8.1K (-0.07) | 13.5K (0.33) | 6.2K (-0.26) | -93.3K (-0.23) |
| `band-14d-q25-q75` | -102.5K (-1.60) | -444.8K (-1.51) | -1.95M (-1.33) | -17.0K (-1.10) | -67.2K (-0.43) | -517.3K (-0.83) |
| `live-1d` | 28.9K (0.40) | -91.6K (-0.27) | -632.9K (-0.75) | 8.8K (0.05) | -149.3K (-0.65) | -816.9K (-0.81) |
| `live-3d` | 18.0K (0.43) | -114.0K (-0.51) | -794.6K (-1.05) | -25.5K (-0.21) | -222.9K (-0.81) | -891.7K (-1.22) |
| `live-7d` | 19.4K (0.22) | -214.8K (-1.02) | -993.1K (-1.16) | -6.5K (-0.00) | -248.6K (-0.74) | -1.19M (-0.94) |
| `live-3d-nocrash` | 15.6K (0.30) | -138.7K (-0.68) | -820.4K (-0.90) | -22.7K (-0.09) | -261.6K (-0.68) | -1.02M (-1.08) |
| `emp-3d-28` | -8.1K (-0.22) | -188.0K (-0.85) | -854.9K (-0.78) | 7.4K (0.04) | -82.0K (-0.31) | -663.6K (-0.78) |
| `emp-3d-28-lcb` | -2.1K (-0.02) | -136.4K (-0.53) | -706.4K (-0.75) | 26.0K (0.30) | -73.4K (-0.25) | -651.4K (-0.74) |
| `emp-1d-28-lcb` | 7.2K (0.04) | -86.1K (-0.42) | -455.1K (-0.85) | 24.4K (0.27) | -33.4K (-0.18) | -745.6K (-0.84) |

**Guide-price fills: P&L/day (Sharpe) by budget, top 10**

| Strategy | Train 10M | Train 50M | Train 250M | Test 10M | Test 50M | Test 250M |
|---|---|---|---|---|---|---|
| `baseline` | -876 (-0.21) | -1.6K (-0.20) | -1.6K (-0.20) | -3.2K (-0.41) | -11.9K (-0.32) | -33.4K (-0.22) |
| `band-90d-q10-q90` | -32.1K (-1.12) | -155.6K (-2.12) | -887.2K (-1.53) | -21.9K (-0.66) | -162.1K (-0.73) | -1.02M (-0.70) |
| `band-30d-q10-q90` | -36.0K (-1.55) | -165.1K (-1.55) | -761.2K (-1.46) | -45.6K (-0.96) | -222.7K (-0.81) | -1.04M (-0.80) |
| `band-14d-q25-q75` | -53.3K (-1.88) | -286.1K (-1.65) | -1.32M (-1.68) | -73.1K (-1.16) | -367.2K (-1.10) | -1.72M (-1.05) |
| `band-7d-q25-q75` | -61.3K (-1.66) | -318.4K (-1.63) | -1.47M (-1.66) | -77.9K (-1.17) | -377.2K (-1.14) | -1.75M (-1.19) |
| `emp-guide-28-lcb` | -6.2K (-0.93) | -24.2K (-0.83) | -88.0K (-0.72) | -10.1K (-0.69) | -50.5K (-0.73) | -192.3K (-0.81) |
| `emp-guide-56-lcb` | -7.2K (-0.81) | -32.0K (-0.82) | -121.3K (-0.76) | -11.7K (-0.69) | -38.4K (-0.57) | -135.7K (-0.56) |

**Guide-price fills: P&L/day (Sharpe) by budget, top 20**

| Strategy | Train 10M | Train 50M | Train 250M | Test 10M | Test 50M | Test 250M |
|---|---|---|---|---|---|---|
| `baseline` | -1.8K (-0.41) | -6.3K (-0.40) | -6.4K (-0.35) | -4.2K (-0.54) | -18.1K (-0.53) | -58.0K (-0.46) |
| `band-90d-q10-q90` | -35.3K (-2.01) | -141.8K (-1.52) | -854.0K (-2.29) | -31.2K (-1.21) | -210.3K (-1.07) | -879.4K (-1.00) |
| `band-30d-q10-q90` | -39.6K (-2.47) | -168.7K (-1.83) | -847.5K (-2.40) | -40.8K (-0.96) | -217.8K (-0.94) | -1.00M (-0.93) |
| `band-14d-q25-q75` | -55.5K (-2.57) | -267.3K (-2.11) | -1.29M (-2.10) | -67.0K (-1.42) | -312.3K (-1.29) | -1.55M (-1.24) |
| `band-7d-q25-q75` | -61.5K (-2.10) | -301.4K (-2.23) | -1.44M (-2.19) | -69.7K (-1.42) | -344.4K (-1.42) | -1.67M (-1.47) |
| `emp-guide-28-lcb` | -6.9K (-1.16) | -30.5K (-1.14) | -87.2K (-1.01) | -11.3K (-0.91) | -52.3K (-0.95) | -195.1K (-0.93) |
| `emp-guide-56-lcb` | -7.1K (-0.99) | -33.6K (-0.99) | -108.2K (-0.92) | -11.9K (-0.97) | -56.8K (-1.02) | -160.0K (-0.86) |

### Paired differences against the baseline

Candidate minus baseline over the same days, with the 95% block-bootstrap interval.

**Real-time fills: paired difference vs `baseline`, top 10 at 50M**

| Strategy | Train Sharpe diff | Train P&L/day diff | Test Sharpe diff | Test P&L/day diff |
|---|---|---|---|---|
| `band-14d-q25-q75` | -1.16 [-3.92, -0.75] | -591.2K [-794.9K, -342.8K] | 0.11 [-1.85, 9.47] | -2.9K [-76.5K, 160.6K] |
| `live-1d` | -0.05 [-0.44, 0.70] | -120.3K [-274.7K, 25.3K] | -0.66 [-7.38, 0.46] | -259.9K [-484.3K, -63.6K] |
| `live-3d` | -0.33 [-1.23, 0.39] | -183.2K [-293.6K, -52.7K] | -0.42 [-2.80, 0.76] | -203.4K [-470.5K, 51.0K] |
| `live-7d` | -0.29 [-0.94, 0.14] | -157.9K [-363.9K, -18.6K] | -0.58 [-2.81, -0.03] | -391.1K [-662.5K, -19.7K] |
| `live-3d-nocrash` | -0.42 [-1.20, 0.24] | -215.2K [-310.0K, -77.0K] | -0.45 [-1.77, 0.24] | -294.5K [-612.4K, 3.2K] |
| `emp-3d-28` | -0.20 [-0.89, 0.92] | -141.5K [-322.2K, 65.9K] | -0.49 [-2.49, -0.10] | -226.4K [-542.9K, -2.0K] |
| `emp-3d-28-lcb` | -0.25 [-0.94, 0.85] | -141.0K [-312.9K, 45.5K] | -0.60 [-10.15, -0.07] | -198.0K [-527.0K, -1.8K] |
| `emp-1d-28-lcb` | -0.39 [-0.98, 0.05] | -91.0K [-171.6K, -17.6K] | -0.13 [-3.84, 0.64] | -119.6K [-348.1K, 33.0K] |

**Guide-price fills: paired difference vs `baseline`, top 10 at 50M**

| Strategy | Train Sharpe diff | Train P&L/day diff | Test Sharpe diff | Test P&L/day diff |
|---|---|---|---|---|
| `band-90d-q10-q90` | -1.92 [-2.74, -1.50] | -154.0K [-178.2K, -123.6K] | -0.40 [-0.79, -0.16] | -150.2K [-263.1K, -70.4K] |
| `band-30d-q10-q90` | -1.35 [-1.99, -1.07] | -163.5K [-200.0K, -125.3K] | -0.49 [-0.96, -0.20] | -210.9K [-358.0K, -109.7K] |
| `band-14d-q25-q75` | -1.44 [-1.97, -1.15] | -284.5K [-359.9K, -226.7K] | -0.78 [-1.39, -0.44] | -355.3K [-525.4K, -230.0K] |
| `band-7d-q25-q75` | -1.43 [-1.91, -1.15] | -316.8K [-392.2K, -257.3K] | -0.82 [-1.36, -0.51] | -365.4K [-510.5K, -241.2K] |
| `emp-guide-28-lcb` | -0.63 [-1.01, -0.37] | -22.6K [-31.5K, -13.9K] | -0.40 [-0.74, -0.09] | -38.6K [-68.9K, -19.3K] |
| `emp-guide-56-lcb` | -0.62 [-0.98, -0.37] | -30.4K [-42.2K, -21.2K] | -0.25 [-0.77, 0.06] | -26.5K [-48.5K, -10.8K] |

### Ranking quality

**Real-time fills: ranking quality at 50M (every candidate, full budget each)**

| Strategy | Train candidates/day | Train IC | Train IC > 0 | Train profit/day per candidate | Test candidates/day | Test IC | Test IC > 0 | Test profit/day per candidate |
|---|---|---|---|---|---|---|---|---|
| `baseline` | 1618 | 0.07 | 97.1% | -21.9K | 1604 | 0.08 | 100.0% | -14.9K |
| `band-14d-q25-q75` | 1187 | -0.05 | 17.6% | -25.6K | 1288 | 0.02 | 56.5% | -16.6K |
| `live-1d` | 889 | 0.04 | 70.6% | -1.8K | 917 | 0.05 | 73.9% | -5.2K |
| `live-3d` | 996 | 0.05 | 91.2% | -3.6K | 1012 | 0.05 | 69.6% | -6.6K |
| `live-7d` | 1067 | 0.04 | 82.4% | -6.3K | 1080 | 0.06 | 87.0% | -8.5K |
| `live-3d-nocrash` | 804 | 0.04 | 82.4% | -5.1K | 740 | 0.05 | 65.2% | -10.3K |
| `emp-3d-28` | 761 | 0.01 | 55.9% | -5.4K | 740 | 0.04 | 87.0% | -5.3K |
| `emp-3d-28-lcb` | 557 | 0.03 | 61.8% | -3.5K | 536 | 0.05 | 95.7% | -2.9K |
| `emp-1d-28-lcb` | 446 | 0.02 | 61.8% | -3.2K | 448 | 0.02 | 69.6% | -5.9K |

**Guide-price fills: ranking quality at 50M (every candidate, full budget each)**

| Strategy | Train candidates/day | Train IC | Train IC > 0 | Train profit/day per candidate | Test candidates/day | Test IC | Test IC > 0 | Test profit/day per candidate |
|---|---|---|---|---|---|---|---|---|
| `baseline` | 1780 | 0.24 | 100.0% | -21.7K | 1650 | 0.14 | 100.0% | -28.6K |
| `band-90d-q10-q90` | 1608 | -0.02 | 35.4% | -19.8K | 1544 | -0.10 | 14.9% | -26.6K |
| `band-30d-q10-q90` | 1350 | -0.10 | 21.8% | -26.3K | 1354 | -0.12 | 14.9% | -26.8K |
| `band-14d-q25-q75` | 813 | -0.17 | 0.7% | -42.8K | 967 | -0.17 | 6.1% | -39.8K |
| `band-7d-q25-q75` | 453 | -0.19 | 2.7% | -50.8K | 653 | -0.20 | 2.6% | -45.9K |
| `emp-guide-28-lcb` | 916 | -0.01 | 41.5% | -6.5K | 731 | -0.03 | 37.7% | -8.7K |
| `emp-guide-56-lcb` | 644 | -0.01 | 51.0% | -6.0K | 515 | -0.02 | 24.6% | -8.5K |

The baseline has the highest IC in both studies, so its ranking does carry some information about relative outcomes. With guide-price fills it is 0.24 in training and 0.14 held out. With real-time fills it is 0.07 and 0.08. Its average candidate still loses: the information is about which items lose less.

### Tiers

The baseline's tiers do not put realised outcomes in order. With real-time fills, return per day on cost in the held-out period is S -0.56%, A -0.27%, B -0.48% and C -0.63%, and S is the worst tier in training. Tier X, a net loss after tax at the model's prices, passes the default filters, and only 0.1-7% of its filled trades made money.

For `emp-3d-28-lcb`, the realised return falls with every tier in both periods (S > A > B > C), so the track record orders items correctly. It overstates the size, though: items it rated 1%+ a day (S) made 0.33% and 0.76% a day, those rated 0.5-1% (A) broke even, and B and C lost money. The app's Exp/day risk note says this.

For `live-3d`, the S, A and B tiers (ROI above 8% or margin above 200K) made money per trade in both periods, at 3.1-3.4% a day on cost for S. The C tier lost. Most of those profitable trades were small orders in thinly traded items. The top-10 portfolio instead picks the wide spreads that can take a full 5M order, and it still lost. Its losses came from a few collapsing markets:
- In training, the worst were Croesus flakes (bought at 864, 61-79% of the order lost), Radiant energy and Triskelion.
- In the held-out period, the worst were Sanguine spines (bought at 3,139, 95% lost) and Bogolin telepearl, Crimson ichor and Bogwart. Those last three first appeared on the GE a day or two before the signal.

**Real-time fills: tiers at 50M (every candidate, full budget each)**

| Strategy | Period | Tier | Trades | Fill | Hit | Profit/day | Return/day on cost |
|---|---|---|---|---|---|---|---|
| `baseline` | train | S | 2071 | 48.3% | 30.3% | -37.3K | -1.58% |
| `baseline` | train | A | 6714 | 59.9% | 39.6% | -18.7K | -0.72% |
| `baseline` | train | B | 16357 | 67.2% | 36.6% | -30.2K | -0.84% |
| `baseline` | train | C | 22036 | 65.7% | 42.6% | -18.0K | -0.72% |
| `baseline` | train | X | 7827 | 55.4% | 7.2% | -14.4K | -0.67% |
| `baseline` | test | S | 4681 | 57.7% | 45.8% | -7.9K | -0.56% |
| `baseline` | test | A | 7314 | 61.6% | 41.8% | -5.2K | -0.27% |
| `baseline` | test | B | 12457 | 65.4% | 45.9% | -15.7K | -0.48% |
| `baseline` | test | C | 9690 | 61.4% | 43.1% | -24.0K | -0.63% |
| `baseline` | test | X | 2747 | 52.7% | 6.1% | -17.3K | -0.37% |
| `emp-3d-28-lcb` | train | S | 10630 | 84.1% | 52.6% | 2.1K | 0.33% |
| `emp-3d-28-lcb` | train | A | 2791 | 84.8% | 51.3% | 1.7K | 0.08% |
| `emp-3d-28-lcb` | train | B | 2398 | 84.9% | 48.4% | -10.3K | -0.29% |
| `emp-3d-28-lcb` | train | C | 3118 | 82.5% | 41.3% | -21.9K | -0.38% |
| `emp-3d-28-lcb` | test | S | 7121 | 83.4% | 53.1% | 4.5K | 0.76% |
| `emp-3d-28-lcb` | test | A | 1720 | 84.4% | 47.9% | -620 | -0.02% |
| `emp-3d-28-lcb` | test | B | 1768 | 85.2% | 44.0% | -15.3K | -0.30% |
| `emp-3d-28-lcb` | test | C | 1722 | 83.1% | 42.7% | -23.5K | -0.37% |
| `live-3d` | train | S | 7411 | 77.9% | 62.8% | 7.3K | 3.11% |
| `live-3d` | train | A | 3819 | 77.7% | 63.1% | 12.4K | 1.89% |
| `live-3d` | train | B | 7369 | 80.3% | 63.0% | 7.4K | 0.43% |
| `live-3d` | train | C | 15278 | 82.0% | 57.4% | -18.2K | -0.31% |
| `live-3d` | test | S | 4493 | 76.5% | 62.6% | 8.1K | 3.38% |
| `live-3d` | test | A | 2480 | 79.4% | 60.8% | 12.4K | 1.66% |
| `live-3d` | test | B | 5013 | 80.3% | 59.4% | 4.0K | 0.21% |
| `live-3d` | test | C | 11295 | 80.8% | 56.9% | -21.4K | -0.33% |

**Guide-price fills: tiers at 50M (every candidate, full budget each)**

| Strategy | Period | Tier | Trades | Fill | Hit | Profit/day | Return/day on cost |
|---|---|---|---|---|---|---|---|
| `baseline` | train | S | 16935 | 22.4% | 7.3% | -17.2K | -0.98% |
| `baseline` | train | A | 33597 | 29.9% | 6.6% | -28.1K | -0.95% |
| `baseline` | train | B | 74033 | 28.6% | 6.3% | -27.1K | -0.84% |
| `baseline` | train | C | 100990 | 36.0% | 3.3% | -19.6K | -0.70% |
| `baseline` | train | X | 36150 | 57.9% | 0.1% | -12.9K | -0.39% |
| `baseline` | test | S | 11012 | 34.2% | 16.7% | -23.5K | -1.00% |
| `baseline` | test | A | 25609 | 38.6% | 13.1% | -32.8K | -0.91% |
| `baseline` | test | B | 55365 | 43.1% | 11.2% | -40.9K | -0.94% |
| `baseline` | test | C | 71309 | 41.0% | 7.7% | -24.7K | -0.82% |
| `baseline` | test | X | 24858 | 57.3% | 0.2% | -10.2K | -0.51% |

### Crash flags

**Guide-price fills: crash flags at 50M (guide price 7 days later)**

| Period | Flagged | Fell | Not flagged | Fell |
|---|---|---|---|---|
| train | 24446 | 54.4% | 242697 | 30.9% |
| test | 27911 | 53.2% | 166093 | 39.3% |

**Real-time fills: crash flags at 50M (guide price 7 days later)**

| Period | Flagged | Fell | Not flagged | Fell |
|---|---|---|---|---|
| train | 9538 | 54.8% | 48435 | 44.5% |
| test | 9780 | 49.1% | 28975 | 45.1% |

The real-time study covers only items with recent real-time trades, over a shorter and later period, so its base rate differs from the guide-price study's.

### Rejected ideas

- **Guide-price percentile bands** (`band-*`, four variants). They fill far more often than the baseline, but a guide price that drifts down to the band's lower edge tends to keep falling within the week. With guide-price fills, only 9-22% of their filled top-10 trades made money and 94-100% of their units were dumped at day 7. They were the worst candidates in training in both studies.
- **Track record from guide-price fills** (`emp-guide-28-lcb`, `emp-guide-56-lcb`). They do not separate items (IC about 0) and lost more than the baseline in both periods.
- **Live quote ranked by expected gp** (`live-1d`, `live-3d`, `live-7d`). These lost 134-385K/day at 50M, mostly through the collapsing markets described under [Tiers](#tiers). At 10M, all three made money in training (+7K to +29K/day) and all lost on held-out data.
- **Live quote without crash-flagged items** (`live-3d-nocrash`). Dropping the flagged items made it worse than `live-3d` in training (Sharpe -0.71 against -0.62). The guide-price crash checks need weeks of history, so they cannot flag a new item that collapses in its first days.
- **Live quote scored by its track record** (`emp-3d-28`, `emp-3d-28-lcb`, `emp-1d-28-lcb`). Of the real-time candidates, these had the fewest dumped units (33-51%) and the highest hit rates, but they still lost 105-220K/day at 50M. At 10M they made +20K to +26K/day held out, but the training results were mixed. Because they score the record alone, they also picked some items whose current live margin was negative, for example a 2.46M buy with a 1.30M sell.

## What changed in the app

- **Data.** `rs3_collector.py` stores the missing real-time hours of the last 7 days and today in `rt_hourly` after its daily job, then folds each missing complete day into `rt_daily`. A real-time failure is logged as a warning and does not fail the job. `backfill_rt.py` fills `rt_hourly` from 2026-07-23 17:00 UTC and `rt_daily` from 2026-07-24. (The folding from stored hours was added after this study; it gives the same `rt_daily` rows. See [strategy.md](strategy.md).)
- **Scan.** For each item, the scan computes `flips::live_estimate` as of the newest `rt_daily` day. It uses a 3-day quote, a 28-day track record and the one-standard-error penalty, which is `emp-3d-28-lcb` above. These settings were fixed before any held-out result. Real-time data whose newest day is more than 3 days old is not shown. Without the table, or without readable rows, the scan works exactly as before.
- **UI.**
  - The table has two new columns: **Live margin** (profit per item after tax at the live instant-sell and instant-buy averages) and **Exp/day** (the track record × the live quantity at your budget, capped at 10% of the live volume).
  - The detail panel adds Live margin and Exp/day tiles, the live buy, sell, quantity and volume, and risk notes when there is no live data, when the live margin is negative, and on how far Exp/day can be trusted.
  - The default sort is still Score.
  - `ui/lib/price-cases.json` holds shared cases that check the UI's live quantity and Exp/day against the backtest's order size and rank key in both Rust and TypeScript.
- **Strategy tab.** A later, separate search for hourly strategies ([strategy.md](strategy.md)) added a Strategy tab. It leaves the Trading Desk and its default Score ranking unchanged.

## Limitations

- **The fill proxies are coarse.**
  - A daily guide price is not a trade price. A daily average below the bid does not guarantee a fill, and an offer can fill on a day whose average stays above it.
  - Daily real-time averages are noisy: in 15% of the days on which both sides traded (43,663 of 285,719), `avg_low` is above `avg_high`. 1,104 of the 4,635 live quotes in the current scan have a negative margin after tax.
- **The trade model is simple.** Each trade is one offer at one price, never re-priced, with one GE limit load and nothing compounded. The 10% volume cap is the only model of market impact.
- **The samples are small.** The real-time test has 23 signal days, about 3 independent cohorts per start day. A held-out real-time result inside its interval says little either way.
- **Many comparisons.** 7 guide-price and 9 real-time candidates, each at 3 budgets and 2 values of K, were compared. Only the pre-declared metric decides anything, and the other rows are context.
- **The item list is today's.** Items that left the GE in the past year are missing, and the items whose price does not fit in 32 bits are skipped.
- **The guide-price test was partly seen.** See [Strategies tested](#strategies-tested).

## Follow-ups

- **Rank live quotes by return on cost.** In both periods, `live-3d`'s high-ROI tiers made money per trade, and ranking by absolute gp lost.
  - Add a candidate that ranks live quotes by margin ÷ buy price, with the same quantity cap. Fix its settings before running it.
  - Judge it only on real-time signals after 2026-10-11, once about 30 more days have been collected. Run with `--rt-test-from 2026-10-12` and train on everything before.
  - Expect a small P&L/day at large budgets: these items are thin, so most of the budget would sit idle.
- **Stay out of collapsing markets.** A few trades on items whose price fell 30-95% within the week decided every live portfolio's result, and some were items only a day or two old. These candidates also need to be fixed in advance and judged on new data in the same way:
  - Skip items with only a few days of history.
  - Skip items whose live prices are falling fast.
  - Require a positive live margin before using the track record.
- **Collector dates.** Stamp each guide-price row with the dump's own date instead of the run date. This removes the off-by-one and the duplicate day where the backfill and the collector meet.
- **Tier X under default filters.** Consider hiding rows that lose money after tax at the model's own prices, as the app already does for negative scores.

## Re-running

The database is opened read-only. From `src-tauri/`:

```bash
cargo run --release --features backtest --bin backtest -- --help
cargo run --release --features backtest --bin backtest -- --db ../rs3_market.db   # full report on stdout
cargo run --release --features backtest --bin backtest -- --db ../rs3_market.db --out report.md
cargo run --release --features backtest --bin backtest -- --db ../rs3_market.db --budgets 50m --k 10 --strategies baseline,live-3d
cargo run --release --features backtest --bin backtest -- --db ../rs3_market.db --trace 4151   # every signal and trade for one item
```

The tables above come from the full default run on the database described under [Data](#data), which takes about a minute. The binary is behind the `backtest` feature so that `pnpm tauri build`, which bundles every enabled binary, leaves it out of the installers. A plain `cargo run --bin backtest` therefore asks for `--features backtest`. The simulator, metrics, look-ahead rules and the UI parity cases are covered by `cargo test` (`src/trade.rs`, `src/backtest.rs`).

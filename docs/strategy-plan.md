# Hourly strategy search: pre-registered plan

Written 2026-10-12 around 22:45 UTC, before any strategy was run on any date. Nothing below may change once a validation number has been printed; any deviation is reported as a deviation in `docs/strategy.md`.

## Goal and bar

Find a Grand Exchange strategy whose **held-out** top-10 portfolio at a **50M** budget has:

- a **win rate** of at least 70%, where win rate = profitable filled trades ÷ filled trades, after GE tax; and
- **positive realised profit per day**.

10M and 250M budgets are reported too but do not enter any decision.

## Data

- `rt_hourly`: the RuneScape Wiki real-time `/1h` buckets, one row per item per hour (`ts` = hour start, UTC). It covers 2026-07-23 17:00 UTC onwards. A bucket with no row for an item means that item did not trade that hour. Missing rows are never forward-filled.
- `history`: daily guide prices. Only the crash flag uses them (see "Market health").
- No train evaluation runs until the backfill has stored every hour through the last train day plus one. The test run needs every hour through 2026-10-12 23:00 UTC, so `backfill_rt.py` is re-run after 2026-10-13 01:00 UTC.

## Cohorts and splits

- **One signal per UTC day**, at 00:00 UTC on signal day `d`. The signal sees buckets up to and including `d-1 23:00`, called `h` below. Nothing can fill in bucket `h` or earlier.
- **Why 00:00 UTC.** The train-period activity profile peaks 17:00-04:00 UTC. In the user's timezone (AEDT, UTC+11), 00:00 UTC is 11:00 local, and the 12-hour entry window then runs to 23:00 local. Outcomes played no part in the choice.
- Each signal day's trades live entirely in buckets `d 00:00` to `d 23:00`, so cohorts never overlap. Each day is one independent unit, and no embargo is needed between splits.
- **Splits**, by signal day:

| Split | Signal days | Days |
|---|---|---|
| Train | 2026-07-31 to 2026-09-08 | 40 |
| Validation | 2026-09-09 to 2026-09-23 | 15 |
| Test | 2026-09-24 to 2026-10-12 | 19 |

- 2026-07-31 is the first day with a full 7-day (168-bucket) lookback.
- The test split is touched **exactly once**, at the end, for the chosen strategy and the runner-up. The harness prints test numbers only behind an explicit flag; the default run prints train and validation.

## Hourly simulator (shared `trade.rs`; the daily `simulate` stays untouched)

- **Prices.** Each pick is a bid `B` and an ask `S`, both fixed at signal time, in whole gp.
- **Order size.** A trip is at most `floor(allocation ÷ B)` units, where allocation = budget ÷ K. It is also capped by the GE limit (below).
- **Entry.** While flat, the bid fills in the first bucket in `h+1` to `h+12` (00:00-11:00 UTC) where:
  - `avg_low` (the instant-sell average) is at or below `B`, and
  - `floor(10% × low_volume)` is at least 1.
  
  The fill quantity is the smallest of: the order size, `floor(10% × low_volume)`, and what is left of the GE limit in the current 4-hour window. It fills at `B`. Partial buys are not accumulated over several buckets.
- **GE limit.** At most the item's limit is bought per 4-hour window. A window starts at the first buy and lasts 4 buckets. A NULL limit is uncapped, and the number of such picks is reported.
- **Exit.** From the bucket after the buy, in each bucket where `avg_high` (the instant-buy average) is at or above `S`, up to `floor(10% × high_volume)` units sell at `S`. Nothing sells in the buy bucket.
- **Re-entry.** Once a position is fully sold in bucket `k`, the bid is live again from `k+1`, but only while `k+1 ≤ h+12`. That allows one position per item at a time. This assumes the player re-places offers within the hour. It is the primary setting; a single-trip row is reported as robustness only.
- **Forced exit.** Units still held after the sales in bucket `h+24` (23:00 UTC) are dumped:
  - at that bucket's `avg_low`, or
  - if it has none, at the latest `avg_low` since the buy bucket.
  
  The dump has no volume cap. The forced-unit share is reported, together with a 2%-haircut robustness row.
- **Tax.** The GE tax (`ge_tax`, 2% rounded down per item, Bonds exempt) applies to every sale, including dumps.
- **Trade (trip).** One buy fill and the sales or dump that close it. Profit = proceeds after tax − quantity × `B`. A trip counts as a win when its profit is above 0.
- **Profit/day.** The mean, over **every** signal day in the split, of that day's total realised P&L (the sum of its trip profits). Days with no picks or no fills count as 0 gp.

## Signals

Shared by every candidate, computed only from buckets at or before `h`:

- **Lookback.** Buckets `h-L+1` to `h`. `active` = the share of those `L` buckets in which both sides traded.
- **Liquidity.** The median over the `L` buckets of `min(low_volume, high_volume)`, with a missing or one-sided bucket counting as 0.
- **Capacity.** `min(order size, floor(10% × liquidity))`.
- **Filters**, all required:
  - `active` ≥ 0.75;
  - capacity ≥ 1;
  - `B` ≥ 100 gp (the app's filter; it also keeps out sub-50 gp items, which pay no tax);
  - ROI = margin ÷ `B` ≤ 150% (the app's filter);
  - margin ≥ `X` × `B`, where margin = `S − B − ge_tax(S)` per unit.
- **Rank key.** margin × capacity, the expected profit of one round trip, highest first, with the item id breaking ties. The top `K` = 10 items are traded, each with budget ÷ 10.

### Quote rules

- `flip`: `B` = median of the hourly `avg_low` over the lookback buckets where instant sells happened. `S` = median of the hourly `avg_high` over the buckets where instant buys happened. Both are rounded to whole gp.
- `dip(k)`: lookback 168 buckets.
  - `B` = mean − `k` × sample standard deviation of the hourly `avg_low`.
  - `S` = mean of the hourly `avg_low` ("sell back at the mean").
  
  A bid resting `k`σ below the 7-day mean fills only on a dip.
- `cons` (conservative): `B` = 25th percentile of `avg_low`, `S` = 40th percentile of `avg_high`. It uses nearest-rank quantiles (index `round((n-1)q)`), as in `stats.rs`.

### Market health (`-h` candidates), all required

- (a) The item's first `rt_hourly` bucket is at or before `h-168`, so it is at least 7 days old in the data.
- (b) 3-day trend: the median `avg_low` over `h-71` to `h` is at least 95% of the median over `h-143` to `h-72`. Both windows must be non-empty.
- (c) The app's guide-price model (`flips::analyze` on guide rows dated `d-91` to `d-1`) does not flag the item Crashing or Crashed. Items without guide history fail.
- (d) No recent margin collapse: over the two-sided buckets in `h-23` to `h`, the median of `avg_high − avg_low − ge_tax(avg_high)` is at least half the quote margin.

## Candidates (frozen grid: 12)

| # | Name | Quote | Lookback (h) | Min margin X | Health |
|---|---|---|---|---|---|
| 1 | `flip24-m1` | flip | 24 | 1% | no |
| 2 | `flip72-m1` | flip | 72 | 1% | no |
| 3 | `flip24-m3` | flip | 24 | 3% | no |
| 4 | `flip72-m3` | flip | 72 | 3% | no |
| 5 | `flip24-m1-h` | flip | 24 | 1% | yes |
| 6 | `flip72-m1-h` | flip | 72 | 1% | yes |
| 7 | `flip24-m3-h` | flip | 24 | 3% | yes |
| 8 | `flip72-m3-h` | flip | 72 | 3% | yes |
| 9 | `dip1-h` | dip, k = 1 | 168 | 1% | yes |
| 10 | `dip2-h` | dip, k = 2 | 168 | 1% | yes |
| 11 | `cons24-h` | cons | 24 | 1% | yes |
| 12 | `cons72-h` | cons | 72 | 1% | yes |

Every reported result carries the grid size (12).

## Metrics

Reported per candidate, split and budget, for top 10:

- days, and picks per day;
- picks with no GE limit;
- fill rate (picks with at least one trip ÷ picks), and trips per filled pick;
- trips (filled trades), with mean and median profit per trip;
- **win rate** with a 95% interval;
- **profit/day** with a 95% interval, and the median day;
- share of days with at least one trip;
- max drawdown of cumulative daily P&L;
- worst-10 trips' share of gross losses;
- forced-unit share;
- distinct items traded, and the top item's share of trips and of profit.

Intervals come from a moving-block bootstrap over signal days, with 3-day blocks, 2,000 resamples and a fixed seed. Each resample recomputes:

- profit/day as the mean of its day P&Ls;
- win rate as pooled wins ÷ pooled trips, which clusters it by day.

## Selection rule (fixed before validation is run)

All of this is judged on top 10 at 50M.

- **Eligible:**
  - at least 100 trips in train and 40 in validation; and
  - at least one trip on 50% or more of the days in each split.
- **Qualified:** eligible, with win rate ≥ 70% **and** profit/day > 0 in **both** train and validation.
- **Chosen:** the qualified candidate with the highest validation profit/day. **Runner-up:** the second highest.
- **Fallback.** If fewer than two candidates qualify, the empty slots are filled in this order:
  1. eligible candidates with validation profit/day > 0, by highest validation win rate;
  2. then any eligible candidate, by highest validation win rate.
  
  Ties go to the higher validation profit/day. Fallback picks are labelled "did not qualify". They go to test only to report the best attempt.

## Test and decision

- One test run reports the chosen strategy, the runner-up and their robustness rows.
- **The decision uses the chosen strategy only.** The bar is met when its test win rate is at least 70% **and** its test profit/day is above 0, both as point estimates. The intervals are reported next to them. There is no switching to the runner-up after the test.
- **If the bar is met:** add a selectable "Strategy" mode to the app, keeping the current default ranking, given only 19 test days. It shows per-item buy and sell targets, the expected win rate and profit/day, and a risk note.
- **If not:** leave the app's ranking unchanged, and document the best result and what data would be needed.

## Robustness rows

These are for the chosen and the runner-up, on every split. They are not used for selection or the decision:

- single trip per pick per day;
- 5% volume share;
- dump at 2% below the forced-exit price;
- budgets of 10M and 250M.

## Known weaknesses, stated in advance

- **Small samples.** 40 / 15 / 19 independent days. Market-wide moves make days correlated, so the intervals will be wide.
- **Multiple testing.** 12 candidates are compared on train and validation, so the best validation number is biased upward. Only the single test run is an unbiased estimate, and a lucky test result is still possible.
- **The test period is not fully unseen.**
  - It overlaps the previous daily study's held-out window (signals 2026-09-12 to 2026-10-04). Failures seen there, such as new items collapsing within days, motivated health checks (a) and (c).
  - The only clean confirmation is a forward test on data after 2026-10-12.
- **Fill proxy.** Hourly averages say nothing about queue position or depth. Volume shares, forced-exit depth and re-entry speed are assumptions.

# Hourly strategy search

The question was whether any Grand Exchange strategy clears this bar on **held-out** data, with the top 10 picks a day at a 50M budget:

- at least **70% of filled trades profitable** after GE tax; and
- **positive realised profit per day**.

**Verdict: the bar was met on the held-out test.**

- **Result.** The pre-registered choice, `cons24-h`, ran over 19 test days (2026-09-24 to 2026-10-12) at 50M:
  - 86.1% of its 209 trades were profitable (95% interval 81.6% to 89.0%);
  - it made 935.8K gp a day on average (95% interval 592.8K to 1.26M).
- **Caveats.** It is a short sample from one stretch of the market, and the fills are estimates from hourly averages. Treat it as promising, not proven. [Limitations](#limitations) lists what could make real trading worse. The only clean confirmation is a forward test on later days.

The search ran on the RuneScape Wiki's hourly real-time prices (`rt_hourly`), with a plan fixed and hashed before any strategy was run ([strategy-plan.md](strategy-plan.md)). Twelve candidates were compared on train and validation. One was chosen by the pre-registered rule, and only it and the runner-up were run on the test split, once. This is in-game gold; nothing here is financial advice.

## Data

`rt_hourly` holds one row per item per hour from the wiki's `/1h` endpoint (`ts` = hour start, UTC). `backfill_rt.py` filled it from the first hour the API has, 2026-07-23 17:00 UTC.

| | |
|---|---|
| Hourly buckets | 1,951, from 2026-07-23 17:00 UTC to 2026-10-12 23:00 UTC, none missing |
| Rows | 4,772,249 |
| Items | 7,087 |
| Items per hour | median 2,472 (min 495, max 3,197) |
| Database after the backfill | 430.7 MB (WAL checkpointed) |

**Checks on the stored data** (`verify_db.py`, read-only). The API spot checks ran at 2026-10-12 23:51 UTC; the rest ran again at 2026-10-13 01:02 UTC, after the final top-up.

- **No gaps.** Every hour from the first bucket to the newest is present.
- **The fold matches.** `rt_daily` used to be built by summing the day's hourly responses. Rebuilding all 81 stored days (440,854 item-days) from `rt_hourly` gives bit-identical rows.
- **The API agrees.** Three hours re-fetched from the API matched the stored rows exactly: 2026-08-02 22:00, 2026-09-12 12:00 and 2026-10-12 20:00 UTC.
- **One thin stretch.** From 2026-08-10 12:00 to 2026-08-11 13:00 UTC, the API returned only about 495-700 items an hour, against a median of about 2,470. That is probably a feed outage or a game update. It falls in train, and a missing bucket simply means nothing traded.

Guide prices (`history`) enter only through the crash flag of health check (c).

## Method

The plan in [strategy-plan.md](strategy-plan.md) was written and hashed before any strategy ran on any date. Its SHA-256 at the time was `b75d791e3f700e0ae21da076cb961ba662c0e6f650c4b8f8d2d84ca0242e1c32`, recorded 2026-10-12 22:40 UTC. This section summarises it, and [Deviations](#deviations-from-the-plan) lists every departure.

### Cohorts

- One signal per UTC day, taken at 00:00 UTC. It uses buckets up to 23:00 the day before.
- The bid can fill from 00:00 to 11:00 UTC. Sales run until 23:00 UTC, and whatever is left is dumped in that bucket.
- Every day's trades therefore start and end within that UTC day. Days never overlap, so each one is an independent unit.
- Profit/day is the mean of the daily P&L over every signal day in a split. Days with no picks or no fills count as 0.

### Hourly simulator (`trade::simulate_hourly`)

The app and the backtest share it.

- **Buy.** While flat, the bid fills at `B` in the first entry bucket whose instant-sell average is at or below `B`. A fill takes at most 10% of that bucket's instant-sell volume, the allocation (budget ÷ 10), and what is left of the GE limit in the current 4-hour window.
- **Sell.** From the next bucket, up to 10% of each bucket's instant-buy volume sells at `S` whenever the instant-buy average reaches `S`.
- **Re-entry.** After a full exit the bid is live again from the next bucket, within the entry window. That allows one position per item at a time.
- **Dump.** Units left after the 23:00 bucket's sales are dumped at that bucket's instant-sell average, or at the latest one since the buy.
- **Tax.** The GE tax applies to every sale, Bonds exempt.
- **Missing buckets.** A missing bucket means no trades: nothing fills and nothing is forward-filled.

### Signals (`hourly.rs`)

Computed only from buckets up to and including the signal bucket:

- **Quote rules.**
  - `flip`: the median hourly instant-sell and instant-buy averages over 24 or 72 hours.
  - `dip`: 1 or 2 standard deviations below the 7-day mean instant-sell average, selling back at that mean.
  - `cons`: the 25th percentile of instant-sell averages and the 40th percentile of instant-buy averages.
- **Filters.**
  - both sides traded in at least 75% of the lookback hours;
  - capacity of at least one unit;
  - buy price at least 100 gp;
  - ROI at most 150%;
  - margin after tax at least 1% or 3% of the bid.
- **Rank.** Margin × capacity, where capacity = min(allocation units, GE limit, 10% of the median thinner-side hourly volume).
- **Market health** (`-h` candidates):
  - at least 7 days of real-time history;
  - the 3-day instant-sell median not more than 5% below the previous 3 days;
  - not flagged crashing or crashed by the app's guide-price model as of the day before;
  - the last day's hourly margins at least half the quote margin.

### Splits and selection

- **Splits.** Train is 40 signal days (2026-07-31 to 2026-09-08), validation 15 (2026-09-09 to 2026-09-23), and test 19 (2026-09-24 to 2026-10-12).
- **Eligible.** At least 100 trades in train and 40 in validation, with trades on at least half the days in each.
- **Qualified.** Win rate ≥ 70% and profit/day > 0 in both train and validation, top 10 at 50M.
- **Choice.** The qualified candidate with the highest validation profit/day is chosen; the second highest is the runner-up.
- **Fallback.** Empty slots go to the eligible candidates with the best validation win rate, those with positive validation profit/day first.
- **Test.** The test split is run once, for those two.

### Sequence of runs

1. **Plan hashed.** 2026-10-12 22:40 UTC, before any strategy was run on any date.
2. **Train-only sanity run.** At about 23:14 UTC, once hours through 2026-09-10 01:00 were stored (`--train-only`, which prints no validation numbers). Traces of Soul rune and Torn grimoire page were checked by hand for:
   - fills;
   - re-entry;
   - GE-limit windows;
   - forced exits.
3. **Train and validation.** At about 23:30 UTC, with all 1,495 buckets through 2026-09-23 23:00 present.
   - The train rows were identical to the train-only run, although 690,000 more rows had been stored in between. Later data does not reach train signals.
4. **App refactor.** Health check (a) now takes the item's first bucket as an argument, so the app can load a short window. Train and validation output stayed byte-identical, apart from the coverage line.
5. **Test.** One run with `--reveal-test`, at 01:03 UTC on 2026-10-13, for `cons24-h` and `flip24-m3-h`.
   - Before it, `backfill_rt.py` stored the last three hours of 2026-10-12, and a gap check confirmed all 1,951 buckets through 2026-10-12 23:00.
   - The run's train and validation sections were identical to step 3's.

## Results: train and validation

All candidates at the decision budget, 50M, top 10. Intervals are 95% moving-block bootstrap intervals over signal days. There are 40 train days and 15 validation days.

| Strategy | Train trips | Train win rate | Train profit/day | Train max DD | Validation trips | Validation win rate | Validation profit/day | Validation max DD | Standing |
|---|---|---|---|---|---|---|---|---|---|
| `flip24-m1` | 502 | 79.5% [75.2%, 83.1%] | 879.4K [512.8K, 1.27M] | 980.8K | 181 | 74.6% [72.4%, 81.6%] | 360.7K [221.8K, 673.0K] | 1.34M | qualified |
| `flip72-m1` | 479 | 77.9% [72.7%, 81.9%] | 652.8K [312.9K, 921.2K] | 2.58M | 186 | 75.8% [67.8%, 80.9%] | 508.8K [90.7K, 791.6K] | 1.88M | qualified |
| `flip24-m3` | 482 | 79.9% [75.5%, 83.8%] | 1.05M [738.1K, 1.39M] | 613.8K | 181 | 74.0% [65.7%, 80.4%] | 552.9K [336.9K, 820.3K] | 1.35M | qualified |
| `flip72-m3` | 437 | 73.7% [66.4%, 79.5%] | 692.9K [319.0K, 985.0K] | 3.01M | 195 | 76.4% [68.1%, 81.6%] | 577.4K [109.0K, 922.8K] | 2.12M | qualified |
| `flip24-m1-h` | 541 | 80.0% [76.4%, 83.3%] | 835.4K [583.6K, 1.13M] | 534.7K | 194 | 76.3% [71.8%, 81.5%] | 506.4K [333.4K, 736.0K] | 1.28M | qualified |
| `flip72-m1-h` | 487 | 80.3% [73.5%, 85.4%] | 565.2K [292.9K, 769.3K] | 1.31M | 192 | 80.7% [76.1%, 83.5%] | 468.1K [58.6K, 674.9K] | 836.8K | qualified |
| `flip24-m3-h` | 506 | 80.2% [76.6%, 83.6%] | 903.7K [649.0K, 1.19M] | 534.7K | 193 | 77.7% [71.1%, 82.9%] | 698.8K [476.6K, 966.0K] | 1.05M | qualified |
| `flip72-m3-h` | 437 | 78.0% [72.2%, 82.1%] | 506.5K [174.4K, 723.8K] | 3.41M | 189 | 79.9% [74.6%, 83.2%] | 599.1K [340.8K, 714.9K] | 188.3K | qualified |
| `dip1-h` | 206 | 93.7% [90.9%, 96.0%] | 327.1K [233.4K, 432.9K] | 238.2K | 61 | 88.5% [82.5%, 95.3%] | 276.7K [171.0K, 388.9K] | 335.3K | qualified |
| `dip2-h` | 90 | 93.3% [88.4%, 97.1%] | 243.3K [151.7K, 333.0K] | 441.5K | 32 | 93.8% [83.3%, 100.0%] | 473.8K [220.9K, 647.6K] | 168.0K | too few trades |
| `cons24-h` | 376 | 84.8% [80.8%, 87.4%] | 904.9K [663.9K, 1.09M] | 42.1K | 170 | 82.9% [78.9%, 89.2%] | 1.11M [836.8K, 1.34M] | 0 | qualified |
| `cons72-h` | 354 | 83.9% [78.1%, 88.7%] | 600.0K [373.1K, 773.4K] | 883.2K | 130 | 77.7% [74.6%, 82.0%] | 508.7K [299.1K, 770.3K] | 352.7K | qualified |

The same table for every budget, with all metrics, is under [All candidates and budgets](#all-candidates-and-budgets).

Every candidate except `dip2-h` (too few trades) cleared the bar on both splits. The bar did not separate the candidates. The selection rule's tie-breaker did: the highest validation profit/day.

- The `dip` rules win most often but fill rarely.
- `cons` (bid at the 25th percentile, ask at the 40th) gave the most profit per day in validation.
- Of the four `flip` pairs with and without the health checks (`-h`), the checks lowered the 50M max drawdown in all four in validation, and in three in train (all but `flip72-m3`).

### Selection

| Strategy | Standing | Train trips | Train win | Train profit/day | Validation trips | Validation win | Validation profit/day |
|---|---|---|---|---|---|---|---|
| flip24-m1 | qualified | 502 | 79.5% | 879.4K | 181 | 74.6% | 360.7K |
| flip72-m1 | qualified | 479 | 77.9% | 652.8K | 186 | 75.8% | 508.8K |
| flip24-m3 | qualified | 482 | 79.9% | 1.05M | 181 | 74.0% | 552.9K |
| flip72-m3 | qualified | 437 | 73.7% | 692.9K | 195 | 76.4% | 577.4K |
| flip24-m1-h | qualified | 541 | 80.0% | 835.4K | 194 | 76.3% | 506.4K |
| flip72-m1-h | qualified | 487 | 80.3% | 565.2K | 192 | 80.7% | 468.1K |
| flip24-m3-h | qualified | 506 | 80.2% | 903.7K | 193 | 77.7% | 698.8K |
| flip72-m3-h | qualified | 437 | 78.0% | 506.5K | 189 | 79.9% | 599.1K |
| dip1-h | qualified | 206 | 93.7% | 327.1K | 61 | 88.5% | 276.7K |
| dip2-h | too few trades | 90 | 93.3% | 243.3K | 32 | 93.8% | 473.8K |
| cons24-h | qualified | 376 | 84.8% | 904.9K | 170 | 82.9% | 1.11M |
| cons72-h | qualified | 354 | 83.9% | 600.0K | 130 | 77.7% | 508.7K |

- Chosen: `cons24-h`
- Runner-up: `flip24-m3-h`
- Candidates compared: 12. The best validation result is biased upward by that choice.

### Robustness on train and validation (not used to select)

| Split | Strategy | Budget | Days | Picks/day | Fill | Trips (per filled pick) | Win rate [95% CI] | Profit/day [95% CI] | Median day | Mean / median trip | Days traded | Max DD | Worst-10 share of losses | Forced units | Items (top: trips / profit) | No GE limit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| train | cons24-h (single trip per pick) | 50.00M | 40 | 9.6 | 70.5% | 270 (1.0) | 82.2% [77.9%, 85.7%] | 688.1K [481.2K, 866.5K] | 561.0K | 101.9K / 82.9K | 97.5% | 100.8K | 48.7% | 20.9% | 79 (6.3% / 17.1%) | 0 |
| train | flip24-m3-h (single trip per pick) | 50.00M | 40 | 9.6 | 87.2% | 334 (1.0) | 75.7% [71.8%, 80.1%] | 487.1K [315.0K, 654.1K] | 488.6K | 58.3K / 56.6K | 97.5% | 935.9K | 50.8% | 24.7% | 99 (5.4% / 10.3%) | 0 |
| validation | cons24-h (single trip per pick) | 50.00M | 15 | 10.0 | 77.3% | 116 (1.0) | 84.5% [78.6%, 91.1%] | 869.7K [601.1K, 1.09M] | 791.0K | 112.5K / 91.5K | 100.0% | 94.0K | 96.8% | 12.2% | 44 (6.9% / 12.1%) | 0 |
| validation | flip24-m3-h (single trip per pick) | 50.00M | 15 | 10.0 | 88.0% | 132 (1.0) | 72.7% [65.0%, 79.1%] | 373.6K [159.9K, 611.1K] | 616.0K | 42.5K / 69.7K | 100.0% | 1.17M | 75.2% | 17.4% | 52 (6.8% / 17.1%) | 0 |
| train | cons24-h (5% volume share) | 50.00M | 40 | 9.6 | 70.2% | 369 (1.4) | 84.3% [80.2%, 86.8%] | 586.8K [423.3K, 732.4K] | 528.5K | 63.6K / 44.1K | 97.5% | 128.1K | 48.4% | 17.5% | 78 (7.6% / 19.3%) | 0 |
| train | flip24-m3-h (5% volume share) | 50.00M | 40 | 9.6 | 86.2% | 490 (1.5) | 79.4% [75.5%, 82.7%] | 497.4K [331.7K, 682.1K] | 460.8K | 40.6K / 29.0K | 97.5% | 553.7K | 48.8% | 20.9% | 98 (7.1% / 12.5%) | 0 |
| validation | cons24-h (5% volume share) | 50.00M | 15 | 10.0 | 77.3% | 162 (1.4) | 82.7% [78.2%, 89.0%] | 699.7K [537.1K, 829.1K] | 522.1K | 64.8K / 48.9K | 100.0% | 0 | 79.8% | 19.0% | 44 (8.0% / 16.9%) | 0 |
| validation | flip24-m3-h (5% volume share) | 50.00M | 15 | 10.0 | 88.0% | 187 (1.4) | 77.0% [70.1%, 82.6%] | 405.1K [241.9K, 610.6K] | 481.6K | 32.5K / 35.1K | 100.0% | 1.08M | 69.8% | 23.4% | 52 (8.0% / 20.8%) | 0 |
| train | cons24-h (dump 2% lower) | 50.00M | 40 | 9.6 | 70.5% | 376 (1.4) | 83.5% [79.0%, 86.2%] | 813.8K [578.4K, 990.9K] | 680.9K | 86.6K / 77.6K | 97.5% | 118.9K | 38.3% | 17.2% | 79 (7.7% / 15.6%) | 0 |
| train | flip24-m3-h (dump 2% lower) | 50.00M | 40 | 9.6 | 87.2% | 506 (1.5) | 78.7% [74.4%, 82.3%] | 787.1K [533.4K, 1.08M] | 677.9K | 62.2K / 56.7K | 97.5% | 785.6K | 39.1% | 20.3% | 99 (6.9% / 15.5%) | 0 |
| validation | cons24-h (dump 2% lower) | 50.00M | 15 | 10.0 | 77.3% | 170 (1.5) | 81.8% [77.1%, 88.5%] | 981.1K [692.5K, 1.23M] | 905.2K | 86.6K / 85.6K | 100.0% | 113.1K | 68.4% | 20.3% | 44 (7.6% / 14.3%) | 0 |
| validation | flip24-m3-h (dump 2% lower) | 50.00M | 15 | 10.0 | 88.0% | 193 (1.5) | 76.7% [69.5%, 81.8%] | 554.4K [314.6K, 828.1K] | 758.2K | 43.1K / 70.2K | 100.0% | 1.19M | 63.1% | 20.1% | 52 (8.8% / 22.5%) | 0 |

## Held-out test

The run covered 19 signal days, 2026-09-24 to 2026-10-12, for the chosen strategy and the runner-up only. The decision uses the chosen strategy's 50M row.

| Split | Strategy | Budget | Days | Picks/day | Fill | Trips (per filled pick) | Win rate [95% CI] | Profit/day [95% CI] | Median day | Mean / median trip | Days traded | Max DD | Worst-10 share of losses | Forced units | Items (top: trips / profit) | No GE limit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| test | cons24-h | 10.00M | 19 | 10.0 | 76.3% | 223 (1.5) | 84.3% [81.8%, 87.4%] | 392.7K [239.0K, 586.6K] | 322.1K | 33.5K / 42.5K | 100.0% | 244.6K | 68.6% | 16.2% | 63 (5.8% / 11.5%) | 0 |
| test | cons24-h | 50.00M | 19 | 10.0 | 75.3% | 209 (1.5) | 86.1% [81.6%, 89.0%] | 935.8K [592.8K, 1.26M] | 1.05M | 85.1K / 104.5K | 100.0% | 954.7K | 77.8% | 12.2% | 48 (9.1% / 13.6%) | 0 |
| test | cons24-h | 250.00M | 19 | 10.0 | 74.7% | 195 (1.4) | 86.2% [81.8%, 89.3%] | 1.29M [296.9K, 2.10M] | 1.63M | 125.2K / 154.6K | 100.0% | 6.86M | 90.8% | 10.1% | 48 (9.2% / 16.5%) | 0 |
| test | flip24-m3-h | 10.00M | 19 | 10.0 | 89.5% | 289 (1.7) | 82.4% [78.9%, 86.7%] | 501.5K [370.5K, 680.2K] | 486.0K | 33.0K / 43.4K | 100.0% | 13.8K | 56.8% | 14.8% | 71 (8.0% / 12.1%) | 0 |
| test | flip24-m3-h | 50.00M | 19 | 10.0 | 88.9% | 266 (1.6) | 80.8% [77.1%, 85.6%] | 1.03M [664.8K, 1.48M] | 1.05M | 73.6K / 86.9K | 100.0% | 920.3K | 60.6% | 18.2% | 69 (8.6% / 16.0%) | 0 |
| test | flip24-m3-h | 250.00M | 19 | 10.0 | 88.9% | 257 (1.5) | 80.2% [76.3%, 85.2%] | 1.23M [478.6K, 2.02M] | 1.20M | 90.8K / 77.7K | 100.0% | 2.92M | 72.5% | 20.0% | 69 (8.9% / 17.9%) | 0 |
| test | cons24-h (single trip per pick) | 50.00M | 19 | 10.0 | 75.3% | 143 (1.0) | 84.6% [81.0%, 86.8%] | 610.5K [308.1K, 912.6K] | 762.4K | 81.1K / 108.7K | 100.0% | 1.38M | 84.4% | 11.5% | 48 (7.7% / 16.0%) | 0 |
| test | flip24-m3-h (single trip per pick) | 50.00M | 19 | 10.0 | 88.9% | 169 (1.0) | 74.6% [70.4%, 78.7%] | 464.2K [140.5K, 804.6K] | 600.6K | 52.2K / 82.5K | 100.0% | 1.79M | 64.7% | 21.6% | 69 (5.9% / 20.6%) | 0 |
| test | cons24-h (5% volume share) | 50.00M | 19 | 10.0 | 74.7% | 197 (1.4) | 86.3% [81.0%, 90.1%] | 581.7K [222.7K, 890.6K] | 730.3K | 56.1K / 66.3K | 100.0% | 1.41M | 85.9% | 11.2% | 48 (9.1% / 15.3%) | 0 |
| test | flip24-m3-h (5% volume share) | 50.00M | 19 | 10.0 | 88.9% | 259 (1.5) | 80.3% [76.5%, 85.2%] | 532.4K [206.5K, 877.2K] | 535.3K | 39.1K / 41.9K | 100.0% | 1.38M | 70.7% | 18.8% | 69 (8.9% / 19.8%) | 0 |
| test | cons24-h (dump 2% lower) | 50.00M | 19 | 10.0 | 75.3% | 209 (1.5) | 84.2% [80.5%, 86.6%] | 844.4K [494.0K, 1.17M] | 1.04M | 76.8K / 104.5K | 100.0% | 1.15M | 71.8% | 12.2% | 48 (9.1% / 15.0%) | 0 |
| test | flip24-m3-h (dump 2% lower) | 50.00M | 19 | 10.0 | 88.9% | 266 (1.6) | 80.1% [75.9%, 85.0%] | 893.2K [525.0K, 1.37M] | 866.7K | 63.8K / 84.1K | 100.0% | 1.03M | 55.0% | 18.2% | 69 (8.6% / 18.0%) | 0 |

Decision (chosen strategy `cons24-h`, top 10 at 50.00M): win rate 86.1% [81.6%, 89.0%], profit/day 935.8K [592.8K, 1.26M]: the bar is met.

What the test says beyond the decision:

- **The test agrees with validation.** `cons24-h` made 82.9% and 1.11M a day at 50M in validation, and 86.1% and 935.8K in test.
- **The runner-up passed too.** `flip24-m3-h` made 80.8% and 1.03M a day. Under the plan, that does not enter the decision.
- **Losses are concentrated.**
  - Every losing trade comes from a forced exit. A sale at the ask always clears the margin, because the margin after tax is at least 1%.
  - At 50M, 12.2% of the units bought were dumped.
  - The 10 worst trades made up 77.8% of all losses.
- **Part of the profit rests on the fill assumptions.** All three robustness rows still clear the bar at 50M, but:
  - halving the volume share to 5% cuts profit per day by 38%, to 581.7K;
  - allowing one trip per pick a day cuts it by 35%, to 610.5K;
  - a 2% lower dump price costs 10%, leaving 844.4K.
- **Bigger budgets do not scale.**
  - 250M earned 1.29M a day, only 38% more than 50M.
  - Its interval is wide (296.9K to 2.10M) and its worst drawdown was 6.86M.
  - 10M earned 392.7K a day.

## Deviations from the plan

There were none in the end.

- **The Strategy view.** The plan said to add a Strategy mode only if the bar was met. The user asked for the view whatever the result: labelled experimental if not validated, with the existing Score ranking kept as the default.
  - It was built before the test ran.
  - Only the pass/fail flag and the test numbers were filled in afterwards.
  - Because the bar was met, the plan's own rule covers it anyway.
- **Everything else ran as written.**
  - The candidates, splits, simulator settings, metrics and selection rule match the plan.
  - The refactor in step 4 changed no output.

## The app's Strategy view

The **Strategy** tab shows:

- the chosen strategy's buy and sell targets for the current signal day, top 10 at the budget set on the Trading Desk (each pick gets budget ÷ 10);
- the day's schedule in UTC and local time, with where "now" falls;
- the held-out test results at 10M, 50M and 250M, with the row nearest your budget marked (profit is not scaled to your budget);
- the risks.

Its badge reads "Passed the held-out test", next to a note that the sample is short and the fills are estimates. The Trading Desk and its Score ranking are unchanged and stay the default tab.

It uses the backtest's code:

- `hourly::daily_pick` for the quotes and filters;
- `hourly::capacity` for the ranking, mirrored in `strategyPicks` (`ui/lib/flips.ts`), with parity cases shared in `ui/lib/strategy-cases.json`.

A test checks that a pick computed from the app's short window equals the pick from the full series for every candidate.

Differences from the backtest:

- **Hourly data loaded.** The scan loads only the buckets the rule reads: 144 for `cons24-h`, because of the trend check. It also loads each item's first stored bucket. Both are primary-key lookups, so the cost does not grow with the table.
- **Crash flag (health check c).** It uses the scan's guide window, the 90 days to today. The backtest uses rows up to the day before the signal, so that a guide price published during the trading day cannot leak in. In the app, today's guide price is simply the present.
- **Late start.** The signal day is the newest UTC day whose 23:00 bucket is stored. The collector treats an hour as final only once it has been closed for an hour, so that bucket arrives after 01:00 UTC at the earliest. The backtest's bids were live from 00:00 UTC, so real use loses at least the first of the 12 entry hours.
- **Freshness.** Data more than 3 days old shows no picks. Picks for a day that has ended are marked out of date.

## Limitations

- **Small samples.** There are 40 train, 15 validation and 19 test days. Days share market-wide moves, which is why the intervals are block-bootstrap intervals over days, and they are wide.
- **Multiple testing.** Twelve candidates were compared. The chosen strategy's validation numbers are biased upward by that choice. The single test run is the unbiased estimate, but a lucky or unlucky 19-day window is still possible.
- **The test window is not fully unseen.** It overlaps the earlier daily study's held-out window (signals 2026-09-12 to 2026-10-04). Lessons from that window shaped health checks (a) and (c). The only clean confirmation is a forward test on days after 2026-10-12.
- **The fill proxy is optimistic in ways the data cannot show.**
  - A bid counts as filled when the hour's average instant-sell price is at or below it. That ignores queue position: other players' offers at the same price fill first.
  - A trip may take at most 10% of the hour's volume. The robustness rows halve that.
  - A forced exit has no depth limit.
  - Re-entry assumes offers are replaced within the hour.
- **Hourly averages are not quotes.** Prices inside an hour vary, and the average can sit on either side of a target that only some trades crossed.
- **Regime.** Every result comes from late July to mid-October 2026. Game updates, events and item releases can change which items are flippable.

## How to re-run

```bash
python backfill_rt.py      # fill or top up rt_hourly (resumable; skips stored hours)
cd src-tauri
cargo run --release --features backtest --bin backtest -- --db ../rs3_market.db --hourly --train-only
cargo run --release --features backtest --bin backtest -- --db ../rs3_market.db --hourly            # train, validation, selection
cargo run --release --features backtest --bin backtest -- --db ../rs3_market.db --hourly --trace 566 # every signal and trip of one item
cargo run --release --features backtest --bin backtest -- --db ../rs3_market.db --hourly --reveal-test --out test.md
```

- **`--reveal-test` is the one-time test run.** It was used once for the numbers above. Running it again gives the same numbers, but the test split can no longer serve as an unseen check.
- **`--db`.** Release builds look for the database only in the current directory and next to the executable, so it is passed explicitly from `src-tauri`.
- **Tests.** `cargo test` covers the hourly simulator in `trade.rs`:
  - fills need volume;
  - nothing sells in the buy bucket;
  - forced exit;
  - GE limit per 4 hours;
  - tax;
  - the entry window.
  
  It also covers the signals and the window parity in `hourly.rs`, and the evaluation, look-ahead and selection rule in `backtest.rs`.

## All candidates and budgets

Train and validation, every candidate and budget, as the harness printed them.

| Split | Strategy | Budget | Days | Picks/day | Fill | Trips (per filled pick) | Win rate [95% CI] | Profit/day [95% CI] | Median day | Mean / median trip | Days traded | Max DD | Worst-10 share of losses | Forced units | Items (top: trips / profit) | No GE limit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| train | flip24-m1 | 10.00M | 40 | 9.8 | 82.8% | 491 (1.5) | 80.0% [74.9%, 83.5%] | 494.0K [310.5K, 647.5K] | 413.2K | 40.2K / 38.1K | 97.5% | 443.9K | 36.4% | 22.8% | 93 (6.5% / 10.4%) | 0 |
| train | flip24-m1 | 50.00M | 40 | 9.8 | 85.9% | 502 (1.5) | 79.5% [75.2%, 83.1%] | 879.4K [512.8K, 1.27M] | 700.7K | 70.1K / 75.3K | 97.5% | 980.8K | 35.5% | 22.7% | 84 (6.6% / 15.0%) | 0 |
| train | flip24-m1 | 250.00M | 40 | 9.8 | 86.7% | 485 (1.4) | 77.5% [73.2%, 81.4%] | 989.8K [518.0K, 1.45M] | 773.8K | 81.6K / 75.6K | 97.5% | 878.1K | 39.0% | 23.7% | 84 (6.0% / 14.2%) | 0 |
| train | flip72-m1 | 10.00M | 40 | 10.0 | 81.2% | 489 (1.5) | 77.3% [71.4%, 82.3%] | 368.5K [227.0K, 484.1K] | 376.3K | 30.1K / 32.7K | 100.0% | 1.04M | 30.6% | 30.2% | 72 (7.4% / 10.7%) | 0 |
| train | flip72-m1 | 50.00M | 40 | 10.0 | 80.8% | 479 (1.5) | 77.9% [72.7%, 81.9%] | 652.8K [312.9K, 921.2K] | 722.4K | 54.5K / 61.4K | 100.0% | 2.58M | 39.9% | 29.3% | 62 (8.1% / 17.4%) | 0 |
| train | flip72-m1 | 250.00M | 40 | 10.0 | 81.0% | 466 (1.4) | 77.0% [71.5%, 81.2%] | 750.1K [132.6K, 1.21M] | 773.7K | 64.4K / 64.2K | 100.0% | 6.98M | 50.3% | 31.6% | 63 (8.2% / 18.8%) | 0 |
| train | flip24-m3 | 10.00M | 40 | 9.6 | 83.3% | 491 (1.5) | 81.1% [75.8%, 84.5%] | 535.0K [368.2K, 674.6K] | 487.7K | 43.6K / 38.9K | 97.5% | 85.6K | 35.3% | 22.9% | 95 (6.5% / 9.6%) | 0 |
| train | flip24-m3 | 50.00M | 40 | 9.6 | 84.6% | 482 (1.5) | 79.9% [75.5%, 83.8%] | 1.05M [738.1K, 1.39M] | 935.6K | 87.5K / 75.6K | 97.5% | 613.8K | 37.2% | 23.1% | 91 (7.5% / 12.7%) | 0 |
| train | flip24-m3 | 250.00M | 40 | 9.6 | 84.6% | 470 (1.5) | 78.7% [74.0%, 82.7%] | 1.22M [792.9K, 1.67M] | 1.08M | 104.2K / 72.4K | 97.5% | 1.02M | 49.4% | 24.6% | 91 (7.0% / 12.3%) | 0 |
| train | flip72-m3 | 10.00M | 40 | 9.8 | 80.3% | 459 (1.5) | 75.6% [69.4%, 80.8%] | 368.9K [231.0K, 480.0K] | 450.2K | 32.2K / 37.0K | 100.0% | 1.13M | 30.0% | 29.5% | 70 (6.5% / 10.7%) | 0 |
| train | flip72-m3 | 50.00M | 40 | 9.8 | 79.5% | 437 (1.4) | 73.7% [66.4%, 79.5%] | 692.9K [319.0K, 985.0K] | 820.1K | 63.4K / 61.6K | 100.0% | 3.01M | 38.6% | 33.4% | 70 (6.6% / 17.5%) | 0 |
| train | flip72-m3 | 250.00M | 40 | 9.8 | 79.5% | 430 (1.4) | 73.3% [65.7%, 79.1%] | 824.0K [209.6K, 1.30M] | 709.6K | 76.7K / 61.0K | 100.0% | 7.57M | 52.2% | 35.6% | 70 (6.5% / 20.1%) | 0 |
| train | flip24-m1-h | 10.00M | 40 | 9.6 | 86.2% | 538 (1.6) | 81.4% [78.1%, 84.4%] | 467.0K [356.1K, 575.9K] | 434.7K | 34.7K / 33.6K | 97.5% | 44.3K | 36.5% | 17.4% | 94 (6.7% / 13.2%) | 0 |
| train | flip24-m1-h | 50.00M | 40 | 9.6 | 88.5% | 541 (1.6) | 80.0% [76.4%, 83.3%] | 835.4K [583.6K, 1.13M] | 690.2K | 61.8K / 64.5K | 97.5% | 534.7K | 32.5% | 20.0% | 87 (7.9% / 12.9%) | 0 |
| train | flip24-m1-h | 250.00M | 40 | 9.6 | 88.5% | 523 (1.5) | 79.5% [75.7%, 83.0%] | 872.4K [364.8K, 1.28M] | 669.9K | 66.7K / 66.7K | 97.5% | 3.87M | 46.5% | 19.1% | 87 (7.8% / 13.0%) | 0 |
| train | flip72-m1-h | 10.00M | 40 | 9.8 | 81.6% | 500 (1.6) | 81.0% [75.6%, 85.5%] | 293.3K [184.7K, 378.5K] | 283.2K | 23.5K / 26.5K | 100.0% | 544.9K | 38.0% | 17.1% | 74 (8.4% / 12.4%) | 0 |
| train | flip72-m1-h | 50.00M | 40 | 9.8 | 82.1% | 487 (1.5) | 80.3% [73.5%, 85.4%] | 565.2K [292.9K, 769.3K] | 581.3K | 46.4K / 53.8K | 100.0% | 1.31M | 39.4% | 20.3% | 71 (9.9% / 13.1%) | 0 |
| train | flip72-m1-h | 250.00M | 40 | 9.8 | 82.1% | 477 (1.5) | 79.9% [72.9%, 85.2%] | 647.2K [83.8K, 1.07M] | 749.7K | 54.3K / 55.0K | 100.0% | 5.56M | 56.5% | 22.3% | 71 (9.9% / 12.6%) | 0 |
| train | flip24-m3-h | 10.00M | 40 | 9.6 | 86.7% | 525 (1.6) | 81.7% [78.4%, 84.6%] | 485.0K [373.0K, 589.8K] | 434.5K | 36.9K / 36.3K | 97.5% | 44.3K | 41.0% | 15.7% | 98 (6.5% / 12.7%) | 0 |
| train | flip24-m3-h | 50.00M | 40 | 9.6 | 87.2% | 506 (1.5) | 80.2% [76.6%, 83.6%] | 903.7K [649.0K, 1.19M] | 815.2K | 71.4K / 57.5K | 97.5% | 534.7K | 44.6% | 20.3% | 99 (6.9% / 13.6%) | 0 |
| train | flip24-m3-h | 250.00M | 40 | 9.6 | 87.2% | 498 (1.5) | 79.7% [75.8%, 83.2%] | 945.4K [415.4K, 1.45M] | 902.6K | 75.9K / 56.2K | 97.5% | 3.70M | 58.0% | 22.3% | 99 (7.0% / 13.2%) | 0 |
| train | flip72-m3-h | 10.00M | 40 | 9.6 | 80.6% | 464 (1.5) | 79.5% [73.7%, 83.3%] | 295.8K [178.6K, 373.3K] | 308.2K | 25.5K / 32.1K | 97.5% | 544.9K | 36.1% | 17.0% | 81 (6.5% / 13.3%) | 0 |
| train | flip72-m3-h | 50.00M | 40 | 9.6 | 80.6% | 437 (1.4) | 78.0% [72.2%, 82.1%] | 506.5K [174.4K, 723.8K] | 555.0K | 46.4K / 40.7K | 97.5% | 3.41M | 46.6% | 22.2% | 81 (6.6% / 13.3%) | 0 |
| train | flip72-m3-h | 250.00M | 40 | 9.6 | 80.6% | 434 (1.4) | 77.6% [71.5%, 81.7%] | 526.0K [-26.2K, 902.0K] | 564.2K | 48.5K / 38.9K | 97.5% | 5.64M | 62.8% | 24.7% | 81 (6.7% / 17.6%) | 0 |
| train | dip1-h | 10.00M | 40 | 10.0 | 38.2% | 225 (1.5) | 94.7% [92.2%, 96.7%] | 162.1K [131.6K, 200.5K] | 155.8K | 28.8K / 21.8K | 100.0% | 205.1K | 98.0% | 5.8% | 47 (6.2% / 11.8%) | 0 |
| train | dip1-h | 50.00M | 40 | 10.0 | 36.0% | 206 (1.4) | 93.7% [90.9%, 96.0%] | 327.1K [233.4K, 432.9K] | 271.9K | 63.5K / 43.9K | 100.0% | 238.2K | 97.9% | 4.8% | 46 (6.3% / 19.3%) | 0 |
| train | dip1-h | 250.00M | 40 | 10.0 | 36.0% | 199 (1.4) | 93.0% [90.2%, 95.3%] | 435.6K [320.0K, 570.2K] | 368.1K | 87.6K / 44.6K | 100.0% | 184.9K | 96.6% | 3.8% | 46 (6.0% / 15.5%) | 0 |
| train | dip2-h | 10.00M | 40 | 10.0 | 16.2% | 88 (1.4) | 93.2% [88.4%, 96.9%] | 116.0K [77.5K, 154.4K] | 62.3K | 52.7K / 32.4K | 77.5% | 123.1K | 100.0% | 8.8% | 37 (8.0% / 13.8%) | 0 |
| train | dip2-h | 50.00M | 40 | 10.0 | 17.2% | 90 (1.3) | 93.3% [88.4%, 97.1%] | 243.3K [151.7K, 333.0K] | 158.6K | 108.2K / 56.7K | 80.0% | 441.5K | 100.0% | 8.2% | 40 (6.7% / 24.5%) | 0 |
| train | dip2-h | 250.00M | 40 | 10.0 | 17.2% | 90 (1.3) | 93.3% [88.4%, 97.1%] | 387.1K [208.1K, 582.4K] | 141.4K | 172.0K / 59.8K | 80.0% | 251.8K | 100.0% | 8.0% | 40 (6.7% / 30.1%) | 0 |
| train | cons24-h | 10.00M | 40 | 9.6 | 71.5% | 407 (1.5) | 85.7% [82.5%, 88.8%] | 418.8K [301.9K, 517.6K] | 403.8K | 41.2K / 36.3K | 97.5% | 59.3K | 56.6% | 14.0% | 87 (6.9% / 13.1%) | 0 |
| train | cons24-h | 50.00M | 40 | 9.6 | 70.5% | 376 (1.4) | 84.8% [80.8%, 87.4%] | 904.9K [663.9K, 1.09M] | 782.8K | 96.3K / 79.3K | 97.5% | 42.1K | 44.2% | 17.2% | 79 (7.7% / 14.7%) | 0 |
| train | cons24-h | 250.00M | 40 | 9.6 | 70.5% | 370 (1.4) | 84.9% [80.8%, 87.5%] | 1.39M [934.6K, 1.84M] | 1.12M | 150.8K / 88.7K | 97.5% | 1.46M | 57.7% | 17.4% | 79 (7.6% / 21.6%) | 0 |
| train | cons72-h | 10.00M | 40 | 9.7 | 64.7% | 354 (1.4) | 83.9% [77.5%, 89.6%] | 265.2K [166.0K, 356.8K] | 323.6K | 30.0K / 31.5K | 100.0% | 362.4K | 50.5% | 21.2% | 68 (7.9% / 13.4%) | 0 |
| train | cons72-h | 50.00M | 40 | 9.7 | 64.9% | 354 (1.4) | 83.9% [78.1%, 88.7%] | 600.0K [373.1K, 773.4K] | 524.5K | 67.8K / 64.2K | 100.0% | 883.2K | 52.1% | 19.3% | 66 (11.3% / 11.4%) | 0 |
| train | cons72-h | 250.00M | 40 | 9.7 | 64.7% | 347 (1.4) | 83.6% [77.8%, 88.4%] | 792.0K [300.2K, 1.11M] | 773.9K | 91.3K / 65.0K | 100.0% | 4.51M | 68.7% | 20.6% | 66 (11.2% / 14.2%) | 0 |
| validation | flip24-m1 | 10.00M | 15 | 10.0 | 86.0% | 187 (1.4) | 73.8% [68.6%, 78.5%] | 329.2K [178.4K, 460.4K] | 414.6K | 26.4K / 36.6K | 100.0% | 417.5K | 57.1% | 24.3% | 54 (8.0% / 17.7%) | 0 |
| validation | flip24-m1 | 50.00M | 15 | 10.0 | 84.7% | 181 (1.4) | 74.6% [72.4%, 81.6%] | 360.7K [221.8K, 673.0K] | 386.2K | 29.9K / 73.1K | 100.0% | 1.34M | 60.1% | 23.4% | 46 (10.5% / 37.4%) | 0 |
| validation | flip24-m1 | 250.00M | 15 | 10.0 | 84.0% | 173 (1.4) | 72.3% [69.8%, 80.1%] | 766.8K [174.3K, 1.81M] | 346.9K | 66.5K / 74.5K | 100.0% | 1.78M | 62.4% | 24.8% | 45 (8.7% / 44.8%) | 0 |
| validation | flip72-m1 | 10.00M | 15 | 10.0 | 86.7% | 203 (1.6) | 79.3% [71.6%, 84.6%] | 318.4K [163.2K, 455.4K] | 321.2K | 23.5K / 34.9K | 100.0% | 387.3K | 58.9% | 13.1% | 36 (12.8% / 19.7%) | 0 |
| validation | flip72-m1 | 50.00M | 15 | 10.0 | 82.0% | 186 (1.5) | 75.8% [67.8%, 80.9%] | 508.8K [90.7K, 791.6K] | 575.4K | 41.0K / 63.1K | 100.0% | 1.88M | 60.8% | 21.8% | 36 (11.3% / 19.7%) | 0 |
| validation | flip72-m1 | 250.00M | 15 | 10.0 | 81.3% | 179 (1.5) | 75.4% [67.7%, 80.0%] | 842.5K [128.6K, 1.58M] | 889.6K | 70.6K / 67.3K | 100.0% | 3.91M | 62.5% | 26.0% | 36 (10.6% / 25.2%) | 0 |
| validation | flip24-m3 | 10.00M | 15 | 10.0 | 86.0% | 187 (1.4) | 73.8% [68.6%, 78.5%] | 329.2K [178.4K, 460.4K] | 414.6K | 26.4K / 36.6K | 100.0% | 417.5K | 57.1% | 24.3% | 54 (8.0% / 17.7%) | 0 |
| validation | flip24-m3 | 50.00M | 15 | 10.0 | 85.3% | 181 (1.4) | 74.0% [65.7%, 80.4%] | 552.9K [336.9K, 820.3K] | 515.2K | 45.8K / 65.0K | 100.0% | 1.35M | 61.4% | 27.8% | 53 (9.4% / 25.1%) | 0 |
| validation | flip24-m3 | 250.00M | 15 | 10.0 | 85.3% | 173 (1.4) | 73.4% [65.6%, 80.0%] | 1.03M [345.9K, 1.80M] | 795.3K | 89.5K / 63.0K | 100.0% | 1.28M | 64.4% | 27.8% | 53 (8.1% / 43.1%) | 0 |
| validation | flip72-m3 | 10.00M | 15 | 10.0 | 87.3% | 206 (1.6) | 79.1% [71.9%, 83.4%] | 320.5K [153.4K, 468.6K] | 321.2K | 23.3K / 35.2K | 100.0% | 387.3K | 57.3% | 12.6% | 37 (10.7% / 19.6%) | 0 |
| validation | flip72-m3 | 50.00M | 15 | 10.0 | 87.3% | 195 (1.5) | 76.4% [68.1%, 81.6%] | 577.4K [109.0K, 922.8K] | 527.5K | 44.4K / 57.3K | 100.0% | 2.12M | 62.6% | 20.4% | 37 (10.8% / 19.8%) | 0 |
| validation | flip72-m3 | 250.00M | 15 | 10.0 | 87.3% | 190 (1.5) | 76.3% [68.3%, 81.1%] | 943.5K [47.9K, 1.65M] | 756.7K | 74.5K / 57.6K | 100.0% | 3.98M | 64.6% | 23.5% | 37 (11.1% / 22.5%) | 0 |
| validation | flip24-m1-h | 10.00M | 15 | 10.0 | 87.3% | 212 (1.6) | 81.6% [72.6%, 87.0%] | 459.0K [329.3K, 589.7K] | 479.1K | 32.5K / 36.3K | 100.0% | 141.9K | 65.7% | 13.6% | 51 (9.0% / 12.7%) | 0 |
| validation | flip24-m1-h | 50.00M | 15 | 10.0 | 88.0% | 194 (1.5) | 76.3% [71.8%, 81.5%] | 506.4K [333.4K, 736.0K] | 610.7K | 39.2K / 70.5K | 100.0% | 1.28M | 62.2% | 23.3% | 50 (12.4% / 27.9%) | 0 |
| validation | flip24-m1-h | 250.00M | 15 | 10.0 | 87.3% | 185 (1.4) | 74.6% [70.5%, 81.0%] | 884.2K [430.1K, 1.66M] | 705.7K | 71.7K / 69.2K | 100.0% | 914.2K | 62.1% | 23.4% | 50 (10.8% / 38.9%) | 0 |
| validation | flip72-m1-h | 10.00M | 15 | 10.0 | 84.0% | 193 (1.5) | 79.3% [74.7%, 82.3%] | 261.8K [94.4K, 376.6K] | 304.8K | 20.3K / 33.6K | 100.0% | 382.8K | 58.8% | 16.3% | 35 (12.4% / 19.9%) | 0 |
| validation | flip72-m1-h | 50.00M | 15 | 10.0 | 86.0% | 192 (1.5) | 80.7% [76.1%, 83.5%] | 468.1K [58.6K, 674.9K] | 432.1K | 36.6K / 57.0K | 100.0% | 836.8K | 59.7% | 18.9% | 33 (12.5% / 21.3%) | 0 |
| validation | flip72-m1-h | 250.00M | 15 | 10.0 | 86.0% | 188 (1.5) | 80.9% [76.0%, 83.6%] | 679.8K [-54.9K, 1.21M] | 364.2K | 54.2K / 57.0K | 100.0% | 1.05M | 67.4% | 21.6% | 33 (12.8% / 20.8%) | 0 |
| validation | flip24-m3-h | 10.00M | 15 | 10.0 | 88.0% | 216 (1.6) | 82.4% [76.3%, 86.6%] | 488.4K [354.5K, 622.5K] | 522.3K | 33.9K / 37.0K | 100.0% | 141.9K | 66.7% | 13.1% | 53 (7.4% / 13.1%) | 0 |
| validation | flip24-m3-h | 50.00M | 15 | 10.0 | 88.0% | 193 (1.5) | 77.7% [71.1%, 82.9%] | 698.8K [476.6K, 966.0K] | 844.3K | 54.3K / 73.2K | 100.0% | 1.05M | 68.7% | 20.1% | 52 (8.8% / 19.8%) | 0 |
| validation | flip24-m3-h | 250.00M | 15 | 10.0 | 88.0% | 186 (1.4) | 76.9% [70.2%, 82.4%] | 1.29M [665.4K, 1.91M] | 1.18M | 104.4K / 69.7K | 100.0% | 907.5K | 68.4% | 19.0% | 52 (8.1% / 34.4%) | 0 |
| validation | flip72-m3-h | 10.00M | 15 | 10.0 | 85.3% | 198 (1.5) | 80.3% [75.0%, 83.3%] | 296.9K [157.5K, 378.4K] | 335.9K | 22.5K / 34.0K | 100.0% | 166.1K | 61.9% | 13.5% | 40 (9.6% / 15.2%) | 0 |
| validation | flip72-m3-h | 50.00M | 15 | 10.0 | 85.3% | 189 (1.5) | 79.9% [74.6%, 83.2%] | 599.1K [340.8K, 714.9K] | 608.9K | 47.5K / 47.9K | 100.0% | 188.3K | 69.0% | 16.9% | 40 (9.5% / 15.4%) | 0 |
| validation | flip72-m3-h | 250.00M | 15 | 10.0 | 85.3% | 185 (1.4) | 80.0% [74.6%, 83.1%] | 751.4K [92.9K, 1.21M] | 601.0K | 60.9K / 47.9K | 100.0% | 1.87M | 79.5% | 18.9% | 40 (9.7% / 16.8%) | 0 |
| validation | dip1-h | 10.00M | 15 | 10.0 | 32.7% | 75 (1.5) | 90.7% [85.2%, 96.3%] | 98.8K [65.1K, 154.5K] | 83.1K | 19.8K / 19.9K | 100.0% | 85.8K | 100.0% | 10.7% | 27 (10.7% / 17.9%) | 0 |
| validation | dip1-h | 50.00M | 15 | 10.0 | 30.0% | 61 (1.4) | 88.5% [82.5%, 95.3%] | 276.7K [171.0K, 388.9K] | 217.0K | 68.0K / 57.1K | 100.0% | 335.3K | 100.0% | 13.1% | 26 (11.5% / 20.2%) | 0 |
| validation | dip1-h | 250.00M | 15 | 10.0 | 30.0% | 57 (1.3) | 87.7% [81.5%, 94.9%] | 538.3K [281.4K, 635.4K] | 480.3K | 141.7K / 57.1K | 100.0% | 335.3K | 100.0% | 10.5% | 26 (10.5% / 28.3%) | 0 |
| validation | dip2-h | 10.00M | 15 | 10.0 | 14.7% | 35 (1.6) | 94.3% [83.3%, 100.0%] | 156.3K [66.6K, 238.1K] | 77.3K | 67.0K / 56.3K | 86.7% | 41.1K | 100.0% | 7.7% | 17 (22.9% / 18.4%) | 0 |
| validation | dip2-h | 50.00M | 15 | 10.0 | 15.3% | 32 (1.4) | 93.8% [83.3%, 100.0%] | 473.8K [220.9K, 647.6K] | 251.2K | 222.1K / 153.2K | 86.7% | 168.0K | 100.0% | 9.7% | 17 (18.8% / 14.7%) | 0 |
| validation | dip2-h | 250.00M | 15 | 10.0 | 15.3% | 30 (1.3) | 93.3% [82.8%, 100.0%] | 665.6K [256.7K, 988.6K] | 376.9K | 332.8K / 264.9K | 86.7% | 168.0K | 100.0% | 10.1% | 17 (20.0% / 18.4%) | 0 |
| validation | cons24-h | 10.00M | 15 | 10.0 | 74.7% | 173 (1.5) | 83.8% [80.5%, 88.8%] | 527.2K [381.6K, 724.4K] | 501.8K | 45.7K / 40.5K | 100.0% | 0 | 77.5% | 19.4% | 44 (9.2% / 11.2%) | 0 |
| validation | cons24-h | 50.00M | 15 | 10.0 | 77.3% | 170 (1.5) | 82.9% [78.9%, 89.2%] | 1.11M [836.8K, 1.34M] | 971.7K | 97.7K / 88.0K | 100.0% | 0 | 75.8% | 20.3% | 44 (7.6% / 13.9%) | 0 |
| validation | cons24-h | 250.00M | 15 | 10.0 | 77.3% | 166 (1.4) | 83.7% [79.5%, 89.4%] | 1.64M [1.10M, 2.12M] | 1.81M | 148.2K / 92.3K | 100.0% | 93.0K | 81.2% | 23.5% | 44 (7.8% / 24.9%) | 0 |
| validation | cons72-h | 10.00M | 15 | 10.0 | 63.3% | 127 (1.3) | 79.5% [76.4%, 83.6%] | 257.8K [195.9K, 338.9K] | 262.3K | 30.5K / 34.4K | 100.0% | 27.5K | 78.0% | 11.3% | 38 (8.7% / 10.8%) | 0 |
| validation | cons72-h | 50.00M | 15 | 10.0 | 68.0% | 130 (1.3) | 77.7% [74.6%, 82.0%] | 508.7K [299.1K, 770.3K] | 554.2K | 58.7K / 63.0K | 100.0% | 352.7K | 72.8% | 19.2% | 39 (8.5% / 12.1%) | 0 |
| validation | cons72-h | 250.00M | 15 | 10.0 | 67.3% | 126 (1.2) | 77.8% [74.3%, 81.8%] | 814.2K [377.8K, 1.23M] | 348.0K | 96.9K / 66.3K | 100.0% | 434.7K | 75.6% | 20.2% | 39 (8.7% / 19.4%) | 0 |

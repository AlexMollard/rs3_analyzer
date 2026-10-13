"""
RS3 real-time price backfill
Fills rt_hourly with every hourly bucket of the RuneScape Wiki real-time API (instant-buy and
instant-sell averages and volumes per item), from the first hour of data to the last settled hour,
then folds each missing complete UTC day into rt_daily. Hours and days already stored are skipped,
so it is safe to stop and re-run.
"""

import sys

from rs3_collector import (DB_PATH, RT_FIRST_DAY, RT_FIRST_HOUR, USER_AGENT, connect_db, ensure_schema,
                           fold_rt_days, hour_label, last_complete_rt_day, last_complete_rt_hour,
                           missing_rt_days, missing_rt_hours, update_rt_hours)


def backfill_rt(db_path=DB_PATH):
    """Fetches every missing hour (one bulk request each), then folds the missing days.

    Returns the failures: a list of (hour or day, error).
    """
    conn = connect_db(db_path)
    try:
        ensure_schema(conn)
        last_hour = last_complete_rt_hour()
        hours = missing_rt_hours(conn, RT_FIRST_HOUR, last_hour)
        print(f"Real-time backfill {hour_label(RT_FIRST_HOUR)} to {hour_label(last_hour)}: "
              f"{len(hours)} hour(s) to fetch, one request each")
        done = [0]

        def report(line):
            done[0] += 1
            print(f"[{done[0]}/{len(hours)}] {line}", flush=True)

        stored_hours, failed_hours = update_rt_hours(conn, hours, {"User-Agent": USER_AGENT}, report=report)
        stored_days, incomplete_days = fold_rt_days(conn, missing_rt_days(conn, RT_FIRST_DAY, last_complete_rt_day()))
        h_first, h_last, h_rows, h_items, h_count = conn.execute(
            "SELECT MIN(ts), MAX(ts), COUNT(*), COUNT(DISTINCT item_id), COUNT(DISTINCT ts) FROM rt_hourly").fetchone()
        d_first, d_last, d_rows, d_items, d_count = conn.execute(
            "SELECT MIN(record_date), MAX(record_date), COUNT(*), COUNT(DISTINCT item_id), "
            "COUNT(DISTINCT record_date) FROM rt_daily").fetchone()
    finally:
        conn.close()

    failures = ([(hour_label(ts), error) for ts, error in failed_hours]
                + [(day, "hours missing") for day in incomplete_days])
    print("=" * 60)
    print("REAL-TIME BACKFILL COMPLETE" if not failures else "REAL-TIME BACKFILL FINISHED WITH FAILURES")
    print(f"Hours stored this run: {len(stored_hours)}, failed: {len(failed_hours)}; "
          f"days folded: {len(stored_days)}, incomplete: {len(incomplete_days)}")
    if h_count:
        print(f"rt_hourly: {h_rows:,} rows, {h_items:,} items, {h_count} hours "
              f"({hour_label(h_first)} to {hour_label(h_last)})")
    print(f"rt_daily: {d_rows:,} rows, {d_items:,} items, {d_count} days ({d_first} to {d_last})")
    for where, error in failures:
        print(f"Failed {where}: {error}")
    return failures


if __name__ == "__main__":
    # Avoid UnicodeEncodeError on cp1252 consoles
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    try:
        remaining = backfill_rt()
    except KeyboardInterrupt:
        print("\n\nBackfill interrupted by user")
        sys.exit(130)
    except Exception as e:
        print(f"\n\nFatal error: {e}")
        sys.exit(1)
    sys.exit(1 if remaining else 0)

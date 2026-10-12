"""
RS3 real-time price backfill
Fills rt_daily with daily instant-buy/instant-sell averages and volumes from the RuneScape Wiki
real-time API, from the first complete day of data to yesterday. Days already stored are skipped,
so it is safe to re-run.
"""

import sys

from rs3_collector import (DB_PATH, RT_FIRST_DAY, USER_AGENT, connect_db, ensure_schema,
                           last_complete_rt_day, missing_rt_days, update_rt_days)


def backfill_rt(db_path=DB_PATH):
    """Fetches every missing day (24 bulk requests each). Returns the (day, error) failures."""
    conn = connect_db(db_path)
    try:
        ensure_schema(conn)
        last_day = last_complete_rt_day()
        days = missing_rt_days(conn, RT_FIRST_DAY, last_day)
        print(f"Real-time backfill {RT_FIRST_DAY} to {last_day}: {len(days)} day(s) to fetch, "
              f"{len(days) * 24} requests")
        done = [0]

        def report(line):
            done[0] += 1
            print(f"[{done[0]}/{len(days)}] {line}", flush=True)

        stored, failed = update_rt_days(conn, days, {"User-Agent": USER_AGENT}, report=report)
        first, last, rows, items, day_count = conn.execute(
            "SELECT MIN(record_date), MAX(record_date), COUNT(*), COUNT(DISTINCT item_id), "
            "COUNT(DISTINCT record_date) FROM rt_daily").fetchone()
    finally:
        conn.close()

    print("=" * 60)
    print("REAL-TIME BACKFILL COMPLETE" if not failed else "REAL-TIME BACKFILL FINISHED WITH FAILURES")
    print(f"Days stored this run: {len(stored)}, failed: {len(failed)}")
    print(f"rt_daily: {rows:,} rows, {items:,} items, {day_count} days ({first} to {last})")
    for day, error in failed:
        print(f"Failed {day}: {error}")
    return failed


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

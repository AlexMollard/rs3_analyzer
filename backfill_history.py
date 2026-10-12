"""
RS3 Market Data Historical Backfill Script
Fetches price history from Weirdgloop API for all items, keeping the last HISTORY_DAYS
"""

import argparse
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timedelta, timezone

from rs3_collector import (DB_PATH, USER_AGENT, FetchError, connect_db,
                           ensure_schema, get_with_retry)

# Configuration
API_URL = "https://api.weirdgloop.org/exchange/history/rs/all?id={}"
RATE_LIMIT_DELAY = 1  # minimum seconds between requests, shared across all workers
MAX_WORKERS = 3  # parallel workers (requests are still spaced by RATE_LIMIT_DELAY)
BATCH_SIZE = 100  # Commit to DB every N items
# Matches the app's longest window (365-day price chart; flips use the last 90).
# The API has no 365-day endpoint, so the full history is fetched and older rows dropped.
HISTORY_DAYS = 365

# Thread-safe database lock
db_lock = threading.Lock()


class RateLimiter:
    """Thread-safe limiter: successive wait() calls return at least `delay` seconds apart."""

    def __init__(self, delay, clock=time.monotonic, sleep=time.sleep):
        self.delay = delay
        self._clock = clock
        self._sleep = sleep
        self._lock = threading.Lock()
        self._next = 0.0

    def wait(self):
        with self._lock:
            now = self._clock()
            slot = max(now, self._next)
            self._next = slot + self.delay
        if slot > now:
            self._sleep(slot - now)


limiter = RateLimiter(RATE_LIMIT_DELAY)


def get_all_item_ids():
    """Get all item IDs from the database"""
    conn = connect_db(DB_PATH)
    try:
        ensure_schema(conn)
        cursor = conn.cursor()
        cursor.execute("SELECT id FROM items ORDER BY id")
        return [row[0] for row in cursor.fetchall()]
    finally:
        conn.close()


def fetch_item_history(item_id):
    """Fetch complete price history for a single item.

    Returns a list of records ([] means the API has no data for the item).
    Raises FetchError if the request failed after retries.
    """
    response = get_with_retry(
        API_URL.format(item_id),
        headers={"User-Agent": USER_AGENT},
        before_request=limiter.wait,
    )
    if response.status_code == 404:
        return []  # unknown item: genuinely no data
    if response.status_code != 200:
        raise FetchError(f"item {item_id}: HTTP {response.status_code}")
    try:
        data = response.json()
    except ValueError as e:
        raise FetchError(f"item {item_id}: invalid JSON ({e})")

    # Extract history from the response
    records = data.get(str(item_id)) if isinstance(data, dict) else None
    return records if isinstance(records, list) else []


def prepare_records(item_id, history_records, history_days=HISTORY_DAYS):
    """Validate raw API records, dropping any older than history_days.

    Returns (rows, skipped_count); dropped old records are not counted as skipped.
    """
    cutoff = (datetime.now(timezone.utc) - timedelta(days=history_days)).strftime('%Y-%m-%d')
    rows = []
    skipped = 0
    for record in history_records:
        try:
            price = record['price']
            if price is None or isinstance(price, bool) or not isinstance(price, (int, float)):
                raise ValueError("non-numeric price")
            timestamp_sec = record['timestamp'] / 1000
            date_str = datetime.fromtimestamp(timestamp_sec, tz=timezone.utc).strftime('%Y-%m-%d')
            if date_str < cutoff:
                continue
            volume = record.get('volume')
            if isinstance(volume, bool) or not isinstance(volume, (int, float)):
                volume = None  # unknown volume -> NULL
            else:
                volume = int(volume)
            rows.append((item_id, date_str, int(price), volume))
        except (KeyError, TypeError, ValueError, OverflowError, OSError, AttributeError):
            skipped += 1
    return rows, skipped


def insert_history_batch(item_id, history_records):
    """Insert a batch of history records for an item (thread-safe).

    Returns (inserted, skipped).
    """
    records_to_insert, skipped = prepare_records(item_id, history_records)

    # Thread-safe database insert
    with db_lock:
        conn = connect_db(DB_PATH)
        try:
            before = conn.total_changes
            # Use executemany for much faster bulk insert
            conn.executemany("""
                INSERT OR IGNORE INTO history (item_id, record_date, price, volume)
                VALUES (?, ?, ?, ?)
            """, records_to_insert)
            conn.commit()
            # cursor.rowcount is unreliable after executemany; total_changes is not
            inserted = conn.total_changes - before
        finally:
            conn.close()

    return inserted, skipped


def process_single_item(item_id):
    """Process a single item (fetch and insert) - designed for parallel execution.

    Returns (item_id, status, record_count, inserted, skipped, error) where status is
    'ok', 'nodata' or 'failed'.
    """
    try:
        history = fetch_item_history(item_id)
    except Exception as e:
        return (item_id, "failed", 0, 0, 0, str(e))
    if not history:
        return (item_id, "nodata", 0, 0, 0, None)
    try:
        inserted, skipped = insert_history_batch(item_id, history)
    except Exception as e:
        return (item_id, "failed", 0, 0, 0, f"database error: {e}")
    return (item_id, "ok", len(history), inserted, skipped, None)


def run_pass(item_ids, label, stats, start_time):
    """Run one parallel pass over item_ids, updating stats. Returns the ids that failed."""
    failed = []
    total = len(item_ids)
    processed = 0
    with ThreadPoolExecutor(max_workers=MAX_WORKERS) as executor:
        futures = [executor.submit(process_single_item, item_id) for item_id in item_ids]

        for future in as_completed(futures):
            processed += 1
            item_id, status, record_count, inserted, skipped, error = future.result()

            if status == "failed":
                failed.append(item_id)
                print(f"[{label} {processed}/{total}] Item {item_id}: FAILED - {error}")
            elif status == "nodata":
                stats["nodata"] += 1
                print(f"[{label} {processed}/{total}] Item {item_id}: no data")
            else:
                stats["records"] += record_count
                stats["inserted"] += inserted
                stats["skipped"] += skipped
                extra = f", {skipped} skipped" if skipped else ""
                print(f"[{label} {processed}/{total}] Item {item_id}: OK {record_count} records ({inserted} new{extra})")

            # Progress update every 100 items
            if processed % 100 == 0:
                elapsed = time.time() - start_time
                items_per_sec = processed / elapsed if elapsed > 0 else 0
                eta_seconds = (total - processed) / items_per_sec if items_per_sec > 0 else 0
                eta_minutes = eta_seconds / 60

                print("-" * 60)
                print(f"Progress: {processed}/{total} ({100*processed/total:.1f}%)")
                print(f"Speed: {items_per_sec:.1f} items/sec")
                print(f"ETA: {eta_minutes:.1f} minutes")
                print(f"New records inserted: {stats['inserted']:,}")
                print("-" * 60)
    return failed


def backfill_historical_data():
    """Main backfill function with parallel processing. Returns the ids that still failed."""
    print("Starting FAST historical data backfill...")
    print("=" * 60)

    # Get all items
    item_ids = get_all_item_ids()
    total_items = len(item_ids)
    print(f"Found {total_items} items in database")
    print(f"Using {MAX_WORKERS} parallel workers, at most 1 request per {RATE_LIMIT_DELAY}s overall")
    print("=" * 60)

    stats = {"records": 0, "inserted": 0, "skipped": 0, "nodata": 0}
    start_time = time.time()

    failed = run_pass(item_ids, "pass 1", stats, start_time)
    if failed:
        print(f"\n{len(failed)} items failed; retrying them once more...")
        failed = run_pass(failed, "retry", stats, start_time)

    elapsed_time = max(time.time() - start_time, 1e-9)

    # Final summary
    print("=" * 60)
    print("BACKFILL COMPLETE!" if not failed else "BACKFILL FINISHED WITH FAILURES")
    print("=" * 60)
    print(f"Total time: {elapsed_time/60:.1f} minutes ({elapsed_time:.0f} seconds)")
    print(f"Total items: {total_items}")
    print(f"Items with no data: {stats['nodata']}")
    print(f"Items failed: {len(failed)}")
    print(f"Total records fetched: {stats['records']:,}")
    print(f"New history records inserted: {stats['inserted']:,}")
    print(f"Malformed records skipped: {stats['skipped']:,}")
    print("=" * 60)

    # Show updated data range
    conn = connect_db(DB_PATH)
    try:
        cursor = conn.cursor()
        cursor.execute("""
            SELECT
                MIN(record_date) as earliest,
                MAX(record_date) as latest,
                COUNT(*) as total_records,
                COUNT(DISTINCT item_id) as unique_items
            FROM history
        """)
        stats_row = cursor.fetchone()
    finally:
        conn.close()

    print("\nDatabase Statistics After Backfill:")
    print(f"Date range: {stats_row[0]} to {stats_row[1]}")
    print(f"Total records: {stats_row[2]:,}")
    print(f"Unique items: {stats_row[3]:,}")
    if stats_row[3]:
        print(f"Average records per item: {stats_row[2]/stats_row[3]:.1f}")
    print("=" * 60)

    if failed:
        print(f"Failed item ids ({len(failed)}): {', '.join(str(i) for i in sorted(failed))}")
    return failed


def prune_history(history_days=HISTORY_DAYS, db_path=DB_PATH):
    """Delete history older than history_days and compact the file. Returns rows deleted."""
    cutoff = (datetime.now(timezone.utc) - timedelta(days=history_days)).strftime('%Y-%m-%d')
    size_before = db_path.stat().st_size
    conn = connect_db(db_path)
    try:
        deleted = conn.execute("DELETE FROM history WHERE record_date < ?", (cutoff,)).rowcount
        conn.commit()
        conn.execute("VACUUM")  # needs free disk roughly the size of the database
        conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    finally:
        conn.close()
    size_after = db_path.stat().st_size
    print(f"Deleted {deleted:,} rows older than {cutoff}")
    print(f"Database size: {size_before / 1e6:,.1f} MB -> {size_after / 1e6:,.1f} MB")
    return deleted


if __name__ == "__main__":
    # Avoid UnicodeEncodeError on cp1252 consoles
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prune", action="store_true",
                        help=f"delete history older than {HISTORY_DAYS} days and compact the database, then exit")
    args = parser.parse_args()
    if args.prune:
        if not DB_PATH.exists():
            print(f"{DB_PATH} not found")
            sys.exit(1)
        prune_history()
        sys.exit(0)
    try:
        remaining = backfill_historical_data()
    except KeyboardInterrupt:
        print("\n\nBackfill interrupted by user")
        sys.exit(130)
    except Exception as e:
        print(f"\n\nFatal error: {e}")
        sys.exit(1)
    sys.exit(1 if remaining else 0)

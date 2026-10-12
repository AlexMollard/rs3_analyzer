import sqlite3
import sys
import time
from datetime import date, datetime, timedelta, timezone
from pathlib import Path

import requests

BASE_DIR = Path(__file__).resolve().parent
DB_PATH = BASE_DIR / "rs3_market.db"
LOG_PATH = BASE_DIR / "collector_log.txt"
# Weird Gloop asks for a descriptive User-Agent with a way to contact the operator
USER_AGENT = ("rs3-analyzer-collector/1.0 (RS3 Grand Exchange price history tool; "
              "+https://github.com/AlexMollard/rs3_analyzer)")
DUMP_URL = "https://chisel.weirdgloop.org/gazproj/gazbot/rs_dump.json"
LATEST_URL = "https://api.weirdgloop.org/exchange/history/rs/latest?id={}"
LATEST_BATCH = 100        # max ids per latest request
LATEST_BATCH_DELAY = 1.0  # seconds between latest requests
# RuneScape Wiki real-time prices: bulk hourly averages of instant-buy ("high") and
# instant-sell ("low") trades for every item. Same item ids as the GE.
RT_HOUR_URL = "https://prices.runescape.wiki/api/v2/rs/1h?timestamp={}"
RT_FIRST_DAY = "2026-07-24"  # first complete UTC day (data starts 2026-07-23 17:00 UTC)
RT_REQUEST_DELAY = 1.0       # seconds before each real-time request
RT_CATCHUP_DAYS = 7          # the daily job fills at most this many recent days
RT_SETTLE = 3600             # seconds after midnight UTC before yesterday counts as complete

MAX_RETRIES = 5
BACKOFF_BASE = 2.0   # seconds; doubles each attempt
BACKOFF_MAX = 120.0  # also caps Retry-After


class FetchError(Exception):
    """Raised when a request still fails after all retries."""


def connect_db(path=DB_PATH):
    """Opens the database with WAL and a busy timeout so writers do not collide."""
    conn = sqlite3.connect(str(path), timeout=30)
    conn.execute("PRAGMA busy_timeout = 30000")
    conn.execute("PRAGMA journal_mode = WAL")
    return conn


def ensure_schema(conn):
    """Creates the tables and indexes if they don't exist (shared with backfill_history.py)."""
    # Table for Item Definitions (ID, Name, Buy Limit - NULL when unknown)
    conn.execute('''CREATE TABLE IF NOT EXISTS items (
                    id INTEGER PRIMARY KEY,
                    name TEXT,
                    ge_limit INTEGER)''')

    # Table for Price History (Link to Item, Date, Price, Volume - NULL when unknown)
    # Using a composite primary key to prevent duplicate entries for the same day
    conn.execute('''CREATE TABLE IF NOT EXISTS history (
                    item_id INTEGER,
                    record_date TEXT,
                    price INTEGER,
                    volume INTEGER,
                    PRIMARY KEY (item_id, record_date))''')

    # Real-time daily averages folded from the wiki's hourly buckets: volume-weighted
    # instant-buy (high) and instant-sell (low) prices, NULL when that side did not trade
    conn.execute('''CREATE TABLE IF NOT EXISTS rt_daily (
                    item_id INTEGER,
                    record_date TEXT,
                    avg_high REAL,
                    avg_low REAL,
                    high_volume INTEGER,
                    low_volume INTEGER,
                    PRIMARY KEY (item_id, record_date))''')

    conn.execute("CREATE INDEX IF NOT EXISTS idx_history_record_date ON history(record_date)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_items_name ON items(name)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_rt_daily_record_date ON rt_daily(record_date)")
    conn.commit()


def _retry_after_seconds(response):
    value = response.headers.get("Retry-After")
    if not value:
        return None
    try:
        return max(0.0, float(value))
    except ValueError:
        return None  # HTTP-date form is not worth parsing; fall back to backoff


def get_with_retry(url, headers, timeout=30, retries=MAX_RETRIES,
                   before_request=None, sleep=time.sleep, http_get=requests.get):
    """GET with exponential backoff on timeouts, connection errors, 429 and 5xx.

    Returns the response for any non-retryable status (including 404; the caller decides).
    Raises FetchError once retries are exhausted. before_request (if given) is called
    before every attempt, which is where a rate limiter hooks in.
    """
    last = "no attempts made"
    for attempt in range(retries + 1):
        delay = min(BACKOFF_BASE * (2 ** attempt), BACKOFF_MAX)
        try:
            if before_request:
                before_request()
            res = http_get(url, headers=headers, timeout=timeout)
        except (requests.Timeout, requests.ConnectionError) as e:
            last = f"{type(e).__name__}: {e}"
        else:
            if res.status_code != 429 and res.status_code < 500:
                return res
            last = f"HTTP {res.status_code}"
            retry_after = _retry_after_seconds(res)
            if retry_after is not None:
                delay = min(retry_after, BACKOFF_MAX)
        if attempt < retries:
            sleep(delay)
    raise FetchError(f"{url}: {last} after {retries + 1} attempts")


def _rt_side(item, price_key, volume_key):
    """(price, volume) for one side of an hourly entry, (None, 0) if that side did not trade.

    Raises ValueError for a malformed side. Prices may be fractional and exceed 32 bits.
    """
    price, volume = item.get(price_key), item.get(volume_key)
    if price is None or not volume:
        return None, 0
    if isinstance(price, bool) or not isinstance(price, (int, float)) or not 0 < price < float("inf"):
        raise ValueError("bad price")
    if isinstance(volume, bool) or not isinstance(volume, int) or volume < 0:
        raise ValueError("bad volume")
    return price, volume


def aggregate_rt_hours(buckets):
    """Folds a day's hourly real-time buckets into one row per item.

    Each side's price is volume-weighted over the hours it traded; a side that never traded
    gets a NULL price and 0 volume. Items that did not trade at all are left out and malformed
    entries are skipped. Returns ({item_id: (avg_high, avg_low, high_volume, low_volume)}, skipped).
    """
    totals = {}  # item_id -> [high gp*qty, high qty, low gp*qty, low qty]
    skipped = 0
    for data in buckets:
        for key, item in data.items():
            try:
                item_id = int(key)
                sides = (_rt_side(item, "avgHighPrice", "highPriceVolume"),
                         _rt_side(item, "avgLowPrice", "lowPriceVolume"))
            except (TypeError, ValueError, AttributeError):
                skipped += 1
                continue
            t = totals.setdefault(item_id, [0.0, 0, 0.0, 0])
            for i, (price, volume) in enumerate(sides):
                if volume:
                    t[2 * i] += price * volume
                    t[2 * i + 1] += volume
    rows = {}
    for item_id, (high_gp, high_qty, low_gp, low_qty) in totals.items():
        if high_qty or low_qty:
            rows[item_id] = (high_gp / high_qty if high_qty else None,
                             low_gp / low_qty if low_qty else None,
                             high_qty, low_qty)
    return rows, skipped


def fetch_rt_day(day, headers, sleep=time.sleep, http_get=requests.get):
    """Fetches the 24 hourly real-time buckets of a UTC day ('YYYY-MM-DD'), one bulk request each.

    Raises FetchError if any bucket fails, is empty or is for a different hour, so a partial
    day is never stored.
    """
    start = int(datetime.strptime(day, "%Y-%m-%d").replace(tzinfo=timezone.utc).timestamp())
    buckets = []
    for hour in range(24):
        ts = start + hour * 3600
        where = f"real-time {day} {hour:02d}:00 UTC"
        res = get_with_retry(RT_HOUR_URL.format(ts), headers, sleep=sleep, http_get=http_get,
                             before_request=lambda: sleep(RT_REQUEST_DELAY))
        if res.status_code != 200:
            raise FetchError(f"{where}: HTTP {res.status_code}")
        try:
            body = res.json()
        except ValueError as e:
            raise FetchError(f"{where}: invalid JSON ({e})")
        data = body.get("data") if isinstance(body, dict) else None
        if not isinstance(data, dict) or not data or body.get("timestamp") != ts:
            raise FetchError(f"{where}: empty bucket or wrong hour")
        buckets.append(data)
    return buckets


def last_complete_rt_day(now=None):
    """The latest UTC day whose hourly buckets are all final ('YYYY-MM-DD')."""
    now = now or datetime.now(timezone.utc)
    return ((now - timedelta(seconds=RT_SETTLE)).date() - timedelta(days=1)).isoformat()


def missing_rt_days(conn, first_day, last_day):
    """Days in [first_day, last_day] with no rt_daily rows (days are written atomically)."""
    have = {row[0] for row in conn.execute(
        "SELECT DISTINCT record_date FROM rt_daily WHERE record_date BETWEEN ? AND ?", (first_day, last_day))}
    days = []
    day, last = date.fromisoformat(first_day), date.fromisoformat(last_day)
    while day <= last:
        if day.isoformat() not in have:
            days.append(day.isoformat())
        day += timedelta(days=1)
    return days


def store_rt_day(conn, day, rows):
    """Replaces a day's rt_daily rows in one transaction."""
    with conn:
        conn.execute("DELETE FROM rt_daily WHERE record_date = ?", (day,))
        conn.executemany("INSERT INTO rt_daily (item_id, record_date, avg_high, avg_low, high_volume, low_volume) "
                         "VALUES (?,?,?,?,?,?)", [(item_id, day, *row) for item_id, row in sorted(rows.items())])


def update_rt_days(conn, days, headers, report=None, stop_on_failure=False,
                   sleep=time.sleep, http_get=requests.get):
    """Fetches and stores each day; a failed day is not stored, so the next run retries it.

    report (if given) receives one progress line per day. stop_on_failure skips the remaining
    days after a failure (the API is probably down). Returns (stored, failed): lists of
    (day, item_count) and (day, error).
    """
    stored, failed = [], []
    for day in days:
        if failed and stop_on_failure:
            break
        try:
            rows, skipped = aggregate_rt_hours(fetch_rt_day(day, headers, sleep=sleep, http_get=http_get))
            if not rows:
                raise FetchError(f"real-time {day}: no valid items")
            store_rt_day(conn, day, rows)
        except (FetchError, requests.RequestException, ValueError, sqlite3.Error) as e:
            failed.append((day, str(e)))
            line = f"{day}: FAILED - {e}"
        else:
            stored.append((day, len(rows)))
            line = f"{day}: {len(rows):,} items" + (f" ({skipped} malformed skipped)" if skipped else "")
        if report:
            report(line)
    return stored, failed


class RS3DataCollector:
    def __init__(self):
        self.db_name = DB_PATH
        self.log_file = LOG_PATH
        self.url = DUMP_URL
        self.headers = {'User-Agent': USER_AGENT}

    def init_db(self):
        """Opens the database and creates the tables if they don't exist."""
        conn = connect_db(self.db_name)
        try:
            ensure_schema(conn)
        except Exception:
            conn.close()
            raise
        return conn

    def log(self, message):
        timestamp = time.strftime("%Y-%m-%d %H:%M:%S")
        with open(self.log_file, "a", encoding="utf-8") as f:
            f.write(f"[{timestamp}] {message}\n")

    def fetch_from_dump(self):
        """Reads the full daily dump. Returns (items_to_update, history_to_insert, skipped)."""
        res = get_with_retry(self.url, self.headers)
        res.raise_for_status()
        data = res.json()

        # Validate per item so one bad row cannot abort the batch
        items_to_update = []
        history_to_insert = []
        skipped = 0
        # Date rows by Jagex's price update, as the history API does; stamping the run date
        # copies yesterday's prices onto today when the collector runs before the update
        jagex_ts = data.get('%JAGEX_TIMESTAMP%')
        if isinstance(jagex_ts, (int, float)) and not isinstance(jagex_ts, bool):
            today_str = datetime.fromtimestamp(jagex_ts, tz=timezone.utc).date().isoformat()
        else:
            today_str = datetime.now(timezone.utc).date().isoformat()

        for item_id, item in data.items():
            try:
                if not isinstance(item, dict) or 'price' not in item:
                    continue
                row_id = int(item_id)
                name = item['name']
                price = item['price']
                if not isinstance(name, str) or isinstance(price, bool):
                    raise ValueError("bad name or price")
                price = int(price)
                limit = item.get('limit')  # missing -> NULL (unknown), not a guessed value
                limit = None if limit is None else int(limit)
                volume = item.get('volume')
                volume = None if volume is None else int(volume)
            except (ValueError, TypeError, KeyError, OverflowError):
                skipped += 1
                continue
            items_to_update.append((row_id, name, limit))
            history_to_insert.append((row_id, today_str, price, volume))
        return items_to_update, history_to_insert, skipped

    def fetch_from_latest(self, conn, sleep=time.sleep):
        """Fallback when the dump is down: batched latest prices for items already known.

        Can't discover new items or refresh names/limits, so items is left untouched.
        Dates come from the API's own timestamp. Returns (history_to_insert, skipped).
        """
        ids = [row[0] for row in conn.execute("SELECT id FROM items ORDER BY id")]
        if not ids:
            raise FetchError("latest fallback needs existing items; run backfill_history.py first")

        history_to_insert = []
        skipped = 0
        for start in range(0, len(ids), LATEST_BATCH):
            if start:
                sleep(LATEST_BATCH_DELAY)
            batch = ids[start:start + LATEST_BATCH]
            res = get_with_retry(LATEST_URL.format("|".join(map(str, batch))), self.headers)
            res.raise_for_status()
            for item_id, item in res.json().items():
                try:
                    price = item['price']
                    if isinstance(price, bool):
                        raise ValueError("bad price")
                    record_date = datetime.strptime(item['timestamp'][:10], "%Y-%m-%d").date().isoformat()
                    volume = item.get('volume')
                    volume = None if volume is None else int(volume)
                    history_to_insert.append((int(item_id), record_date, int(price), volume))
                except (ValueError, TypeError, KeyError, OverflowError):
                    skipped += 1
        return history_to_insert, skipped

    def update_realtime(self, conn):
        """Stores yesterday's real-time averages, catching up at most RT_CATCHUP_DAYS missing days.

        Stops at the first failed day. Returns (stored, failed) like update_rt_days.
        """
        last = last_complete_rt_day()
        first = max(RT_FIRST_DAY, (date.fromisoformat(last) - timedelta(days=RT_CATCHUP_DAYS - 1)).isoformat())
        return update_rt_days(conn, missing_rt_days(conn, first, last), self.headers, stop_on_failure=True)

    def run_daily_job(self):
        """Returns True on success, False on failure (also logged)."""
        conn = None
        try:
            conn = self.init_db()
            c = conn.cursor()

            source = "dump"
            try:
                items_to_update, history_to_insert, skipped = self.fetch_from_dump()
            except (FetchError, requests.RequestException, ValueError) as e:
                self.log(f"WARNING: dump unavailable ({e}); falling back to latest API")
                source = "latest"
                items_to_update = []
                history_to_insert, skipped = self.fetch_from_latest(conn)

            c.executemany("INSERT OR REPLACE INTO items (id, name, ge_limit) VALUES (?,?,?)",
                          items_to_update)
            c.executemany("INSERT OR IGNORE INTO history (item_id, record_date, price, volume) "
                          "VALUES (?,?,?,?)", history_to_insert)
            conn.commit()

            # --- SUCCESS LOGGING ---
            self.log(f"SUCCESS ({source}): Updated {len(history_to_insert)} items, "
                     f"skipped {skipped} malformed.")

            # Real-time data is optional: problems are logged but never fail the daily job
            try:
                stored, failed = self.update_realtime(conn)
                if stored:
                    self.log("SUCCESS (real-time): " + ", ".join(f"{day} {n} items" for day, n in stored))
                for day, error in failed:
                    self.log(f"WARNING: real-time {day} not stored ({error})")
            except Exception as e:
                try:
                    self.log(f"WARNING: real-time update failed ({e})")
                except OSError:
                    pass
            print("Success.")
            return True

        except Exception as e:
            # --- ERROR LOGGING ---
            try:
                self.log(f"ERROR: {str(e)}")
            except OSError:
                pass
            print(f"Error: {e}", file=sys.stderr)
            return False
        finally:
            if conn is not None:
                conn.close()


if __name__ == "__main__":
    bot = RS3DataCollector()
    ok = bot.run_daily_job()
    # Keep window open briefly if run manually
    if sys.stdin is not None and sys.stdin.isatty():
        time.sleep(3)
    sys.exit(0 if ok else 1)

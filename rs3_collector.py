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
RT_FIRST_HOUR = 1784826000   # 2026-07-23 17:00 UTC, the first hourly bucket with data
RT_FIRST_DAY = "2026-07-24"  # first complete UTC day
RT_REQUEST_DELAY = 1.0       # seconds before each real-time request
RT_CATCHUP_DAYS = 7          # the daily job fills at most this many recent days (and their hours)
RT_SETTLE = 3600             # seconds a bucket must have been closed before it counts as final

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

    # Real-time hourly buckets as served by the wiki (ts = hour start, unix seconds UTC); rt_daily
    # is folded from them. WITHOUT ROWID clusters each item's hours and stores about 40% less.
    conn.execute('''CREATE TABLE IF NOT EXISTS rt_hourly (
                    item_id INTEGER,
                    ts INTEGER,
                    avg_high REAL,
                    avg_low REAL,
                    high_volume INTEGER,
                    low_volume INTEGER,
                    PRIMARY KEY (item_id, ts)) WITHOUT ROWID''')

    conn.execute("CREATE INDEX IF NOT EXISTS idx_history_record_date ON history(record_date)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_items_name ON items(name)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_rt_daily_record_date ON rt_daily(record_date)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_rt_hourly_ts ON rt_hourly(ts)")
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
    if isinstance(volume, bool) or not isinstance(volume, int) or not 0 <= volume < 2 ** 63:
        raise ValueError("bad volume")
    return float(price), volume


def rt_hour_rows(data):
    """Parses one hourly real-time bucket into one row per item.

    A side that did not trade gets a NULL price and 0 volume. Items that did not trade at all are
    left out and malformed entries are skipped.
    Returns ({item_id: (avg_high, avg_low, high_volume, low_volume)}, skipped).
    """
    rows, skipped = {}, 0
    for key, item in data.items():
        try:
            item_id = int(key)
            (high, high_qty), (low, low_qty) = (_rt_side(item, "avgHighPrice", "highPriceVolume"),
                                                _rt_side(item, "avgLowPrice", "lowPriceVolume"))
        except (TypeError, ValueError, AttributeError, OverflowError):
            skipped += 1
            continue
        if high_qty or low_qty:
            rows[item_id] = (high, low, high_qty, low_qty)
    return rows, skipped


def aggregate_rt_hours(hours):
    """Folds a day's hourly rows (one {item_id: (avg_high, avg_low, high_volume, low_volume)} per
    hour, in hour order) into one row per item.

    Each side's price is volume-weighted over the hours it traded; a side that never traded
    gets a NULL price and 0 volume. Returns {item_id: (avg_high, avg_low, high_volume, low_volume)}.
    """
    totals = {}  # item_id -> [high gp*qty, high qty, low gp*qty, low qty]
    for rows in hours:
        for item_id, (high, low, high_qty, low_qty) in rows.items():
            t = totals.setdefault(item_id, [0.0, 0, 0.0, 0])
            if high_qty:
                t[0] += high * high_qty
                t[1] += high_qty
            if low_qty:
                t[2] += low * low_qty
                t[3] += low_qty
    return {item_id: (high_gp / high_qty if high_qty else None,
                      low_gp / low_qty if low_qty else None,
                      high_qty, low_qty)
            for item_id, (high_gp, high_qty, low_gp, low_qty) in totals.items() if high_qty or low_qty}


def hour_label(ts):
    """'YYYY-MM-DD HH:00 UTC' for an hour start in unix seconds."""
    return datetime.fromtimestamp(ts, timezone.utc).strftime("%Y-%m-%d %H:00 UTC")


def day_start(day):
    """Unix seconds of a UTC day's midnight ('YYYY-MM-DD')."""
    return int(datetime.strptime(day, "%Y-%m-%d").replace(tzinfo=timezone.utc).timestamp())


def fetch_rt_hour(ts, headers, sleep=time.sleep, http_get=requests.get):
    """Fetches the hourly real-time bucket starting at ts (unix seconds) in one bulk request.

    Raises FetchError if the request fails or the bucket is empty or for a different hour, so a
    missing or unfinished hour is never stored.
    """
    where = f"real-time {hour_label(ts)}"
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
    return data


def last_complete_rt_day(now=None):
    """The latest UTC day whose hourly buckets are all final ('YYYY-MM-DD')."""
    now = now or datetime.now(timezone.utc)
    return ((now - timedelta(seconds=RT_SETTLE)).date() - timedelta(days=1)).isoformat()


def last_complete_rt_hour(now=None):
    """Start (unix seconds) of the latest hourly bucket that closed at least RT_SETTLE seconds ago.

    Matches last_complete_rt_day: a day's last hour becomes final when the day does.
    """
    now = now or datetime.now(timezone.utc)
    return (int(now.timestamp()) - RT_SETTLE) // 3600 * 3600 - 3600


def missing_rt_hours(conn, first_ts, last_ts):
    """Hour starts in [first_ts, last_ts] with no rt_hourly rows (hours are written atomically)."""
    have = {row[0] for row in conn.execute(
        "SELECT DISTINCT ts FROM rt_hourly WHERE ts BETWEEN ? AND ?", (first_ts, last_ts))}
    return [ts for ts in range(first_ts, last_ts + 1, 3600) if ts not in have]


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


def store_rt_hour(conn, ts, rows):
    """Replaces an hour's rt_hourly rows in one transaction."""
    with conn:
        conn.execute("DELETE FROM rt_hourly WHERE ts = ?", (ts,))
        conn.executemany("INSERT INTO rt_hourly (item_id, ts, avg_high, avg_low, high_volume, low_volume) "
                         "VALUES (?,?,?,?,?,?)", [(item_id, ts, *row) for item_id, row in sorted(rows.items())])


def update_rt_hours(conn, hours, headers, report=None, stop_on_failure=False,
                    sleep=time.sleep, http_get=requests.get):
    """Fetches and stores each hour (unix seconds); a failed hour is not stored, so the next run
    retries it.

    report (if given) receives one progress line per hour. stop_on_failure skips the remaining
    hours after a failure (the API is probably down). Returns (stored, failed): lists of
    (ts, item_count) and (ts, error).
    """
    stored, failed = [], []
    for ts in hours:
        if failed and stop_on_failure:
            break
        try:
            rows, skipped = rt_hour_rows(fetch_rt_hour(ts, headers, sleep=sleep, http_get=http_get))
            if not rows:
                raise FetchError(f"real-time {hour_label(ts)}: no valid items")
            store_rt_hour(conn, ts, rows)
        except (FetchError, requests.RequestException, ValueError, sqlite3.Error) as e:
            failed.append((ts, str(e)))
            line = f"{hour_label(ts)}: FAILED - {e}"
        else:
            stored.append((ts, len(rows)))
            line = f"{hour_label(ts)}: {len(rows):,} items" + (f" ({skipped} malformed skipped)" if skipped else "")
        if report:
            report(line)
    return stored, failed


def fold_rt_days(conn, days):
    """Writes each day's rt_daily rows from its 24 stored rt_hourly hours.

    A day is written only once all 24 of its hours are stored. Returns (stored, incomplete):
    a list of (day, item_count) and the days left out.
    """
    stored, incomplete = [], []
    for day in days:
        start = day_start(day)
        hours = {}
        for item_id, ts, *row in conn.execute(
                "SELECT item_id, ts, avg_high, avg_low, high_volume, low_volume FROM rt_hourly "
                "WHERE ts BETWEEN ? AND ?", (start, start + 23 * 3600)):
            hours.setdefault(ts, {})[item_id] = tuple(row)
        if len(hours) < 24:
            incomplete.append(day)
            continue
        rows = aggregate_rt_hours(hours[ts] for ts in sorted(hours))
        store_rt_day(conn, day, rows)
        stored.append((day, len(rows)))
    return stored, incomplete


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
        """Stores the missing real-time hours of the last RT_CATCHUP_DAYS days and today in
        rt_hourly, then folds the missing complete days among them into rt_daily.

        Stops fetching at the first failed hour. Returns (stored_hours, failed_hours, stored_days,
        incomplete_days) like update_rt_hours and fold_rt_days.
        """
        last_day = last_complete_rt_day()
        first_day = max(RT_FIRST_DAY,
                        (date.fromisoformat(last_day) - timedelta(days=RT_CATCHUP_DAYS - 1)).isoformat())
        hours = missing_rt_hours(conn, day_start(first_day), last_complete_rt_hour())
        stored_hours, failed_hours = update_rt_hours(conn, hours, self.headers, stop_on_failure=True)
        stored_days, incomplete_days = fold_rt_days(conn, missing_rt_days(conn, first_day, last_day))
        return stored_hours, failed_hours, stored_days, incomplete_days

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
                stored_hours, failed_hours, stored_days, incomplete_days = self.update_realtime(conn)
                if stored_hours:
                    self.log(f"SUCCESS (real-time): {len(stored_hours)} hour(s), "
                             f"{hour_label(stored_hours[0][0])} to {hour_label(stored_hours[-1][0])}")
                for _, error in failed_hours:
                    self.log(f"WARNING: real-time hour not stored ({error})")
                if stored_days:
                    self.log("SUCCESS (real-time): " + ", ".join(f"{day} {n} items" for day, n in stored_days))
                if incomplete_days:
                    self.log("WARNING: real-time days not stored, hours missing: " + ", ".join(incomplete_days))
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

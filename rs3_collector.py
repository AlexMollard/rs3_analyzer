import sqlite3
import sys
import time
from datetime import datetime, timezone
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

    conn.execute("CREATE INDEX IF NOT EXISTS idx_history_record_date ON history(record_date)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_items_name ON items(name)")
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

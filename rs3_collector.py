import sqlite3
import requests
import time
from datetime import date

class RS3DataCollector:
    def __init__(self):
        self.db_name = "rs3_market.db"
        self.url = "https://chisel.weirdgloop.org/gazproj/gazbot/rs_dump.json"
        self.headers = {'User-Agent': 'RS3DataCollector/1.0'}

    def init_db(self):
        """Creates the database tables if they don't exist."""
        conn = sqlite3.connect(self.db_name)
        c = conn.cursor()
        
        # Table for Item Definitions (ID, Name, Buy Limit)
        c.execute('''CREATE TABLE IF NOT EXISTS items (
                     id INTEGER PRIMARY KEY,
                     name TEXT,
                     ge_limit INTEGER)''')

        # Table for Price History (Link to Item, Date, Price, Volume)
        # Using a composite primary key to prevent duplicate entries for the same day
        c.execute('''CREATE TABLE IF NOT EXISTS history (
                     item_id INTEGER,
                     record_date TEXT,
                     price INTEGER,
                     volume INTEGER,
                     PRIMARY KEY (item_id, record_date))''')
        
        conn.commit()
        return conn

    def run_daily_job(self):
        # Define log file path
        log_file = "collector_log.txt"
        
        timestamp = time.strftime("%Y-%m-%d %H:%M:%S")
        
        try:
            conn = self.init_db()
            c = conn.cursor()

            # 1. Download
            res = requests.get(self.url, headers=self.headers, timeout=30)
            res.raise_for_status()
            data = res.json()

            # 2. Process
            items_to_update = []
            history_to_insert = []
            today_str = str(date.today())

            for item_id, item in data.items():
                if not isinstance(item, dict): continue
                if 'price' not in item: continue

                items_to_update.append((int(item_id), item['name'], item.get('limit', 10000)))
                history_to_insert.append((int(item_id), today_str, int(item['price']), item.get('volume', 0)))

            c.executemany("INSERT OR REPLACE INTO items VALUES (?,?,?)", items_to_update)
            c.executemany("INSERT OR IGNORE INTO history VALUES (?,?,?,?)", history_to_insert)
            conn.commit()
            
            # --- SUCCESS LOGGING ---
            with open(log_file, "a") as f:
                f.write(f"[{timestamp}] SUCCESS: Updated {len(history_to_insert)} items.\n")
            print("Success.")

        except Exception as e:
            # --- ERROR LOGGING ---
            with open(log_file, "a") as f:
                f.write(f"[{timestamp}] ERROR: {str(e)}\n")
        finally:
            if 'conn' in locals(): conn.close()

if __name__ == "__main__":
    bot = RS3DataCollector()
    bot.run_daily_job()
    # Keep window open briefly if run manually
    time.sleep(3)
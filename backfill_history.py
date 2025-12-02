"""
RS3 Market Data Historical Backfill Script
Fetches complete price history from Weirdgloop API for all items
"""

import requests
import sqlite3
import time
from datetime import datetime
from concurrent.futures import ThreadPoolExecutor, as_completed
import threading

# Configuration
DB_PATH = "rs3_market.db"
API_URL = "https://api.weirdgloop.org/exchange/history/rs/all?id={}"
RATE_LIMIT_DELAY = 1  # 1s between requests to avoid rate limits
MAX_WORKERS = 3  # Reduced to 3 parallel workers to be API-friendly
BATCH_SIZE = 100  # Commit to DB every N items

# Thread-safe database lock
db_lock = threading.Lock()

def get_all_item_ids():
    """Get all item IDs from the database"""
    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()
    cursor.execute("SELECT id FROM items ORDER BY id")
    item_ids = [row[0] for row in cursor.fetchall()]
    conn.close()
    return item_ids

def fetch_item_history(item_id):
    """Fetch complete price history for a single item"""
    try:
        response = requests.get(
            API_URL.format(item_id),
            headers={"User-Agent": "RS3DataCollector/1.0"},
            timeout=30
        )
        response.raise_for_status()
        data = response.json()
        
        # Extract history from the response
        if str(item_id) in data:
            return data[str(item_id)]
        return []
    except Exception as e:
        print(f"Error fetching item {item_id}: {e}")
        return []

def insert_history_batch(item_id, history_records):
    """Insert a batch of history records for an item (thread-safe)"""
    inserted = 0
    
    # Prepare all records first
    records_to_insert = []
    for record in history_records:
        try:
            timestamp_sec = record['timestamp'] / 1000
            date_str = datetime.fromtimestamp(timestamp_sec).strftime('%Y-%m-%d')
            records_to_insert.append((item_id, date_str, record['price'], record.get('volume')))
        except Exception as e:
            pass  # Skip malformed records silently
    
    # Thread-safe database insert
    with db_lock:
        conn = sqlite3.connect(DB_PATH)
        cursor = conn.cursor()
        
        # Use executemany for much faster bulk insert
        cursor.executemany("""
            INSERT OR IGNORE INTO history (item_id, record_date, price, volume)
            VALUES (?, ?, ?, ?)
        """, records_to_insert)
        
        inserted = cursor.rowcount
        conn.commit()
        conn.close()
    
    return inserted

def process_single_item(item_id):
    """Process a single item (fetch and insert) - designed for parallel execution"""
    try:
        history = fetch_item_history(item_id)
        if history:
            inserted = insert_history_batch(item_id, history)
            return (item_id, len(history), inserted, None)
        else:
            return (item_id, 0, 0, "No data")
    except Exception as e:
        return (item_id, 0, 0, str(e))

def backfill_historical_data():
    """Main backfill function with parallel processing"""
    print("Starting FAST historical data backfill...")
    print("=" * 60)
    
    # Get all items
    item_ids = get_all_item_ids()
    total_items = len(item_ids)
    print(f"Found {total_items} items in database")
    print(f"Using {MAX_WORKERS} parallel workers")
    print("=" * 60)
    
    # Track statistics
    total_inserted = 0
    total_records = 0
    processed = 0
    errors = 0
    start_time = time.time()
    
    # Process items in parallel using ThreadPoolExecutor
    with ThreadPoolExecutor(max_workers=MAX_WORKERS) as executor:
        # Submit all tasks
        future_to_item = {executor.submit(process_single_item, item_id): item_id 
                          for item_id in item_ids}
        
        # Process completed tasks as they finish
        for future in as_completed(future_to_item):
            processed += 1
            item_id, record_count, inserted, error = future.result()
            
            if error:
                errors += 1
                print(f"[{processed}/{total_items}] Item {item_id}: ERROR - {error}")
            else:
                total_records += record_count
                total_inserted += inserted
                print(f"[{processed}/{total_items}] Item {item_id}: ✓ {record_count} records ({inserted} new)")
            
            # Progress update every 100 items
            if processed % 100 == 0:
                elapsed = time.time() - start_time
                items_per_sec = processed / elapsed
                eta_seconds = (total_items - processed) / items_per_sec if items_per_sec > 0 else 0
                eta_minutes = eta_seconds / 60
                
                print("-" * 60)
                print(f"Progress: {processed}/{total_items} ({100*processed/total_items:.1f}%)")
                print(f"Speed: {items_per_sec:.1f} items/sec")
                print(f"ETA: {eta_minutes:.1f} minutes")
                print(f"New records inserted: {total_inserted:,}")
                print("-" * 60)
            
            # Small delay to avoid overwhelming the API
            time.sleep(RATE_LIMIT_DELAY)
    
    elapsed_time = time.time() - start_time
    
    # Final summary
    print("=" * 60)
    print("BACKFILL COMPLETE!")
    print("=" * 60)
    print(f"Total time: {elapsed_time/60:.1f} minutes ({elapsed_time:.0f} seconds)")
    print(f"Speed: {processed/elapsed_time:.1f} items/second")
    print(f"Total items processed: {processed}")
    print(f"Items with errors: {errors}")
    print(f"Total records fetched: {total_records:,}")
    print(f"New history records inserted: {total_inserted:,}")
    print("=" * 60)
    
    # Show updated data range
    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()
    cursor.execute("""
        SELECT 
            MIN(record_date) as earliest,
            MAX(record_date) as latest,
            COUNT(*) as total_records,
            COUNT(DISTINCT item_id) as unique_items
        FROM history
    """)
    stats = cursor.fetchone()
    conn.close()
    
    print("\nDatabase Statistics After Backfill:")
    print(f"Date range: {stats[0]} to {stats[1]}")
    print(f"Total records: {stats[2]:,}")
    print(f"Unique items: {stats[3]:,}")
    print(f"Average records per item: {stats[2]/stats[3]:.1f}")
    print("=" * 60)

if __name__ == "__main__":
    try:
        backfill_historical_data()
    except KeyboardInterrupt:
        print("\n\nBackfill interrupted by user")
    except Exception as e:
        print(f"\n\nFatal error: {e}")

use rusqlite::{Connection, OpenFlags, Result};
use crate::model::{ItemSnapshot, RtDay, RtHour};
use crate::stats::day_number;
use std::collections::HashMap;
use std::path::Path;

// Read-only: never create an empty database when the file is missing
fn open_db(db_path: &Path) -> Result<Connection> {
    Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI,
    )
}

// Keep good rows, count and report bad ones instead of silently dropping them
fn collect_rows<T>(rows: impl Iterator<Item = Result<T>>, what: &str) -> (Vec<T>, usize) {
    let mut out = Vec::new();
    let mut errors = 0;
    for row in rows {
        match row {
            Ok(v) => out.push(v),
            Err(e) => {
                if errors == 0 {
                    eprintln!("{}: skipping unreadable row: {}", what, e);
                }
                errors += 1;
            }
        }
    }
    if errors > 0 {
        eprintln!("{}: {} row(s) skipped due to read errors", what, errors);
    }
    (out, errors)
}

fn snapshot_from_row(row: &rusqlite::Row) -> Result<ItemSnapshot> {
    Ok(ItemSnapshot {
        item_id: row.get(0)?,
        name: row.get(1)?,
        ge_limit: row.get(2)?,
        record_date: row.get(3)?,
        price: row.get(4)?,
        volume: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
    })
}

/// Returns the snapshots and the number of rows skipped due to read errors.
pub fn load_snapshots(db_path: impl AsRef<Path>) -> Result<(Vec<ItemSnapshot>, usize)> {
    let conn = open_db(db_path.as_ref())?;

    let mut stmt = conn.prepare(
        "SELECT i.id, i.name, i.ge_limit, h.record_date,
                h.price, h.volume
         FROM history h
         JOIN items i ON h.item_id = i.id
         WHERE h.record_date >= date('now', '-90 days')
         ORDER BY h.record_date"
    )?;

    let rows = stmt.query_map([], snapshot_from_row)?;

    Ok(collect_rows(rows, "load_snapshots"))
}

/// Every history row, ordered by item then date (the backtest slices its own windows).
pub fn load_all_snapshots(db_path: impl AsRef<Path>) -> Result<(Vec<ItemSnapshot>, usize)> {
    let conn = open_db(db_path.as_ref())?;

    let mut stmt = conn.prepare(
        "SELECT i.id, i.name, i.ge_limit, h.record_date,
                h.price, h.volume
         FROM history h
         JOIN items i ON h.item_id = i.id
         ORDER BY h.item_id, h.record_date"
    )?;

    let rows = stmt.query_map([], snapshot_from_row)?;

    Ok(collect_rows(rows, "load_all_snapshots"))
}

/// One item's real-time day: volume-weighted instant-buy (high) and instant-sell (low) prices.
#[derive(Debug, Clone)]
pub struct RtRow {
    pub item_id: i32,
    pub record_date: String,
    pub avg_high: Option<f64>,  // None when nobody instant-bought that day
    pub avg_low: Option<f64>,   // None when nobody instant-sold that day
    pub high_volume: i64,
    pub low_volume: i64,
}

impl RtRow {
    /// None when the date is unreadable.
    pub fn rt_day(&self) -> Option<RtDay> {
        Some(RtDay {
            day: day_number(&self.record_date)?,
            high: self.avg_high,
            low: self.avg_low,
            high_volume: self.high_volume as f64,
            low_volume: self.low_volume as f64,
        })
    }
}

/// Real-time days from the last `days` days (all of them when None), ordered by item then date.
/// Empty for databases created before the rt_daily table existed.
pub fn load_rt(db_path: impl AsRef<Path>, days: Option<i64>) -> Result<(Vec<RtRow>, usize)> {
    let conn = open_db(db_path.as_ref())?;
    let has_table: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'rt_daily')",
        [],
        |row| row.get(0),
    )?;
    if !has_table {
        return Ok((Vec::new(), 0));
    }

    let mut stmt = conn.prepare(
        "SELECT item_id, record_date, avg_high, avg_low, high_volume, low_volume
         FROM rt_daily
         WHERE ?1 IS NULL OR record_date >= date('now', printf('-%d days', ?1))
         ORDER BY item_id, record_date"
    )?;

    let rows = stmt.query_map([days], |row| {
        Ok(RtRow {
            item_id: row.get(0)?,
            record_date: row.get(1)?,
            avg_high: row.get(2)?,
            avg_low: row.get(3)?,
            high_volume: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
            low_volume: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
        })
    })?;

    Ok(collect_rows(rows, "load_rt"))
}

/// One item's hourly buckets in chronological order.
pub type ItemHours = (i32, Vec<RtHour>);

fn has_rt_hourly(conn: &Connection) -> Result<bool> {
    conn.query_row("SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'rt_hourly')", [], |row| {
        row.get(0)
    })
}

/// Real-time hourly buckets grouped by item. With `since` (unix seconds), only buckets from then
/// on, and only for items in the items table. Empty for databases created before the rt_hourly
/// table existed.
pub fn load_rt_hourly(db_path: impl AsRef<Path>, since: Option<i64>) -> Result<(Vec<ItemHours>, usize)> {
    let conn = open_db(db_path.as_ref())?;
    if !has_rt_hourly(&conn)? {
        return Ok((Vec::new(), 0));
    }

    let mut stmt = match since {
        // CROSS JOIN keeps items as the outer loop, so each item is one primary-key range seek and
        // the app's recent window stays fast as the table grows (`WHERE ts >= ?` scans all of it)
        Some(_) => conn.prepare(
            "SELECT r.item_id, r.ts, r.avg_high, r.avg_low, r.high_volume, r.low_volume
             FROM items i CROSS JOIN rt_hourly r ON r.item_id = i.id AND r.ts >= ?1
             ORDER BY i.id, r.ts",
        )?,
        // The full load streams the table in key order instead of sorting it
        None => conn.prepare("SELECT item_id, ts, avg_high, avg_low, high_volume, low_volume FROM rt_hourly ORDER BY item_id, ts")?,
    };
    let read = |row: &rusqlite::Row| -> Result<(i32, RtHour)> {
        Ok((
            row.get(0)?,
            RtHour {
                hour: row.get::<_, i64>(1)?.div_euclid(3600),
                high: row.get(2)?,
                low: row.get(3)?,
                high_volume: row.get::<_, Option<i64>>(4)?.unwrap_or(0) as f64,
                low_volume: row.get::<_, Option<i64>>(5)?.unwrap_or(0) as f64,
            },
        ))
    };
    let rows = match since {
        Some(ts) => stmt.query_map([ts], read)?,
        None => stmt.query_map([], read)?,
    };

    // Grouped while streaming: the full table is millions of rows
    let mut out: Vec<ItemHours> = Vec::new();
    let (_, errors) = collect_rows(
        rows.map(|row| {
            row.map(|(item_id, hour)| match out.last_mut() {
                Some((id, hours)) if *id == item_id => hours.push(hour),
                _ => out.push((item_id, vec![hour])),
            })
        }),
        "load_rt_hourly",
    );
    Ok((out, errors))
}

/// The hour index (unix hours) of the newest stored bucket; None without hourly data.
pub fn newest_rt_hour(db_path: impl AsRef<Path>) -> Result<Option<i64>> {
    let conn = open_db(db_path.as_ref())?;
    if !has_rt_hourly(&conn)? {
        return Ok(None);
    }
    let ts: Option<i64> = conn.query_row("SELECT MAX(ts) FROM rt_hourly", [], |row| row.get(0))?;
    Ok(ts.map(|ts| ts.div_euclid(3600)))
}

/// The hour index of each item's first stored bucket, for items in the items table (one index
/// seek each). The hourly strategy's age check needs it when only a recent window is loaded.
pub fn first_rt_hours(db_path: impl AsRef<Path>) -> Result<HashMap<i32, i64>> {
    let conn = open_db(db_path.as_ref())?;
    if !has_rt_hourly(&conn)? {
        return Ok(HashMap::new());
    }
    let mut stmt = conn.prepare("SELECT i.id, (SELECT MIN(ts) FROM rt_hourly WHERE item_id = i.id) FROM items i")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i32>(0)?, row.get::<_, Option<i64>>(1)?)))?;
    let (pairs, _) = collect_rows(rows, "first_rt_hours");
    Ok(pairs.into_iter().filter_map(|(id, ts)| Some((id, ts?.div_euclid(3600)))).collect())
}

#[derive(serde::Serialize)]
pub struct HistoryRow {
    pub date: String,
    pub price: i64,
    pub volume: i64,  // 0 when unknown
}

pub fn load_item_history(db_path: impl AsRef<Path>, item_id: i32) -> Result<Vec<HistoryRow>> {
    let conn = open_db(db_path.as_ref())?;

    let mut stmt = conn.prepare(
        "SELECT h.record_date, h.price, h.volume
         FROM history h
         WHERE h.item_id = ?1
         AND h.record_date >= date('now', '-365 days')
         ORDER BY h.record_date"
    )?;

    let rows = stmt.query_map([item_id], |row| {
        Ok(HistoryRow {
            date: row.get(0)?,
            price: row.get(1)?,
            volume: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
        })
    })?;

    Ok(collect_rows(rows, "load_item_history").0)
}

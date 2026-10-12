use rusqlite::{Connection, OpenFlags, Result};
use crate::model::ItemSnapshot;
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

    let rows = stmt.query_map([], |row| {
        Ok(ItemSnapshot {
            item_id: row.get(0)?,
            name: row.get(1)?,
            ge_limit: row.get(2)?,
            record_date: row.get(3)?,
            price: row.get(4)?,
            volume: row.get::<_, Option<i32>>(5)?.unwrap_or(0),
        })
    })?;

    Ok(collect_rows(rows, "load_snapshots"))
}

#[derive(serde::Serialize)]
pub struct HistoryRow {
    pub date: String,
    pub price: i32,
    pub volume: i32,  // 0 when unknown
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
            volume: row.get::<_, Option<i32>>(2)?.unwrap_or(0),
        })
    })?;

    Ok(collect_rows(rows, "load_item_history").0)
}

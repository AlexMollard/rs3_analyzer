use rusqlite::{Connection, Result};
use crate::model::ItemSnapshot;

pub fn load_snapshots(db_path: &str) -> Result<Vec<ItemSnapshot>> {
    let conn = Connection::open(db_path)?;
    
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
            volume: row.get(5)?,
        })
    })?;

    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn load_item_history(db_path: &str, item_name: &str) -> Result<Vec<(String, f64)>> {
    let conn = Connection::open(db_path)?;
    
    let mut stmt = conn.prepare(
        "SELECT h.record_date, h.price
         FROM history h
         JOIN items i ON h.item_id = i.id
         WHERE i.name = ?1
         AND h.record_date >= date('now', '-365 days')
         ORDER BY h.record_date"
    )?;

    let rows = stmt.query_map([item_name], |row| {
        Ok((row.get(0)?, row.get::<_, i32>(1)? as f64))
    })?;

    Ok(rows.filter_map(|r| r.ok()).collect())
}

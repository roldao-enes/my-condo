//! Small key/value store for application settings.

use rusqlite::{params, Connection, OptionalExtension};

pub fn get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        [key],
        |row| row.get(0),
    )
    .optional()
}

pub fn set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE
           SET value = excluded.value, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![key, value],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, key: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM app_settings WHERE key = ?1", [key])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;

    #[test]
    fn stores_updates_and_deletes_values() {
        let conn = open_in_memory().unwrap();
        assert_eq!(get(&conn, "k").unwrap(), None);
        set(&conn, "k", "one").unwrap();
        set(&conn, "k", "two").unwrap();
        assert_eq!(get(&conn, "k").unwrap().as_deref(), Some("two"));
        delete(&conn, "k").unwrap();
        assert_eq!(get(&conn, "k").unwrap(), None);
        assert!(set(&conn, "", "x").is_err()); // key length check
    }
}

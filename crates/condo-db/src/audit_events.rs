//! The transactional `audit_events` table. Each state change writes one row inside the same
//! transaction, so an operation and its audit record commit or roll back together.
//! (The separate rotating file log lives in `condo-audit`.)

use rusqlite::{params, Connection};
use serde_json::Value;

pub fn record(
    conn: &Connection,
    actor_id: i64,
    action: &str,
    entity_type: &str,
    entity_id: i64,
    metadata: Option<&Value>,
) -> rusqlite::Result<()> {
    let metadata = metadata.map_or_else(|| "{}".to_owned(), Value::to_string);
    conn.execute(
        "INSERT INTO audit_events (actor_id, action, entity_type, entity_id, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![actor_id, action, entity_type, entity_id, metadata],
    )?;
    Ok(())
}

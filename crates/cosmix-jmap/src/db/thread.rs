//! Thread storage operations.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use rusqlite::{Connection, params};
use uuid::Uuid;

/// Find or create a thread for a message based on in-reply-to / message-id.
pub async fn find_or_create(
    conn: &Arc<Mutex<Connection>>,
    account_id: i32,
    _message_id: Option<&str>,
    in_reply_to: Option<&[String]>,
) -> Result<Uuid> {
    let conn = conn.clone();
    let in_reply_to = in_reply_to.map(|s| s.to_vec());
    tokio::task::spawn_blocking(move || {
        let conn = conn.lock().map_err(|e| anyhow::anyhow!("lock error: {e}"))?;

        // Try to find an existing thread by in-reply-to
        if let Some(refs) = &in_reply_to {
            for ref_id in refs {
                let result: rusqlite::Result<String> = conn.query_row(
                    "SELECT thread_id FROM emails WHERE account_id = ?1 AND message_id = ?2 LIMIT 1",
                    params![account_id, ref_id],
                    |row| row.get(0),
                );
                if let Ok(thread_id_str) = result {
                    return Ok(thread_id_str.parse::<Uuid>()?);
                }
            }
        }

        // Create a new thread
        let id = Uuid::new_v4();
        let id_str = id.to_string();
        conn.execute(
            "INSERT INTO threads (id, account_id) VALUES (?1, ?2)",
            params![id_str, account_id],
        )?;
        Ok(id)
    }).await?
}

//! JMAP Email methods (RFC 8621).

use anyhow::Result;
use serde::Deserialize;
use uuid::Uuid;

use crate::db::{self, Db};
use crate::filter::SpamFilter;
use super::types::*;

/// Email/get — returns email metadata and optionally body values.
pub async fn get(db: &Db, account_id: i32, args: serde_json::Value) -> Result<serde_json::Value> {
    let acct = account_id.to_string();

    let fetch_text = args.get("fetchTextBodyValues").and_then(|v| v.as_bool()).unwrap_or(false);
    let fetch_html = args.get("fetchHTMLBodyValues").and_then(|v| v.as_bool()).unwrap_or(false);

    let ids: Option<Vec<String>> = args.get("ids")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let emails = if let Some(ids) = ids {
        let uuids: Vec<Uuid> = ids.iter().filter_map(|s| s.parse().ok()).collect();
        db::email::get_by_ids(&db.pool, account_id, &uuids).await?
    } else {
        let (ids, _) = db::email::query_ids(&db.pool, account_id, None, true, 0, 100).await?;
        db::email::get_by_ids(&db.pool, account_id, &ids).await?
    };

    let state = db::changelog::current_state(&db.pool, account_id, "Email").await?;

    // If body values requested, load blobs and parse MIME
    let list: Vec<serde_json::Value> = if fetch_text || fetch_html {
        let mut result = Vec::new();
        for email in &emails {
            let mut val = serde_json::to_value(email)?;
            if let Ok(Some(blob_data)) = db::blob::load(&db.pool, &db.blob_dir, email.blob_id).await {
                add_body_parts(&mut val, &blob_data, fetch_text, fetch_html);
            }
            result.push(val);
        }
        result
    } else {
        emails.iter().map(|e| serde_json::to_value(e).unwrap()).collect()
    };

    let resp = serde_json::json!({
        "accountId": acct,
        "state": state,
        "list": list,
        "notFound": [],
    });

    Ok(resp)
}

/// Parse a message and add textBody, htmlBody, bodyValues to the JSON response.
fn add_body_parts(val: &mut serde_json::Value, data: &[u8], fetch_text: bool, fetch_html: bool) {
    use mail_parser::{MessageParser, PartType};

    let parser = MessageParser::default();
    let Some(msg) = parser.parse(data) else { return };

    let mut body_values = serde_json::Map::new();
    let mut text_body = Vec::new();
    let mut html_body = Vec::new();
    let mut part_idx = 0u32;

    for part in msg.parts.iter() {
        let (is_text, is_html, body_text) = match &part.body {
            PartType::Text(text) => (true, false, text.as_ref().to_string()),
            PartType::Html(html) => (false, true, html.as_ref().to_string()),
            _ => continue,
        };

        if body_text.is_empty() { continue; }

        let pid = part_idx.to_string();
        let mime_type = if is_text { "text/plain" } else { "text/html" };

        if is_text {
            text_body.push(serde_json::json!({
                "partId": pid,
                "type": mime_type,
            }));
            if fetch_text {
                body_values.insert(pid.clone(), serde_json::json!({
                    "value": body_text,
                    "isEncodingProblem": false,
                    "isTruncated": false,
                }));
            }
        }

        if is_html {
            html_body.push(serde_json::json!({
                "partId": pid,
                "type": mime_type,
            }));
            if fetch_html {
                body_values.insert(pid.clone(), serde_json::json!({
                    "value": body_text,
                    "isEncodingProblem": false,
                    "isTruncated": false,
                }));
            }
        }

        part_idx += 1;
    }

    val["textBody"] = serde_json::Value::Array(text_body);
    val["htmlBody"] = serde_json::Value::Array(html_body);
    val["bodyValues"] = serde_json::Value::Object(body_values);
}

/// Email/query
pub async fn query(db: &Db, account_id: i32, args: serde_json::Value) -> Result<serde_json::Value> {
    let acct = account_id.to_string();

    #[derive(Deserialize)]
    struct Filter {
        #[serde(rename = "inMailbox")]
        in_mailbox: Option<String>,
    }

    let filter: Option<Filter> = args.get("filter")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let mailbox_id = filter
        .and_then(|f| f.in_mailbox)
        .and_then(|s| s.parse::<Uuid>().ok());

    let position = args.get("position").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as i64;

    let (ids, total) = db::email::query_ids(&db.pool, account_id, mailbox_id, true, position, limit).await?;
    let state = db::changelog::current_state(&db.pool, account_id, "Email").await?;

    let resp = QueryResponse {
        account_id: acct,
        query_state: state,
        can_calculate_changes: false,
        position: position as u64,
        ids: ids.into_iter().map(|u| u.to_string()).collect(),
        total: total as u64,
    };

    Ok(serde_json::to_value(resp)?)
}

/// Email/set — handles updates (keywords, mailboxIds with spam retraining) and destroy.
pub async fn set(
    db: &Db,
    account_id: i32,
    args: serde_json::Value,
    spam_filter: &SpamFilter,
) -> Result<serde_json::Value> {
    let acct = account_id.to_string();
    let old_state = db::changelog::current_state(&db.pool, account_id, "Email").await?;

    let mut updated_map = std::collections::HashMap::new();
    let mut destroyed_list = Vec::new();
    let mut not_updated = std::collections::HashMap::new();
    let mut not_destroyed = std::collections::HashMap::new();

    // Look up the Junk mailbox ID for spam retraining
    let junk_id = db::mailbox::get_by_role(&db.pool, account_id, "junk").await?;

    // Handle updates (keywords, mailboxIds)
    if let Some(update) = args.get("update").and_then(|v| v.as_object()) {
        for (id_str, patch) in update {
            let Ok(id) = id_str.parse::<Uuid>() else {
                not_updated.insert(id_str.clone(), SetError {
                    error_type: "invalidArguments".into(),
                    description: Some("Invalid id".into()),
                });
                continue;
            };

            let mut changed = false;

            if let Some(keywords) = patch.get("keywords") {
                if db::email::update_keywords(&db.pool, account_id, id, keywords).await? {
                    changed = true;
                }
            }

            if let Some(mailbox_ids) = patch.get("mailboxIds") {
                if let Some(obj) = mailbox_ids.as_object() {
                    let new_mbox_uuids: Vec<Uuid> = obj.keys()
                        .filter_map(|k| k.parse().ok())
                        .collect();

                    // Get old mailbox_ids + blob_id for retraining
                    if let Some(junk_id) = junk_id {
                        if let Ok(Some((old_mboxes, blob_id))) =
                            db::email::get_mailbox_and_blob(&db.pool, account_id, id).await
                        {
                            let was_in_junk = old_mboxes.contains(&junk_id);
                            let now_in_junk = new_mbox_uuids.contains(&junk_id);

                            if was_in_junk != now_in_junk {
                                // Retrain spamlite based on folder move
                                if let Ok(Some(blob_data)) = db::blob::load(&db.pool, &db.blob_dir, blob_id).await {
                                    if !was_in_junk && now_in_junk {
                                        // Moved TO Junk → train as spam
                                        if let Err(e) = spam_filter.train_spam(account_id, &blob_data) {
                                            tracing::warn!(error = %e, "Spam retrain failed");
                                        }
                                    } else {
                                        // Moved FROM Junk → train as good
                                        if let Err(e) = spam_filter.train_good(account_id, &blob_data) {
                                            tracing::warn!(error = %e, "Ham retrain failed");
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if db::email::update_mailboxes(&db.pool, account_id, id, &new_mbox_uuids).await? {
                        changed = true;
                    }
                }
            }

            if changed {
                db::changelog::record(&db.pool, account_id, "Email", id, "updated").await?;
                updated_map.insert(id_str.clone(), serde_json::Value::Null);
            }
        }
    }

    // Handle destroy
    if let Some(destroy) = args.get("destroy").and_then(|v| v.as_array()) {
        for id_val in destroy {
            let Some(id_str) = id_val.as_str() else { continue };
            let Ok(id) = id_str.parse::<Uuid>() else {
                not_destroyed.insert(id_str.to_string(), SetError {
                    error_type: "notFound".into(),
                    description: None,
                });
                continue;
            };

            if db::email::delete(&db.pool, account_id, id).await? {
                db::changelog::record(&db.pool, account_id, "Email", id, "destroyed").await?;
                destroyed_list.push(id_str.to_string());
            } else {
                not_destroyed.insert(id_str.to_string(), SetError {
                    error_type: "notFound".into(),
                    description: None,
                });
            }
        }
    }

    let new_state = db::changelog::current_state(&db.pool, account_id, "Email").await?;

    let resp = SetResponse {
        account_id: acct,
        old_state,
        new_state,
        created: None,
        updated: if updated_map.is_empty() { None } else { Some(updated_map.into_iter().map(|(k, v)| (k, v)).collect()) },
        destroyed: if destroyed_list.is_empty() { None } else { Some(destroyed_list) },
        not_created: None,
        not_updated: if not_updated.is_empty() { None } else { Some(not_updated) },
        not_destroyed: if not_destroyed.is_empty() { None } else { Some(not_destroyed) },
    };

    Ok(serde_json::to_value(resp)?)
}

/// Email/changes
pub async fn changes(db: &Db, account_id: i32, args: serde_json::Value) -> Result<serde_json::Value> {
    let acct = account_id.to_string();
    let since_state = args.get("sinceState")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);

    let max = args.get("maxChanges").and_then(|v| v.as_i64()).unwrap_or(500);

    let result = db::changelog::changes_since(&db.pool, account_id, "Email", since_state, max).await?;

    let resp = ChangesResponse {
        account_id: acct,
        old_state: since_state.to_string(),
        new_state: result.new_state,
        has_more_changes: result.has_more_changes,
        created: result.created,
        updated: result.updated,
        destroyed: result.destroyed,
    };

    Ok(serde_json::to_value(resp)?)
}

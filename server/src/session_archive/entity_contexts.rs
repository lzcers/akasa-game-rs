use agent::{agent::Context as AgentContext, core::Message};
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use story_engine::resources::session_events::{
    EntityContextItemAppended, EntityContextRollback, EntityContextRollbackPolicy,
};

use super::story_path::{ensure_linear_story_path, linear_node_id_for_depth};
use super::{SessionArchiveRepository, StoredEntityContext, schema};

impl SessionArchiveRepository {
    pub async fn save_entity_context_item(&self, update: &EntityContextItemAppended) -> Result<()> {
        let session_id = update.session_id.trim();
        let entity_name = update.entity_name.trim();
        if session_id.is_empty() || entity_name.is_empty() {
            return Ok(());
        }

        let node_id = linear_node_id_for_depth(update.round);
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("entity context items")?;
        schema::init(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        ensure_linear_story_path(&conn, session_id, update.round, &now)?;
        insert_entity_context_message(
            &conn,
            session_id,
            &node_id,
            entity_name,
            &update.message,
            &now,
        )?;
        Ok(())
    }
    pub async fn save_entity_context_rollback(
        &self,
        rollback: &EntityContextRollback,
    ) -> Result<()> {
        let session_id = rollback.session_id.trim();
        let entity_name = rollback.entity_name.trim();
        if session_id.is_empty() || entity_name.is_empty() {
            return Ok(());
        }

        let node_id = linear_node_id_for_depth(rollback.round);
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("entity context rollbacks")?;
        schema::init(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        ensure_linear_story_path(&conn, session_id, rollback.round, &now)?;
        match rollback.policy {
            EntityContextRollbackPolicy::LatestInput => {
                insert_entity_context_rollback(&conn, session_id, &node_id, entity_name, &now)?;
            }
        }
        Ok(())
    }
    pub async fn replace_entity_contexts_from_contexts(
        &self,
        session_id: &str,
        round: u64,
        contexts: &[(&str, &AgentContext)],
    ) -> Result<()> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }

        let node_id = linear_node_id_for_depth(round);
        let _guard = self.db.lock().await;
        let mut conn = self.db.open_connection("entity context replacement")?;
        schema::init(&conn)?;
        let tx = conn
            .transaction()
            .context("failed to start entity context replacement transaction")?;
        let now = chrono::Utc::now().to_rfc3339();
        ensure_linear_story_path(&tx, session_id, round, &now)?;
        tx.execute(
            "DELETE FROM entity_context_items WHERE session_id = ?1",
            params![session_id],
        )
        .context("failed to clear existing entity context items")?;
        for (entity_name, context) in contexts {
            for message in context_messages_for_storage(context) {
                insert_entity_context_message(
                    &tx,
                    session_id,
                    &node_id,
                    entity_name,
                    &message,
                    &now,
                )?;
            }
        }
        tx.commit()
            .context("failed to commit entity context replacement")?;
        Ok(())
    }
    pub async fn load_entity_contexts(&self, session_id: &str) -> Result<Vec<StoredEntityContext>> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("entity context items")?;
        schema::init(&conn)?;
        let mut stmt = conn
            .prepare(
                r#"
                WITH RECURSIVE active_path(node_id, parent_node_id, node_depth) AS (
                    SELECT n.node_id, n.parent_node_id, n.node_depth
                    FROM story_nodes n
                    JOIN sessions s
                        ON s.session_id = n.session_id
                        AND s.active_node_id = n.node_id
                    WHERE n.session_id = ?1
                    UNION ALL
                    SELECT parent.node_id, parent.parent_node_id, parent.node_depth
                    FROM story_nodes parent
                    JOIN active_path child
                        ON child.parent_node_id = parent.node_id
                    WHERE parent.session_id = ?1
                )
                SELECT
                    item.entity_name,
                    item.item_kind,
                    item.message_role,
                    item.content
                FROM entity_context_items item
                JOIN active_path
                    ON active_path.node_id = item.node_id
                WHERE item.session_id = ?1
                ORDER BY
                    item.entity_name ASC,
                    active_path.node_depth ASC,
                    item.item_index ASC,
                    item.id ASC
                "#,
            )
            .context("failed to prepare entity context load")?;
        let mut rows = stmt
            .query(params![session_id])
            .context("failed to query entity context items")?;
        let mut contexts = std::collections::BTreeMap::<String, AgentContext>::new();
        while let Some(row) = rows
            .next()
            .context("failed to read entity context item row")?
        {
            let entity_name: String = row.get(0)?;
            let item_kind: String = row.get(1)?;
            let context = contexts.entry(entity_name).or_default();
            match item_kind.as_str() {
                "message" => {
                    let message_role: String =
                        row.get::<_, Option<String>>(2)?.ok_or_else(|| {
                            rusqlite::Error::InvalidColumnType(
                                2,
                                "message_role".to_string(),
                                rusqlite::types::Type::Null,
                            )
                        })?;
                    let content: String = row.get::<_, Option<String>>(3)?.ok_or_else(|| {
                        rusqlite::Error::InvalidColumnType(
                            3,
                            "content".to_string(),
                            rusqlite::types::Type::Null,
                        )
                    })?;
                    let message =
                        message_from_role_and_content(&message_role, content).map_err(|err| {
                            rusqlite::Error::FromSqlConversionFailure(
                                2,
                                rusqlite::types::Type::Text,
                                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                            )
                        })?;
                    context.add_message(message);
                }
                "rollback_latest_input" => {
                    context.rollback_latest_input();
                }
                _ => {}
            }
        }

        Ok(contexts
            .into_iter()
            .map(|(entity_name, context)| StoredEntityContext {
                entity_name,
                context,
            })
            .collect())
    }
}

fn insert_entity_context_message(
    conn: &Connection,
    session_id: &str,
    node_id: &str,
    entity_name: &str,
    message: &Message,
    now: &str,
) -> Result<()> {
    let item_index = next_entity_context_item_index(conn, session_id, node_id, entity_name)?;
    conn.execute(
        r#"
        INSERT INTO entity_context_items (
            session_id,
            node_id,
            entity_name,
            item_index,
            item_kind,
            message_role,
            content,
            created_at
        ) VALUES (?1, ?2, ?3, ?4, 'message', ?5, ?6, ?7)
        "#,
        params![
            session_id,
            node_id,
            entity_name,
            item_index,
            message_role(message),
            message.content(),
            now,
        ],
    )
    .context("failed to insert entity context message")?;
    Ok(())
}
fn insert_entity_context_rollback(
    conn: &Connection,
    session_id: &str,
    node_id: &str,
    entity_name: &str,
    now: &str,
) -> Result<()> {
    let item_index = next_entity_context_item_index(conn, session_id, node_id, entity_name)?;
    conn.execute(
        r#"
        INSERT INTO entity_context_items (
            session_id,
            node_id,
            entity_name,
            item_index,
            item_kind,
            created_at
        ) VALUES (?1, ?2, ?3, ?4, 'rollback_latest_input', ?5)
        "#,
        params![session_id, node_id, entity_name, item_index, now],
    )
    .context("failed to insert entity context rollback")?;
    Ok(())
}
fn next_entity_context_item_index(
    conn: &Connection,
    session_id: &str,
    node_id: &str,
    entity_name: &str,
) -> Result<i64> {
    conn.query_row(
        r#"
        SELECT COALESCE(MAX(item_index), 0) + 1
        FROM entity_context_items
        WHERE session_id = ?1
            AND node_id = ?2
            AND entity_name = ?3
        "#,
        params![session_id, node_id, entity_name],
        |row| row.get(0),
    )
    .context("failed to compute next entity context item index")
}
fn context_messages_for_storage(context: &AgentContext) -> Vec<Message> {
    let messages = context.conversation();
    if messages.is_empty() {
        context.to_messages()
    } else {
        messages
    }
}
fn message_role(message: &Message) -> &'static str {
    match message {
        Message::System { .. } => "system",
        Message::User { .. } => "user",
        Message::Assistant { .. } => "assistant",
        Message::Tool { .. } => "tool",
    }
}
fn message_from_role_and_content(
    role: &str,
    content: String,
) -> std::result::Result<Message, String> {
    match role {
        "system" => Ok(Message::system(content)),
        "user" => Ok(Message::user(content)),
        "assistant" => Ok(Message::assistant(content)),
        "tool" => Err(
            "tool messages require tool_call_id and cannot be restored from role/content"
                .to_string(),
        ),
        other => Err(format!("unknown entity context message role: {other}")),
    }
}

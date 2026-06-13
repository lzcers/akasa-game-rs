use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::api::archive::{
    EntityContextItemArchive, EntityFlowOutputArchive, SessionDatabaseArchive,
    StoryEdgeActionArchive, StoryEdgeArchive, StoryNodeArchive,
};

use super::{SessionArchiveRepository, schema};

impl SessionArchiveRepository {
    pub async fn export_session_database_archive(
        &self,
        session_id: &str,
    ) -> Result<SessionDatabaseArchive> {
        let session_id = session_id.trim();
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("session database archive export")?;
        schema::init(&conn)?;

        let (active_node_id, total_node_count) = conn
            .query_row(
                r#"
                SELECT active_node_id, total_node_count
                FROM sessions
                WHERE session_id = ?1
                "#,
                params![session_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .context("failed to load session archive metadata")?
            .ok_or_else(|| anyhow::anyhow!("session `{session_id}` does not exist"))?;

        Ok(SessionDatabaseArchive {
            active_node_id,
            total_node_count,
            story_nodes: load_story_nodes(&conn, session_id)?,
            story_edges: load_story_edges(&conn, session_id)?,
            story_edge_actions: load_story_edge_actions(&conn, session_id)?,
            entity_flow_outputs: load_entity_flow_outputs(&conn, session_id)?,
            entity_context_items: load_entity_context_items(&conn, session_id)?,
        })
    }

    pub async fn restore_session_database_archive(
        &self,
        session_id: &str,
        archive: &SessionDatabaseArchive,
    ) -> Result<()> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }

        let _guard = self.db.lock().await;
        let mut conn = self
            .db
            .open_connection("session database archive restore")?;
        schema::init(&conn)?;
        let tx = conn
            .transaction()
            .context("failed to start session database archive restore")?;

        tx.execute(
            r#"
            UPDATE sessions
            SET active_node_id = ?2,
                total_node_count = ?3
            WHERE session_id = ?1
            "#,
            params![session_id, archive.active_node_id, archive.total_node_count],
        )
        .context("failed to restore session archive metadata")?;

        for node in &archive.story_nodes {
            tx.execute(
                r#"
                INSERT INTO story_nodes (
                    session_id,
                    node_id,
                    parent_node_id,
                    node_depth,
                    sequence_index,
                    phase,
                    flow_end,
                    created_at,
                    updated_at,
                    last_accessed_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(session_id, node_id) DO UPDATE SET
                    parent_node_id = excluded.parent_node_id,
                    node_depth = excluded.node_depth,
                    sequence_index = excluded.sequence_index,
                    phase = excluded.phase,
                    flow_end = excluded.flow_end,
                    created_at = excluded.created_at,
                    updated_at = excluded.updated_at,
                    last_accessed_at = excluded.last_accessed_at
                "#,
                params![
                    session_id,
                    node.node_id,
                    node.parent_node_id,
                    node.node_depth,
                    node.sequence_index,
                    node.phase,
                    node.flow_end,
                    node.created_at,
                    node.updated_at,
                    node.last_accessed_at,
                ],
            )
            .context("failed to restore story node")?;
        }

        for edge in &archive.story_edges {
            tx.execute(
                r#"
                INSERT INTO story_edges (
                    session_id,
                    from_node_id,
                    to_node_id,
                    created_at
                ) VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(session_id, from_node_id, to_node_id) DO UPDATE SET
                    created_at = excluded.created_at
                "#,
                params![
                    session_id,
                    edge.from_node_id,
                    edge.to_node_id,
                    edge.created_at
                ],
            )
            .context("failed to restore story edge")?;
        }

        for action in &archive.story_edge_actions {
            tx.execute(
                r#"
                INSERT INTO story_edge_actions (
                    session_id,
                    from_node_id,
                    to_node_id,
                    character_name,
                    player_id,
                    action_type,
                    title,
                    action,
                    motivation_and_risk,
                    submitted_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(session_id, from_node_id, to_node_id, character_name) DO UPDATE SET
                    player_id = excluded.player_id,
                    action_type = excluded.action_type,
                    title = excluded.title,
                    action = excluded.action,
                    motivation_and_risk = excluded.motivation_and_risk,
                    submitted_at = excluded.submitted_at
                "#,
                params![
                    session_id,
                    action.from_node_id,
                    action.to_node_id,
                    action.character_name,
                    action.player_id,
                    action.action_type,
                    action.title,
                    action.action,
                    action.motivation_and_risk,
                    action.submitted_at,
                ],
            )
            .context("failed to restore story edge action")?;
        }

        for output in &archive.entity_flow_outputs {
            tx.execute(
                r#"
                INSERT INTO entity_flow_outputs (
                    session_id,
                    node_id,
                    stage,
                    entity_name,
                    output_type,
                    content,
                    created_at,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(session_id, node_id, stage, entity_name, output_type) DO UPDATE SET
                    content = excluded.content,
                    created_at = excluded.created_at,
                    updated_at = excluded.updated_at
                "#,
                params![
                    session_id,
                    output.node_id,
                    output.stage,
                    output.entity_name,
                    output.output_type,
                    output.content,
                    output.created_at,
                    output.updated_at,
                ],
            )
            .context("failed to restore entity flow output")?;
        }

        for item in &archive.entity_context_items {
            tx.execute(
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
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(session_id, node_id, entity_name, item_index) DO UPDATE SET
                    item_kind = excluded.item_kind,
                    message_role = excluded.message_role,
                    content = excluded.content,
                    created_at = excluded.created_at
                "#,
                params![
                    session_id,
                    item.node_id,
                    item.entity_name,
                    item.item_index,
                    item.item_kind,
                    item.message_role,
                    item.content,
                    item.created_at,
                ],
            )
            .context("failed to restore entity context item")?;
        }

        tx.commit()
            .context("failed to commit session database archive restore")?;
        Ok(())
    }
}

fn load_story_nodes(conn: &Connection, session_id: &str) -> Result<Vec<StoryNodeArchive>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT node_id, parent_node_id, node_depth, sequence_index, phase, flow_end,
                created_at, updated_at, last_accessed_at
            FROM story_nodes
            WHERE session_id = ?1
            ORDER BY node_depth ASC, sequence_index ASC, node_id ASC
            "#,
        )
        .context("failed to prepare story node archive query")?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(StoryNodeArchive {
                node_id: row.get(0)?,
                parent_node_id: row.get(1)?,
                node_depth: row.get(2)?,
                sequence_index: row.get(3)?,
                phase: row.get(4)?,
                flow_end: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                last_accessed_at: row.get(8)?,
            })
        })
        .context("failed to query story node archive")?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read story node archive")
}

fn load_story_edges(conn: &Connection, session_id: &str) -> Result<Vec<StoryEdgeArchive>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT from_node_id, to_node_id, created_at
            FROM story_edges
            WHERE session_id = ?1
            ORDER BY from_node_id ASC, to_node_id ASC
            "#,
        )
        .context("failed to prepare story edge archive query")?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(StoryEdgeArchive {
                from_node_id: row.get(0)?,
                to_node_id: row.get(1)?,
                created_at: row.get(2)?,
            })
        })
        .context("failed to query story edge archive")?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read story edge archive")
}

fn load_story_edge_actions(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<StoryEdgeActionArchive>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT from_node_id, to_node_id, character_name, player_id, action_type,
                title, action, motivation_and_risk, submitted_at
            FROM story_edge_actions
            WHERE session_id = ?1
            ORDER BY from_node_id ASC, to_node_id ASC, character_name ASC
            "#,
        )
        .context("failed to prepare story edge action archive query")?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(StoryEdgeActionArchive {
                from_node_id: row.get(0)?,
                to_node_id: row.get(1)?,
                character_name: row.get(2)?,
                player_id: row.get(3)?,
                action_type: row.get(4)?,
                title: row.get(5)?,
                action: row.get(6)?,
                motivation_and_risk: row.get(7)?,
                submitted_at: row.get(8)?,
            })
        })
        .context("failed to query story edge action archive")?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read story edge action archive")
}

fn load_entity_flow_outputs(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<EntityFlowOutputArchive>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT node_id, stage, entity_name, output_type, content, created_at, updated_at
            FROM entity_flow_outputs
            WHERE session_id = ?1
            ORDER BY node_id ASC, stage ASC, entity_name ASC, output_type ASC
            "#,
        )
        .context("failed to prepare entity flow output archive query")?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(EntityFlowOutputArchive {
                node_id: row.get(0)?,
                stage: row.get(1)?,
                entity_name: row.get(2)?,
                output_type: row.get(3)?,
                content: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .context("failed to query entity flow output archive")?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read entity flow output archive")
}

fn load_entity_context_items(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<EntityContextItemArchive>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT node_id, entity_name, item_index, item_kind, message_role, content, created_at
            FROM entity_context_items
            WHERE session_id = ?1
            ORDER BY node_id ASC, entity_name ASC, item_index ASC
            "#,
        )
        .context("failed to prepare entity context item archive query")?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(EntityContextItemArchive {
                node_id: row.get(0)?,
                entity_name: row.get(1)?,
                item_index: row.get(2)?,
                item_kind: row.get(3)?,
                message_role: row.get(4)?,
                content: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .context("failed to query entity context item archive")?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read entity context item archive")
}

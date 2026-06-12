use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};
use story_engine::components::{outcome::PlayerActionItem, world_snapshot::WorldSnapshot};

use super::codec::{deserialize_phase, deserialize_player_action_type};
use super::{
    ActivatedStorylineNode, SessionArchiveRepository, StoredStoryline, StoredStorylineEdge,
    StoredStorylineNode, schema,
};

impl SessionArchiveRepository {
    pub async fn load_storyline(&self, session_id: &str) -> Result<Option<StoredStoryline>> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(None);
        }

        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("storyline")?;
        schema::init(&conn)?;

        let metadata = conn
            .query_row(
                r#"
                SELECT root_node_id, active_node_id
                FROM sessions
                WHERE session_id = ?1
                "#,
                params![session_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .context("failed to query storyline session metadata")?;
        let Some((root_node_id, active_node_id)) = metadata else {
            return Ok(None);
        };

        let nodes = load_storyline_nodes(&conn, session_id)?;
        let edges = load_storyline_edges(&conn, session_id)?;

        Ok(Some(StoredStoryline {
            root_node_id,
            active_node_id,
            nodes,
            edges,
        }))
    }

    pub async fn activate_storyline_node(
        &self,
        session_id: &str,
        node_id: &str,
    ) -> Result<Option<ActivatedStorylineNode>> {
        let session_id = session_id.trim();
        let node_id = node_id.trim();
        if session_id.is_empty() || node_id.is_empty() {
            return Ok(None);
        }

        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("storyline activation")?;
        schema::init(&conn)?;
        let Some((round, sequence_index, flow_end)) =
            load_selectable_storyline_node(&conn, session_id, node_id)?
        else {
            return Ok(None);
        };

        let now = chrono::Utc::now().to_rfc3339();
        let total_node_count = round.max(sequence_index);
        conn.execute(
            r#"
            UPDATE story_nodes
            SET updated_at = ?3,
                last_accessed_at = ?3
            WHERE session_id = ?1
                AND node_id = ?2
            "#,
            params![session_id, node_id, now],
        )
        .context("failed to touch selected storyline node")?;
        conn.execute(
            r#"
            UPDATE sessions
            SET active_node_id = ?2,
                total_node_count = MAX(total_node_count, ?3),
                updated_at = ?4,
                last_accessed_at = ?4
            WHERE session_id = ?1
            "#,
            params![session_id, node_id, total_node_count, now],
        )
        .context("failed to activate selected storyline node")?;

        Ok(Some(ActivatedStorylineNode { round, flow_end }))
    }
}

fn load_selectable_storyline_node(
    conn: &rusqlite::Connection,
    session_id: &str,
    node_id: &str,
) -> Result<Option<(u64, u64, bool)>> {
    conn.query_row(
        r#"
        SELECT node.node_depth, node.sequence_index, node.flow_end
        FROM story_nodes node
        WHERE node.session_id = ?1
            AND node.node_id = ?2
            AND node.node_depth > 0
            AND EXISTS (
                SELECT 1
                FROM entity_flow_outputs output
                WHERE output.session_id = node.session_id
                    AND output.node_id = node.node_id
                    AND output.stage = 'simulation'
                    AND output.output_type = 'json'
                    AND length(trim(output.content)) > 0
            )
            AND EXISTS (
                SELECT 1
                FROM entity_flow_outputs output
                WHERE output.session_id = node.session_id
                    AND output.node_id = node.node_id
                    AND output.stage = 'application'
                    AND output.output_type = 'text'
                    AND length(trim(output.content)) > 0
            )
        "#,
        params![session_id, node_id],
        |row| {
            let round = row.get::<_, i64>(0)?.try_into().unwrap_or_default();
            let sequence_index = row.get::<_, i64>(1)?.try_into().unwrap_or_default();
            let flow_end = row.get(2)?;
            Ok((round, sequence_index, flow_end))
        },
    )
    .optional()
    .context("failed to load selectable storyline node")
}

fn load_storyline_nodes(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Vec<StoredStorylineNode>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT
                node.node_id,
                node.parent_node_id,
                node.node_depth,
                node.sequence_index,
                node.phase,
                node.flow_end,
                node.created_at,
                node.updated_at,
                node.last_accessed_at,
                (
                    SELECT output.content
                    FROM entity_flow_outputs output
                    WHERE output.session_id = node.session_id
                        AND output.node_id = node.node_id
                        AND output.stage = 'simulation'
                        AND output.output_type = 'json'
                        AND length(trim(output.content)) > 0
                    ORDER BY
                        CASE output.entity_name
                            WHEN 'FateWeaver' THEN 0
                            ELSE 1
                        END,
                        output.entity_name ASC
                    LIMIT 1
                ) AS world_snapshot_json,
                (
                    SELECT output.content
                    FROM entity_flow_outputs output
                    WHERE output.session_id = node.session_id
                        AND output.node_id = node.node_id
                        AND output.stage = 'application'
                        AND output.output_type = 'text'
                        AND length(trim(output.content)) > 0
                    ORDER BY
                        CASE output.entity_name
                            WHEN 'UpperNarrator' THEN 0
                            ELSE 1
                        END,
                        output.entity_name ASC
                    LIMIT 1
                ) AS narration_text
            FROM story_nodes node
            WHERE node.session_id = ?1
                AND (
                    node.node_depth = 0
                    OR EXISTS (
                        SELECT 1
                        FROM entity_flow_outputs generated_output
                        WHERE generated_output.session_id = node.session_id
                            AND generated_output.node_id = node.node_id
                            AND length(trim(generated_output.content)) > 0
                    )
                )
            ORDER BY node.node_depth ASC, node.sequence_index ASC, node.node_id ASC
            "#,
        )
        .context("failed to prepare storyline node query")?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            let phase: String = row.get(4)?;
            let round = row.get::<_, i64>(2)?.try_into().unwrap_or_default();
            let sequence_index = row.get::<_, i64>(3)?.try_into().unwrap_or_default();
            let world_snapshot_json = row.get::<_, Option<String>>(9)?;
            let narration_text = row.get::<_, Option<String>>(10)?.unwrap_or_default();
            Ok(StoredStorylineNode {
                node_id: row.get(0)?,
                parent_node_id: row.get(1)?,
                round,
                sequence_index,
                phase: deserialize_phase(&phase).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                    )
                })?,
                flow_end: row.get(5)?,
                title: story_node_title(round, world_snapshot_json.as_deref()),
                narration_text,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                last_accessed_at: row.get(8)?,
            })
        })
        .context("failed to query storyline nodes")?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read storyline nodes")
}

fn load_storyline_edges(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Vec<StoredStorylineEdge>> {
    let actions_by_edge = load_storyline_edge_actions(conn, session_id)?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT edge.from_node_id, edge.to_node_id, edge.created_at
            FROM story_edges edge
            JOIN story_nodes from_node
                ON from_node.session_id = edge.session_id
                AND from_node.node_id = edge.from_node_id
            JOIN story_nodes to_node
                ON to_node.session_id = edge.session_id
                AND to_node.node_id = edge.to_node_id
            WHERE edge.session_id = ?1
                AND (
                    from_node.node_depth = 0
                    OR EXISTS (
                        SELECT 1
                        FROM entity_flow_outputs from_output
                        WHERE from_output.session_id = from_node.session_id
                            AND from_output.node_id = from_node.node_id
                            AND length(trim(from_output.content)) > 0
                    )
                )
                AND (
                    to_node.node_depth = 0
                    OR EXISTS (
                        SELECT 1
                        FROM entity_flow_outputs to_output
                        WHERE to_output.session_id = to_node.session_id
                            AND to_output.node_id = to_node.node_id
                            AND length(trim(to_output.content)) > 0
                    )
                )
            ORDER BY
                from_node.node_depth ASC,
                to_node.node_depth ASC,
                edge.created_at ASC,
                edge.from_node_id ASC,
                edge.to_node_id ASC
            "#,
        )
        .context("failed to prepare storyline edge query")?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .context("failed to query storyline edges")?;

    let mut edges = Vec::new();
    for row in rows {
        let (from_node_id, to_node_id, created_at) =
            row.context("failed to read storyline edge")?;
        let actions = actions_by_edge
            .get(&(from_node_id.clone(), to_node_id.clone()))
            .cloned()
            .unwrap_or_default();
        edges.push(StoredStorylineEdge {
            from_node_id,
            to_node_id,
            actions,
            created_at,
        });
    }

    Ok(edges)
}

fn load_storyline_edge_actions(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<BTreeMap<(String, String), Vec<PlayerActionItem>>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT
                from_node_id,
                to_node_id,
                character_name,
                player_id,
                action_type,
                title,
                action,
                motivation_and_risk
            FROM story_edge_actions
            WHERE session_id = ?1
            ORDER BY from_node_id ASC, to_node_id ASC, submitted_at ASC, character_name ASC
            "#,
        )
        .context("failed to prepare storyline edge action query")?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            let action_type: String = row.get(4)?;
            Ok((
                (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
                PlayerActionItem {
                    character_name: row.get(2)?,
                    player_id: row.get(3)?,
                    action_type: deserialize_player_action_type(&action_type).map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                        )
                    })?,
                    title: row.get(5)?,
                    action: row.get(6)?,
                    motivation_and_risk: row.get(7)?,
                },
            ))
        })
        .context("failed to query storyline edge actions")?;

    let mut actions_by_edge = BTreeMap::new();
    for row in rows {
        let (edge_key, action) = row.context("failed to read storyline edge action")?;
        actions_by_edge
            .entry(edge_key)
            .or_insert_with(Vec::new)
            .push(action);
    }
    Ok(actions_by_edge)
}

fn story_node_title(round: u64, world_snapshot_json: Option<&str>) -> String {
    world_snapshot_json
        .and_then(|content| serde_json::from_str::<WorldSnapshot>(content).ok())
        .map(|snapshot| snapshot.scene_title.trim().to_string())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| {
            if round == 0 {
                "根节点".to_string()
            } else {
                format!("第 {round} 章")
            }
        })
}

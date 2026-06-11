use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::session_history::TurnPhase;

use super::codec::serialize_phase;
use super::{ROOT_NODE_ID, normalized_character_name};

#[derive(Debug, Clone)]
pub(super) struct StoryPathNode {
    pub(super) node_id: String,
    pub(super) node_depth: u64,
}

pub(super) struct SessionBaseRecord<'a> {
    pub(super) session_id: &'a str,
    pub(super) character_name: &'a str,
    pub(super) world_profile: &'a str,
    pub(super) character_profile: &'a str,
    pub(super) key_story_beats: &'a str,
    pub(super) active_node_id: &'a str,
    pub(super) total_node_count: i64,
}
struct StoryNodeSeed<'a> {
    session_id: &'a str,
    node_id: &'a str,
    parent_node_id: Option<&'a str>,
    node_depth: u64,
    phase: TurnPhase,
    flow_end: bool,
}
pub(super) fn upsert_session_base(
    conn: &Connection,
    record: SessionBaseRecord<'_>,
    now: &str,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO sessions (
            session_id,
            root_node_id,
            active_node_id,
            total_node_count,
            created_at,
            updated_at,
            last_accessed_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?5)
        ON CONFLICT(session_id) DO UPDATE SET
            root_node_id = excluded.root_node_id,
            active_node_id = excluded.active_node_id,
            total_node_count = MAX(sessions.total_node_count, excluded.total_node_count),
            updated_at = excluded.updated_at,
            last_accessed_at = excluded.last_accessed_at
        "#,
        params![
            record.session_id,
            ROOT_NODE_ID,
            record.active_node_id,
            record.total_node_count,
            now,
        ],
    )
    .context("failed to upsert session metadata")?;
    conn.execute(
        r#"
        INSERT INTO session_worlds (
            session_id,
            world_profile,
            global_key_story_beats,
            created_at,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?4)
        ON CONFLICT(session_id) DO UPDATE SET
            world_profile = excluded.world_profile,
            global_key_story_beats = excluded.global_key_story_beats,
            updated_at = excluded.updated_at
        "#,
        params![
            record.session_id,
            record.world_profile,
            record.key_story_beats,
            now,
        ],
    )
    .context("failed to upsert session world profile")?;
    conn.execute(
        r#"
        INSERT INTO session_characters (
            session_id,
            character_name,
            character_profile,
            key_story_beats,
            player_id,
            is_playable,
            created_at,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, NULL, 1, ?5, ?5)
        ON CONFLICT(session_id, character_name) DO UPDATE SET
            character_profile = excluded.character_profile,
            key_story_beats = excluded.key_story_beats,
            is_playable = excluded.is_playable,
            updated_at = excluded.updated_at
        "#,
        params![
            record.session_id,
            normalized_character_name(record.character_name),
            record.character_profile,
            record.key_story_beats,
            now,
        ],
    )
    .context("failed to upsert session character profile")?;
    Ok(())
}
pub(super) fn ensure_linear_story_path(
    conn: &Connection,
    session_id: &str,
    through_depth: u64,
    now: &str,
) -> Result<()> {
    ensure_story_node_exists(
        conn,
        StoryNodeSeed {
            session_id,
            node_id: ROOT_NODE_ID,
            parent_node_id: None,
            node_depth: 0,
            phase: TurnPhase::Start,
            flow_end: false,
        },
        now,
    )?;

    for depth in 1..=through_depth {
        let node_id = linear_node_id_for_depth(depth);
        let parent_node_id = linear_node_id_for_depth(depth.saturating_sub(1));
        ensure_story_node_exists(
            conn,
            StoryNodeSeed {
                session_id,
                node_id: &node_id,
                parent_node_id: Some(parent_node_id.as_str()),
                node_depth: depth,
                phase: TurnPhase::TurnCompleted,
                flow_end: false,
            },
            now,
        )?;
    }

    Ok(())
}
fn ensure_story_node_exists(conn: &Connection, seed: StoryNodeSeed<'_>, now: &str) -> Result<()> {
    let node_depth = i64::try_from(seed.node_depth)
        .context("story node node_depth exceeds SQLite INTEGER range")?;
    conn.execute(
        r#"
        INSERT OR IGNORE INTO story_nodes (
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
        ) VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?7, ?7)
        "#,
        params![
            seed.session_id,
            seed.node_id,
            seed.parent_node_id,
            node_depth,
            serialize_phase(seed.phase)?,
            seed.flow_end,
            now,
        ],
    )
    .context("failed to ensure story node")?;
    Ok(())
}
pub(super) fn update_story_node_state(
    conn: &Connection,
    session_id: &str,
    node_id: &str,
    phase: TurnPhase,
    flow_end: Option<bool>,
    now: &str,
) -> Result<()> {
    conn.execute(
        r#"
        UPDATE story_nodes
        SET phase = ?3,
            flow_end = COALESCE(?4, flow_end),
            updated_at = ?5,
            last_accessed_at = ?5
        WHERE session_id = ?1
            AND node_id = ?2
        "#,
        params![session_id, node_id, serialize_phase(phase)?, flow_end, now,],
    )
    .context("failed to update story node state")?;
    Ok(())
}
pub(super) fn linear_node_id_for_depth(depth: u64) -> String {
    if depth == 0 {
        ROOT_NODE_ID.to_string()
    } else {
        format!("node-{depth}")
    }
}
pub(super) fn turn_state_from_active_node(phase: TurnPhase, node_depth: u64) -> (u64, u64) {
    match phase {
        TurnPhase::Start => (0, 0),
        TurnPhase::Simulation | TurnPhase::Application | TurnPhase::Failed => {
            (node_depth.saturating_sub(1), node_depth)
        }
        TurnPhase::AwaitingPlayer | TurnPhase::TurnCompleted | TurnPhase::Ended => {
            (node_depth, node_depth)
        }
    }
}
pub(super) fn select_story_path_nodes(
    conn: &Connection,
    session_id: &str,
    before_round: Option<u64>,
    limit: Option<usize>,
) -> Result<Vec<StoryPathNode>> {
    let mut nodes = select_active_story_path_nodes(conn, session_id)?;
    if nodes.is_empty() {
        nodes = select_all_story_nodes_with_outputs(conn, session_id)?;
    }

    if let Some(before_round) = before_round {
        nodes.retain(|node| node.node_depth < before_round);
    }

    if let Some(limit) = limit {
        nodes.truncate(limit);
    }

    Ok(nodes)
}
fn select_active_story_path_nodes(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<StoryPathNode>> {
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
            SELECT active_path.node_id, active_path.node_depth
            FROM active_path
            WHERE active_path.node_depth > 0
                AND EXISTS (
                    SELECT 1
                    FROM entity_flow_outputs output
                    WHERE output.session_id = ?1
                        AND output.node_id = active_path.node_id
                )
            ORDER BY active_path.node_depth DESC
            "#,
        )
        .context("failed to prepare active story path query")?;
    let rows = stmt
        .query_map(params![session_id], story_path_node_from_row)
        .context("failed to query active story path")?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read active story path")
}
fn select_all_story_nodes_with_outputs(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<StoryPathNode>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT node.node_id, node.node_depth
            FROM story_nodes node
            WHERE node.session_id = ?1
                AND node.node_depth > 0
                AND EXISTS (
                    SELECT 1
                    FROM entity_flow_outputs output
                    WHERE output.session_id = ?1
                        AND output.node_id = node.node_id
                )
            ORDER BY node.node_depth DESC, node.sequence_index DESC
            "#,
        )
        .context("failed to prepare story node fallback query")?;
    let rows = stmt
        .query_map(params![session_id], story_path_node_from_row)
        .context("failed to query story node fallback path")?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read story node fallback path")
}
fn story_path_node_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoryPathNode> {
    Ok(StoryPathNode {
        node_id: row.get(0)?,
        node_depth: row.get::<_, i64>(1)?.try_into().unwrap_or_default(),
    })
}

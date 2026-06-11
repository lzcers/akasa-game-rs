use anyhow::{Context, Result};
use rusqlite::Connection;

const CREATE_SESSIONS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    root_node_id TEXT NOT NULL,
    active_node_id TEXT NOT NULL,
    total_node_count INTEGER NOT NULL,
    world_profile TEXT NOT NULL,
    protagonist_profile TEXT NOT NULL,
    key_story_beats TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_accessed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_active_node
ON sessions(session_id, active_node_id);
"#;

const CREATE_STORY_NODES_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS story_nodes (
    session_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    parent_node_id TEXT,
    node_depth INTEGER NOT NULL,
    sequence_index INTEGER NOT NULL,
    phase TEXT NOT NULL,
    flow_end INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_accessed_at TEXT NOT NULL,
    PRIMARY KEY (session_id, node_id)
);

CREATE INDEX IF NOT EXISTS idx_story_nodes_parent
ON story_nodes(session_id, parent_node_id);

CREATE INDEX IF NOT EXISTS idx_story_nodes_depth
ON story_nodes(session_id, node_depth);

CREATE INDEX IF NOT EXISTS idx_story_nodes_sequence
ON story_nodes(session_id, sequence_index);
"#;

const CREATE_STORY_EDGES_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS story_edges (
    session_id TEXT NOT NULL,
    from_node_id TEXT NOT NULL,
    to_node_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (session_id, from_node_id, to_node_id)
);

CREATE INDEX IF NOT EXISTS idx_story_edges_from
ON story_edges(session_id, from_node_id);

CREATE INDEX IF NOT EXISTS idx_story_edges_to
ON story_edges(session_id, to_node_id);
"#;

const CREATE_STORY_EDGE_ACTIONS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS story_edge_actions (
    session_id TEXT NOT NULL,
    from_node_id TEXT NOT NULL,
    to_node_id TEXT NOT NULL,
    character_id TEXT NOT NULL,
    player_id TEXT,
    action_type TEXT NOT NULL,
    title TEXT NOT NULL,
    action TEXT NOT NULL,
    motivation_and_risk TEXT NOT NULL,
    submitted_at TEXT NOT NULL,
    PRIMARY KEY (session_id, from_node_id, to_node_id, character_id)
);

CREATE INDEX IF NOT EXISTS idx_story_edge_actions_edge
ON story_edge_actions(session_id, from_node_id, to_node_id);

CREATE INDEX IF NOT EXISTS idx_story_edge_actions_character
ON story_edge_actions(session_id, character_id);
"#;

const CREATE_FLOW_OUTPUTS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS flow_outputs (
    session_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    stage TEXT NOT NULL,
    entity_name TEXT NOT NULL,
    output_type TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (session_id, node_id, stage, entity_name, output_type)
);

CREATE INDEX IF NOT EXISTS idx_flow_outputs_node
ON flow_outputs(session_id, node_id);
"#;

const DROP_OBSOLETE_AGENT_CONTEXTS_TABLE_SQL: &str = r#"
DROP TABLE IF EXISTS agent_contexts;
"#;

const CREATE_AGENT_CONTEXT_ITEMS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS agent_context_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    item_index INTEGER NOT NULL,
    item_kind TEXT NOT NULL,
    message_role TEXT,
    content TEXT,
    message_json TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(session_id, node_id, agent_name, item_index)
);

CREATE INDEX IF NOT EXISTS idx_agent_context_items_node
ON agent_context_items(session_id, node_id, agent_name, item_index);
"#;

pub(super) fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(DROP_OBSOLETE_AGENT_CONTEXTS_TABLE_SQL)
        .context("failed to drop obsolete agent contexts schema")?;
    reset_obsolete_story_edges_schema(conn)?;
    conn.execute_batch(CREATE_SESSIONS_TABLE_SQL)
        .context("failed to initialize sessions schema")?;
    conn.execute_batch(CREATE_STORY_NODES_TABLE_SQL)
        .context("failed to initialize story nodes schema")?;
    conn.execute_batch(CREATE_STORY_EDGES_TABLE_SQL)
        .context("failed to initialize story edges schema")?;
    conn.execute_batch(CREATE_STORY_EDGE_ACTIONS_TABLE_SQL)
        .context("failed to initialize story edge actions schema")?;
    conn.execute_batch(CREATE_FLOW_OUTPUTS_TABLE_SQL)
        .context("failed to initialize flow outputs schema")?;
    conn.execute_batch(CREATE_AGENT_CONTEXT_ITEMS_TABLE_SQL)
        .context("failed to initialize agent context items schema")
}

fn reset_obsolete_story_edges_schema(conn: &Connection) -> Result<()> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(story_edges)")
        .context("failed to inspect story edges schema")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .context("failed to query story edges schema")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read story edges schema")?;
    if columns
        .iter()
        .any(|column| column == "action" || column == "action_type")
    {
        conn.execute_batch(
            r#"
            DROP TABLE IF EXISTS story_edge_actions;
            DROP TABLE IF EXISTS story_edges;
            "#,
        )
        .context("failed to reset obsolete story edges schema")?;
    }
    Ok(())
}

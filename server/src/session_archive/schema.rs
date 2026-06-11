use anyhow::{Context, Result};
use rusqlite::Connection;

const CREATE_SESSIONS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    root_node_id TEXT NOT NULL,
    active_node_id TEXT NOT NULL,
    total_node_count INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_accessed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_active_node
ON sessions(session_id, active_node_id);
"#;

const CREATE_SESSION_WORLDS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS session_worlds (
    session_id TEXT PRIMARY KEY,
    world_profile TEXT NOT NULL,
    global_key_story_beats TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

const CREATE_SESSION_CHARACTERS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS session_characters (
    session_id TEXT NOT NULL,
    character_name TEXT NOT NULL,
    character_profile TEXT NOT NULL,
    key_story_beats TEXT NOT NULL,
    player_id TEXT,
    is_playable INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (session_id, character_name)
);

CREATE INDEX IF NOT EXISTS idx_session_characters_playable
ON session_characters(session_id, is_playable);
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
    character_name TEXT NOT NULL,
    player_id TEXT,
    action_type TEXT NOT NULL,
    title TEXT NOT NULL,
    action TEXT NOT NULL,
    motivation_and_risk TEXT NOT NULL,
    submitted_at TEXT NOT NULL,
    PRIMARY KEY (session_id, from_node_id, to_node_id, character_name)
);

CREATE INDEX IF NOT EXISTS idx_story_edge_actions_edge
ON story_edge_actions(session_id, from_node_id, to_node_id);

CREATE INDEX IF NOT EXISTS idx_story_edge_actions_character_name
ON story_edge_actions(session_id, character_name);
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

const DROP_OBSOLETE_CONTEXT_TABLES_SQL: &str = r#"
DROP TABLE IF EXISTS agent_contexts;
DROP TABLE IF EXISTS agent_context_items;
"#;

const CREATE_ENTITY_CONTEXT_ITEMS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS entity_context_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    entity_name TEXT NOT NULL,
    item_index INTEGER NOT NULL,
    item_kind TEXT NOT NULL,
    message_role TEXT,
    content TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(session_id, node_id, entity_name, item_index)
);

CREATE INDEX IF NOT EXISTS idx_entity_context_items_node
ON entity_context_items(session_id, node_id, entity_name, item_index);
"#;

pub(super) fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(DROP_OBSOLETE_CONTEXT_TABLES_SQL)
        .context("failed to drop obsolete context schema")?;
    reset_obsolete_sessions_schema(conn)?;
    reset_obsolete_story_edges_schema(conn)?;
    reset_obsolete_entity_context_items_schema(conn)?;
    conn.execute_batch(CREATE_SESSIONS_TABLE_SQL)
        .context("failed to initialize sessions schema")?;
    conn.execute_batch(CREATE_SESSION_WORLDS_TABLE_SQL)
        .context("failed to initialize session worlds schema")?;
    conn.execute_batch(CREATE_SESSION_CHARACTERS_TABLE_SQL)
        .context("failed to initialize session characters schema")?;
    conn.execute_batch(CREATE_STORY_NODES_TABLE_SQL)
        .context("failed to initialize story nodes schema")?;
    conn.execute_batch(CREATE_STORY_EDGES_TABLE_SQL)
        .context("failed to initialize story edges schema")?;
    conn.execute_batch(CREATE_STORY_EDGE_ACTIONS_TABLE_SQL)
        .context("failed to initialize story edge actions schema")?;
    conn.execute_batch(CREATE_FLOW_OUTPUTS_TABLE_SQL)
        .context("failed to initialize flow outputs schema")?;
    conn.execute_batch(CREATE_ENTITY_CONTEXT_ITEMS_TABLE_SQL)
        .context("failed to initialize entity context items schema")
}

fn reset_obsolete_sessions_schema(conn: &Connection) -> Result<()> {
    let columns = table_columns(conn, "sessions").context("failed to inspect sessions schema")?;
    if columns.iter().any(|column| {
        column == "world_profile" || column == "character_profile" || column == "key_story_beats"
    }) {
        conn.execute_batch(
            r#"
            DROP TABLE IF EXISTS entity_context_items;
            DROP TABLE IF EXISTS agent_context_items;
            DROP TABLE IF EXISTS flow_outputs;
            DROP TABLE IF EXISTS story_edge_actions;
            DROP TABLE IF EXISTS story_edges;
            DROP TABLE IF EXISTS story_nodes;
            DROP TABLE IF EXISTS session_characters;
            DROP TABLE IF EXISTS session_worlds;
            DROP TABLE IF EXISTS sessions;
            "#,
        )
        .context("failed to reset obsolete sessions schema")?;
    }
    Ok(())
}

fn reset_obsolete_story_edges_schema(conn: &Connection) -> Result<()> {
    let columns =
        table_columns(conn, "story_edges").context("failed to inspect story edges schema")?;
    let action_columns = table_columns(conn, "story_edge_actions")
        .context("failed to inspect story edge actions schema")?;
    if columns
        .iter()
        .any(|column| column == "action" || column == "action_type")
        || action_columns.iter().any(|column| column == "character_id")
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

fn reset_obsolete_entity_context_items_schema(conn: &Connection) -> Result<()> {
    let columns = table_columns(conn, "entity_context_items")
        .context("failed to inspect entity context items schema")?;
    if columns
        .iter()
        .any(|column| column == "agent_name" || column == "message_json")
    {
        conn.execute_batch("DROP TABLE IF EXISTS entity_context_items;")
            .context("failed to reset obsolete entity context items schema")?;
    }
    Ok(())
}

fn table_columns(conn: &Connection, table_name: &str) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table_name})"))
        .with_context(|| format!("failed to inspect {table_name} schema"))?;
    stmt.query_map([], |row| row.get::<_, String>(1))
        .with_context(|| format!("failed to query {table_name} schema"))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read {table_name} schema"))
}

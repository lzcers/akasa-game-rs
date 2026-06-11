use agent::agent::Context as AgentContext;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use story_engine::{
    components::outcome::PlayerActionType,
    resources::session_events::{AgentContextUpdate, FlowTurnError, PlayerInput, SessionCreated},
};

use crate::session_history::{RoundHistoryEntry, TurnPhase};

use crate::database::AppDatabase;

const CREATE_GAME_SESSION_ARCHIVES_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS game_session_archives (
    session_id TEXT PRIMARY KEY,
    compressed_archive TEXT NOT NULL,
    title TEXT,
    phase TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_accessed_at TEXT NOT NULL
);
"#;

const CREATE_SESSION_ROUNDS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS session_rounds (
    session_id TEXT NOT NULL,
    round INTEGER NOT NULL,
    history_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (session_id, round)
);

CREATE INDEX IF NOT EXISTS idx_session_rounds_session_round
ON session_rounds(session_id, round);
"#;

const CREATE_SESSIONS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    world_profile TEXT NOT NULL,
    protagonist_profile TEXT NOT NULL,
    key_story_beats TEXT NOT NULL,
    phase TEXT NOT NULL,
    turn_index INTEGER NOT NULL,
    active_turn_id INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_accessed_at TEXT NOT NULL
);
"#;

const CREATE_FLOW_TURNS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS flow_turns (
    session_id TEXT NOT NULL,
    round INTEGER NOT NULL,
    history_json TEXT NOT NULL,
    completed_at TEXT,
    ended_at TEXT,
    error_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (session_id, round)
);

CREATE INDEX IF NOT EXISTS idx_flow_turns_session_round
ON flow_turns(session_id, round);
"#;

const CREATE_AGENT_CONTEXTS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS agent_contexts (
    session_id TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    round INTEGER NOT NULL,
    context_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (session_id, agent_name)
);

CREATE INDEX IF NOT EXISTS idx_agent_contexts_session
ON agent_contexts(session_id);
"#;

const CREATE_PLAYER_INPUTS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS player_inputs (
    session_id TEXT NOT NULL,
    round INTEGER NOT NULL,
    action_type TEXT NOT NULL,
    action TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (session_id, round)
);

CREATE INDEX IF NOT EXISTS idx_player_inputs_session_round
ON player_inputs(session_id, round);
"#;

#[derive(Debug, Clone)]
pub struct SessionArchiveRepository {
    db: AppDatabase,
}

#[derive(Debug, Clone)]
pub struct StoredSessionArchive {
    pub session_id: String,
    pub compressed_archive: String,
    pub title: Option<String>,
    pub phase: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed_at: String,
}

#[derive(Debug, Clone)]
pub struct StoredSessionMetadata {
    pub session_id: String,
    pub world_profile: String,
    pub protagonist_profile: String,
    pub key_story_beats: String,
    pub phase: TurnPhase,
    pub turn_index: u64,
    pub active_turn_id: u64,
}

#[derive(Debug, Clone)]
pub struct StoredAgentContext {
    pub agent_name: String,
    pub context: AgentContext,
}

#[derive(Debug, Clone)]
pub struct StoredPlayerInput {
    pub round: u64,
    pub action_type: PlayerActionType,
    pub action: String,
}

#[derive(Debug, Clone)]
pub struct StoredSessionRoundPage {
    pub rounds: Vec<RoundHistoryEntry>,
    pub next_before_round: Option<u64>,
    pub has_more: bool,
}

impl SessionArchiveRepository {
    pub fn new(db: AppDatabase) -> Self {
        Self { db }
    }

    pub async fn save_session_created(&self, event: &SessionCreated) -> Result<()> {
        let session_id = event.session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }

        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("sessions")?;
        init_schema(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            r#"
            INSERT INTO sessions (
                session_id,
                world_profile,
                protagonist_profile,
                key_story_beats,
                phase,
                turn_index,
                active_turn_id,
                created_at,
                updated_at,
                last_accessed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6, ?6, ?6)
            ON CONFLICT(session_id) DO UPDATE SET
                world_profile = excluded.world_profile,
                protagonist_profile = excluded.protagonist_profile,
                key_story_beats = excluded.key_story_beats,
                updated_at = excluded.updated_at,
                last_accessed_at = excluded.last_accessed_at
            "#,
            params![
                session_id,
                event.world_profile,
                event.protagonist_profile,
                event.key_story_beats,
                serialize_phase(TurnPhase::Idle)?,
                now,
            ],
        )
        .context("failed to upsert session metadata")?;
        Ok(())
    }

    pub async fn save_session_metadata(&self, metadata: &StoredSessionMetadata) -> Result<()> {
        let session_id = metadata.session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }

        let turn_index = i64::try_from(metadata.turn_index)
            .context("turn_index exceeds SQLite INTEGER range")?;
        let active_turn_id = i64::try_from(metadata.active_turn_id)
            .context("active_turn_id exceeds SQLite INTEGER range")?;
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("sessions")?;
        init_schema(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            r#"
            INSERT INTO sessions (
                session_id,
                world_profile,
                protagonist_profile,
                key_story_beats,
                phase,
                turn_index,
                active_turn_id,
                created_at,
                updated_at,
                last_accessed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?8)
            ON CONFLICT(session_id) DO UPDATE SET
                world_profile = excluded.world_profile,
                protagonist_profile = excluded.protagonist_profile,
                key_story_beats = excluded.key_story_beats,
                phase = excluded.phase,
                turn_index = excluded.turn_index,
                active_turn_id = excluded.active_turn_id,
                updated_at = excluded.updated_at,
                last_accessed_at = excluded.last_accessed_at
            "#,
            params![
                session_id,
                metadata.world_profile,
                metadata.protagonist_profile,
                metadata.key_story_beats,
                serialize_phase(metadata.phase)?,
                turn_index,
                active_turn_id,
                now,
            ],
        )
        .context("failed to upsert session metadata")?;
        Ok(())
    }

    pub async fn update_session_turn_state(
        &self,
        session_id: &str,
        phase: TurnPhase,
        turn_index: u64,
        active_turn_id: u64,
    ) -> Result<()> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }

        let turn_index = i64::try_from(turn_index).context("turn_index exceeds SQLite range")?;
        let active_turn_id =
            i64::try_from(active_turn_id).context("active_turn_id exceeds SQLite range")?;
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("sessions")?;
        init_schema(&conn)?;
        conn.execute(
            r#"
            UPDATE sessions
            SET phase = ?2,
                turn_index = ?3,
                active_turn_id = ?4,
                updated_at = ?5,
                last_accessed_at = ?5
            WHERE session_id = ?1
            "#,
            params![
                session_id,
                serialize_phase(phase)?,
                turn_index,
                active_turn_id,
                chrono::Utc::now().to_rfc3339(),
            ],
        )
        .context("failed to update session turn state")?;
        Ok(())
    }

    pub async fn load_session_metadata(
        &self,
        session_id: &str,
    ) -> Result<Option<StoredSessionMetadata>> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("sessions")?;
        init_schema(&conn)?;
        conn.query_row(
            r#"
            SELECT
                session_id,
                world_profile,
                protagonist_profile,
                key_story_beats,
                phase,
                turn_index,
                active_turn_id
            FROM sessions
            WHERE session_id = ?1
            "#,
            params![session_id],
            |row| {
                let phase: String = row.get(4)?;
                Ok(StoredSessionMetadata {
                    session_id: row.get(0)?,
                    world_profile: row.get(1)?,
                    protagonist_profile: row.get(2)?,
                    key_story_beats: row.get(3)?,
                    phase: deserialize_phase(&phase).map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                        )
                    })?,
                    turn_index: row.get::<_, i64>(5)?.try_into().unwrap_or_default(),
                    active_turn_id: row.get::<_, i64>(6)?.try_into().unwrap_or_default(),
                })
            },
        )
        .optional()
        .context("failed to load session metadata")
    }

    pub async fn save_agent_context(&self, update: &AgentContextUpdate) -> Result<()> {
        let session_id = update.session_id.trim();
        let agent_name = update.agent_name.trim();
        if session_id.is_empty() || agent_name.is_empty() {
            return Ok(());
        }

        let round =
            i64::try_from(update.round).context("agent context round exceeds SQLite range")?;
        let context_json =
            serde_json::to_string(&update.context).context("failed to serialize agent context")?;
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("agent contexts")?;
        init_schema(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            r#"
            INSERT INTO agent_contexts (
                session_id,
                agent_name,
                round,
                context_json,
                created_at,
                updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
            ON CONFLICT(session_id, agent_name) DO UPDATE SET
                round = excluded.round,
                context_json = excluded.context_json,
                updated_at = excluded.updated_at
            "#,
            params![session_id, agent_name, round, context_json, now],
        )
        .context("failed to upsert agent context")?;
        Ok(())
    }

    pub async fn load_agent_contexts(&self, session_id: &str) -> Result<Vec<StoredAgentContext>> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("agent contexts")?;
        init_schema(&conn)?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT agent_name, context_json
                FROM agent_contexts
                WHERE session_id = ?1
                ORDER BY agent_name ASC
                "#,
            )
            .context("failed to prepare agent context load")?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                let context_json: String = row.get(1)?;
                let context =
                    serde_json::from_str::<AgentContext>(&context_json).map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            Box::new(err),
                        )
                    })?;
                Ok(StoredAgentContext {
                    agent_name: row.get(0)?,
                    context,
                })
            })
            .context("failed to query agent contexts")?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read agent contexts")
    }

    pub async fn save_player_input(&self, input: &PlayerInput) -> Result<()> {
        let session_id = input.session_id.trim();
        let action = input.action.trim();
        if session_id.is_empty() || action.is_empty() {
            return Ok(());
        }

        let round =
            i64::try_from(input.round).context("player input round exceeds SQLite range")?;
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("player inputs")?;
        init_schema(&conn)?;
        conn.execute(
            r#"
            INSERT INTO player_inputs (
                session_id,
                round,
                action_type,
                action,
                created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(session_id, round) DO UPDATE SET
                action_type = excluded.action_type,
                action = excluded.action,
                created_at = excluded.created_at
            "#,
            params![
                session_id,
                round,
                serialize_player_action_type(input.action_type)?,
                action,
                chrono::Utc::now().to_rfc3339(),
            ],
        )
        .context("failed to upsert player input")?;
        Ok(())
    }

    pub async fn load_player_inputs(&self, session_id: &str) -> Result<Vec<StoredPlayerInput>> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("player inputs")?;
        init_schema(&conn)?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT round, action_type, action
                FROM player_inputs
                WHERE session_id = ?1
                ORDER BY round ASC
                "#,
            )
            .context("failed to prepare player input load")?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                let action_type: String = row.get(1)?;
                Ok(StoredPlayerInput {
                    round: row.get::<_, i64>(0)?.try_into().unwrap_or_default(),
                    action_type: deserialize_player_action_type(&action_type).map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                        )
                    })?,
                    action: row.get(2)?,
                })
            })
            .context("failed to query player inputs")?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read player inputs")
    }

    pub async fn record_flow_turn_completed(&self, session_id: &str, round: u64) -> Result<()> {
        self.update_session_turn_state(session_id, TurnPhase::TurnCompleted, round, round)
            .await?;
        self.set_flow_turn_timestamp(session_id, round, "completed_at")
            .await
    }

    pub async fn record_flow_turn_end(&self, session_id: &str, round: u64) -> Result<()> {
        self.update_session_turn_state(session_id, TurnPhase::Ended, round, round)
            .await?;
        self.set_flow_turn_timestamp(session_id, round, "ended_at")
            .await
    }

    pub async fn record_flow_turn_error(&self, error: &FlowTurnError) -> Result<()> {
        self.update_session_turn_state(
            &error.session_id,
            TurnPhase::Failed,
            error.round,
            error.round,
        )
        .await?;
        let error_json = serde_json::to_string(error).context("failed to serialize flow error")?;
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("flow turns")?;
        init_schema(&conn)?;
        conn.execute(
            r#"
            INSERT INTO flow_turns (
                session_id,
                round,
                history_json,
                error_json,
                created_at,
                updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
            ON CONFLICT(session_id, round) DO UPDATE SET
                error_json = excluded.error_json,
                updated_at = excluded.updated_at
            "#,
            params![
                error.session_id,
                i64::try_from(error.round).context("flow turn round exceeds SQLite range")?,
                serde_json::to_string(&RoundHistoryEntry {
                    round: error.round,
                    ..RoundHistoryEntry::default()
                })
                .context("failed to serialize empty flow turn history")?,
                error_json,
                chrono::Utc::now().to_rfc3339(),
            ],
        )
        .context("failed to record flow turn error")?;
        Ok(())
    }

    async fn set_flow_turn_timestamp(
        &self,
        session_id: &str,
        round: u64,
        column: &str,
    ) -> Result<()> {
        let column = match column {
            "completed_at" => "completed_at",
            "ended_at" => "ended_at",
            _ => return Ok(()),
        };
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }

        let round = i64::try_from(round).context("flow turn round exceeds SQLite range")?;
        let now = chrono::Utc::now().to_rfc3339();
        let empty_history = serde_json::to_string(&RoundHistoryEntry {
            round: round.try_into().unwrap_or_default(),
            ..RoundHistoryEntry::default()
        })
        .context("failed to serialize empty flow turn history")?;
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("flow turns")?;
        init_schema(&conn)?;
        conn.execute(
            &format!(
                r#"
                INSERT INTO flow_turns (
                    session_id,
                    round,
                    history_json,
                    {column},
                    created_at,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?4, ?4)
                ON CONFLICT(session_id, round) DO UPDATE SET
                    {column} = excluded.{column},
                    updated_at = excluded.updated_at
                "#
            ),
            params![session_id, round, empty_history, now],
        )
        .context("failed to set flow turn timestamp")?;
        Ok(())
    }

    pub async fn save_archive(&self, archive: StoredSessionArchive) -> Result<()> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("session archive")?;
        init_schema(&conn)?;
        conn.execute(
            r#"
            INSERT INTO game_session_archives (
                session_id,
                compressed_archive,
                title,
                phase,
                created_at,
                updated_at,
                last_accessed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(session_id) DO UPDATE SET
                compressed_archive = excluded.compressed_archive,
                title = excluded.title,
                phase = excluded.phase,
                updated_at = excluded.updated_at,
                last_accessed_at = excluded.last_accessed_at
            "#,
            params![
                archive.session_id,
                archive.compressed_archive,
                archive.title,
                archive.phase,
                archive.created_at,
                archive.updated_at,
                archive.last_accessed_at,
            ],
        )
        .context("failed to upsert game session archive")?;

        Ok(())
    }

    pub async fn load_archive(&self, session_id: &str) -> Result<Option<StoredSessionArchive>> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("session archive")?;
        init_schema(&conn)?;
        conn.query_row(
            r#"
            SELECT
                session_id,
                compressed_archive,
                title,
                phase,
                created_at,
                updated_at,
                last_accessed_at
            FROM game_session_archives
            WHERE session_id = ?1
            "#,
            params![session_id],
            |row| {
                Ok(StoredSessionArchive {
                    session_id: row.get(0)?,
                    compressed_archive: row.get(1)?,
                    title: row.get(2)?,
                    phase: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    last_accessed_at: row.get(6)?,
                })
            },
        )
        .optional()
        .context("failed to load game session archive")
    }

    pub async fn save_rounds(&self, session_id: &str, rounds: &[RoundHistoryEntry]) -> Result<()> {
        if rounds.is_empty() {
            return Ok(());
        }

        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }

        let _guard = self.db.lock().await;
        let mut conn = self.db.open_connection("session rounds")?;
        init_schema(&conn)?;
        let tx = conn
            .transaction()
            .context("failed to start session rounds transaction")?;
        let now = chrono::Utc::now().to_rfc3339();

        for round in rounds {
            let round_index =
                i64::try_from(round.round).context("session round exceeds SQLite INTEGER range")?;
            let round = load_flow_turn_in_transaction(&tx, session_id, round_index)?
                .or(load_round_in_transaction(&tx, session_id, round_index)?)
                .map(|existing| merge_round_history(existing, round.clone()))
                .unwrap_or_else(|| round.clone());
            let history_json =
                serde_json::to_string(&round).context("failed to serialize session round")?;
            tx.execute(
                r#"
                INSERT INTO flow_turns (
                    session_id,
                    round,
                    history_json,
                    created_at,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?4)
                ON CONFLICT(session_id, round) DO UPDATE SET
                    history_json = excluded.history_json,
                    updated_at = excluded.updated_at
                "#,
                params![session_id, round_index, history_json, now],
            )
            .context("failed to upsert flow turn")?;
            tx.execute(
                r#"
                INSERT INTO session_rounds (
                    session_id,
                    round,
                    history_json,
                    created_at,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?4)
                ON CONFLICT(session_id, round) DO UPDATE SET
                    history_json = excluded.history_json,
                    updated_at = excluded.updated_at
                "#,
                params![session_id, round_index, history_json, now],
            )
            .context("failed to upsert session round")?;
        }

        tx.commit()
            .context("failed to commit session rounds transaction")?;
        Ok(())
    }

    pub async fn load_rounds(&self, session_id: &str) -> Result<Vec<RoundHistoryEntry>> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("session rounds")?;
        init_schema(&conn)?;
        let flow_turns = load_rounds_from_table(&conn, "flow_turns", session_id)?;
        if !flow_turns.is_empty() {
            return Ok(flow_turns);
        }
        load_rounds_from_table(&conn, "session_rounds", session_id)
    }

    pub async fn load_round_page(
        &self,
        session_id: &str,
        before_round: Option<u64>,
        limit: usize,
    ) -> Result<StoredSessionRoundPage> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("session rounds")?;
        init_schema(&conn)?;
        let page =
            load_round_page_from_table(&conn, "flow_turns", session_id, before_round, limit)?;
        if !page.rounds.is_empty() || !has_legacy_rounds(&conn, session_id)? {
            return Ok(page);
        }
        load_round_page_from_table(&conn, "session_rounds", session_id, before_round, limit)
    }
}

fn load_rounds_from_table(
    conn: &Connection,
    table_name: &str,
    session_id: &str,
) -> Result<Vec<RoundHistoryEntry>> {
    let mut stmt = conn
        .prepare(&format!(
            r#"
                SELECT history_json
                FROM {table_name}
                WHERE session_id = ?1
                ORDER BY round ASC
                "#
        ))
        .context("failed to prepare session rounds load")?;
    let rows = stmt
        .query_map(params![session_id], |row| row.get::<_, String>(0))
        .context("failed to query session rounds")?;

    rows.map(|row| parse_round_json(&row?))
        .collect::<Result<Vec<_>>>()
}

fn load_round_page_from_table(
    conn: &Connection,
    table_name: &str,
    session_id: &str,
    before_round: Option<u64>,
    limit: usize,
) -> Result<StoredSessionRoundPage> {
    let limit = limit.max(1);
    let fetch_limit = i64::try_from(limit + 1).context("session round page limit too large")?;
    let before_round = before_round
        .map(i64::try_from)
        .transpose()
        .context("before round exceeds SQLite INTEGER range")?;
    let mut round_json = match before_round {
        Some(before_round) => {
            let mut stmt = conn
                .prepare(&format!(
                    r#"
                        SELECT history_json
                        FROM {table_name}
                        WHERE session_id = ?1 AND round < ?2
                        ORDER BY round DESC
                        LIMIT ?3
                        "#
                ))
                .context("failed to prepare older session rounds page")?;
            stmt.query_map(params![session_id, before_round, fetch_limit], |row| {
                row.get::<_, String>(0)
            })
            .context("failed to query older session rounds page")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read older session rounds page")?
        }
        None => {
            let mut stmt = conn
                .prepare(&format!(
                    r#"
                        SELECT history_json
                        FROM {table_name}
                        WHERE session_id = ?1
                        ORDER BY round DESC
                        LIMIT ?2
                        "#
                ))
                .context("failed to prepare latest session rounds page")?;
            stmt.query_map(params![session_id, fetch_limit], |row| {
                row.get::<_, String>(0)
            })
            .context("failed to query latest session rounds page")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read latest session rounds page")?
        }
    };

    let has_more = round_json.len() > limit;
    if has_more {
        round_json.truncate(limit);
    }
    let mut rounds = round_json
        .into_iter()
        .map(|json| parse_round_json(&json))
        .collect::<Result<Vec<_>>>()?;
    rounds.reverse();
    let next_before_round = has_more.then(|| {
        rounds
            .first()
            .expect("extra row implies at least one returned round")
            .round
    });

    Ok(StoredSessionRoundPage {
        rounds,
        next_before_round,
        has_more,
    })
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(CREATE_GAME_SESSION_ARCHIVES_TABLE_SQL)
        .context("failed to initialize game session archive schema")?;
    conn.execute_batch(CREATE_SESSION_ROUNDS_TABLE_SQL)
        .context("failed to initialize session rounds schema")?;
    conn.execute_batch(CREATE_SESSIONS_TABLE_SQL)
        .context("failed to initialize sessions schema")?;
    conn.execute_batch(CREATE_FLOW_TURNS_TABLE_SQL)
        .context("failed to initialize flow turns schema")?;
    conn.execute_batch(CREATE_AGENT_CONTEXTS_TABLE_SQL)
        .context("failed to initialize agent contexts schema")?;
    conn.execute_batch(CREATE_PLAYER_INPUTS_TABLE_SQL)
        .context("failed to initialize player inputs schema")
}

fn parse_round_json(json: &str) -> Result<RoundHistoryEntry> {
    serde_json::from_str(json).context("failed to deserialize session round")
}

fn load_round_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    session_id: &str,
    round: i64,
) -> Result<Option<RoundHistoryEntry>> {
    let json = tx
        .query_row(
            r#"
            SELECT history_json
            FROM session_rounds
            WHERE session_id = ?1 AND round = ?2
            "#,
            params![session_id, round],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .context("failed to load existing session round")?;

    json.as_deref().map(parse_round_json).transpose()
}

fn load_flow_turn_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    session_id: &str,
    round: i64,
) -> Result<Option<RoundHistoryEntry>> {
    let json = tx
        .query_row(
            r#"
            SELECT history_json
            FROM flow_turns
            WHERE session_id = ?1 AND round = ?2
            "#,
            params![session_id, round],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .context("failed to load existing flow turn")?;

    json.as_deref().map(parse_round_json).transpose()
}

fn has_legacy_rounds(conn: &Connection, session_id: &str) -> Result<bool> {
    let count: i64 = conn
        .query_row(
            r#"
            SELECT COUNT(*)
            FROM session_rounds
            WHERE session_id = ?1
            "#,
            params![session_id],
            |row| row.get(0),
        )
        .context("failed to count legacy session rounds")?;
    Ok(count > 0)
}

fn serialize_phase(phase: TurnPhase) -> Result<String> {
    serde_json::to_string(&phase)
        .map(|value| value.trim_matches('"').to_string())
        .context("failed to serialize turn phase")
}

fn deserialize_phase(value: &str) -> std::result::Result<TurnPhase, String> {
    serde_json::from_str(&format!("{value:?}")).map_err(|err| err.to_string())
}

fn serialize_player_action_type(action_type: PlayerActionType) -> Result<String> {
    serde_json::to_string(&action_type)
        .map(|value| value.trim_matches('"').to_string())
        .context("failed to serialize player action type")
}

fn deserialize_player_action_type(value: &str) -> std::result::Result<PlayerActionType, String> {
    serde_json::from_str(&format!("{value:?}")).map_err(|err| err.to_string())
}

fn merge_round_history(
    existing: RoundHistoryEntry,
    incoming: RoundHistoryEntry,
) -> RoundHistoryEntry {
    let narration_text = incoming
        .narration_text
        .filter(|text| !text.trim().is_empty())
        .or(existing.narration_text);
    let committed_action = incoming
        .committed_action
        .filter(|action| !action.trim().is_empty())
        .or(existing.committed_action);
    let choices = if incoming.choices.is_empty() {
        existing.choices
    } else {
        incoming.choices
    };

    RoundHistoryEntry {
        round: incoming.round,
        world_snapshot: incoming.world_snapshot.or(existing.world_snapshot),
        narration_text,
        choices,
        committed_action,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_history::RoundHistoryEntry;
    use crate::{analytics::AnalyticsRepository, api::site::AnalyticsEventInput};
    use agent::core::Message;
    use serde_json::json;
    use uuid::Uuid;

    #[tokio::test]
    async fn save_archive_upserts_and_loads_by_session_id() {
        let repo = test_repo();
        let archive = StoredSessionArchive {
            session_id: "session-one".to_string(),
            compressed_archive: "first".to_string(),
            title: Some("First".to_string()),
            phase: "awaiting_player".to_string(),
            created_at: "2026-06-10T00:00:00Z".to_string(),
            updated_at: "2026-06-10T00:00:00Z".to_string(),
            last_accessed_at: "2026-06-10T00:00:00Z".to_string(),
        };

        repo.save_archive(archive.clone())
            .await
            .expect("archive should save");
        repo.save_archive(StoredSessionArchive {
            compressed_archive: "second".to_string(),
            updated_at: "2026-06-10T00:01:00Z".to_string(),
            ..archive
        })
        .await
        .expect("archive should update");

        let loaded = repo
            .load_archive("session-one")
            .await
            .expect("archive should load")
            .expect("archive should exist");

        assert_eq!(loaded.compressed_archive, "second");
        assert_eq!(loaded.title.as_deref(), Some("First"));
        assert_eq!(loaded.updated_at, "2026-06-10T00:01:00Z");
    }

    #[tokio::test]
    async fn shared_database_stores_analytics_and_session_archives() {
        let db = AppDatabase::new(std::env::temp_dir().join(format!(
            "akasa-shared-db-{}.sqlite3",
            Uuid::new_v4().simple()
        )));
        let analytics = AnalyticsRepository::new(db.clone());
        let archives = SessionArchiveRepository::new(db);

        analytics
            .append_events(&[AnalyticsEventInput {
                id: "evt-shared".to_string(),
                event_name: "session_created".to_string(),
                anonymous_user_id: "anon-shared".to_string(),
                client_session_id: "visit-shared".to_string(),
                game_session_id: Some("session-shared".to_string()),
                source_session_id: None,
                occurred_at: "2026-06-10T00:00:00Z".to_string(),
                app: "game-web".to_string(),
                app_version: None,
                path: Some("/play".to_string()),
                referrer_domain: None,
                utm_source: None,
                utm_medium: None,
                utm_campaign: None,
                device_type: Some("desktop".to_string()),
                platform: Some("MacIntel".to_string()),
                properties: json!({}),
            }])
            .await
            .expect("analytics event should save");
        archives
            .save_archive(StoredSessionArchive {
                session_id: "session-shared".to_string(),
                compressed_archive: "archive".to_string(),
                title: Some("Shared".to_string()),
                phase: "awaiting_player".to_string(),
                created_at: "2026-06-10T00:00:00Z".to_string(),
                updated_at: "2026-06-10T00:00:00Z".to_string(),
                last_accessed_at: "2026-06-10T00:00:00Z".to_string(),
            })
            .await
            .expect("archive should save");

        let summary = analytics
            .summary(24 * 30)
            .await
            .expect("summary should read");
        let archive = archives
            .load_archive("session-shared")
            .await
            .expect("archive should load")
            .expect("archive should exist");

        assert_eq!(summary.totals.events, 1);
        assert_eq!(archive.compressed_archive, "archive");
    }

    #[tokio::test]
    async fn session_rounds_upsert_and_page_with_before_cursor() {
        let repo = test_repo();
        let rounds = (1..=5)
            .map(|round| round_entry(round, &format!("round-{round}")))
            .collect::<Vec<_>>();

        repo.save_rounds("session-rounds", &rounds)
            .await
            .expect("rounds should save");
        repo.save_rounds("session-rounds", &[round_entry(3, "round-3-updated")])
            .await
            .expect("round should upsert");

        let latest = repo
            .load_round_page("session-rounds", None, 2)
            .await
            .expect("latest page should load");
        assert_eq!(
            latest
                .rounds
                .iter()
                .map(|entry| entry.round)
                .collect::<Vec<_>>(),
            vec![4, 5]
        );
        assert_eq!(latest.next_before_round, Some(4));
        assert!(latest.has_more);

        let older = repo
            .load_round_page("session-rounds", latest.next_before_round, 2)
            .await
            .expect("older page should load");
        assert_eq!(
            older
                .rounds
                .iter()
                .map(|entry| entry.round)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(
            older.rounds[1].narration_text.as_deref(),
            Some("round-3-updated")
        );
        assert_eq!(older.next_before_round, Some(2));
        assert!(older.has_more);
    }

    #[tokio::test]
    async fn session_flow_turn_and_agent_context_tables_round_trip() {
        let repo = test_repo();
        repo.save_session_created(&SessionCreated {
            session_id: "session-db".to_string(),
            world_profile: "world".to_string(),
            protagonist_profile: "hero".to_string(),
            key_story_beats: "beats".to_string(),
        })
        .await
        .expect("session metadata should save");

        let metadata = repo
            .load_session_metadata("session-db")
            .await
            .expect("metadata should load")
            .expect("metadata should exist");
        assert_eq!(metadata.world_profile, "world");
        assert_eq!(metadata.phase, TurnPhase::Idle);

        repo.save_rounds("session-db", &[round_entry(1, "round-1")])
            .await
            .expect("flow turn should save");
        repo.record_flow_turn_completed("session-db", 1)
            .await
            .expect("completion should save");
        let metadata = repo
            .load_session_metadata("session-db")
            .await
            .expect("metadata should load")
            .expect("metadata should exist");
        assert_eq!(metadata.phase, TurnPhase::TurnCompleted);
        assert_eq!(metadata.turn_index, 1);
        assert_eq!(metadata.active_turn_id, 1);

        let mut context = AgentContext::new();
        context.add_message(Message::user("latest context"));
        repo.save_agent_context(&AgentContextUpdate {
            session_id: "session-db".to_string(),
            round: 1,
            agent_name: "UpperNarrator".to_string(),
            context,
        })
        .await
        .expect("agent context should save");
        let contexts = repo
            .load_agent_contexts("session-db")
            .await
            .expect("agent contexts should load");
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].agent_name, "UpperNarrator");
        assert!(matches!(
            contexts[0].context.conversation().last(),
            Some(Message::User { content }) if content == "latest context"
        ));

        repo.save_player_input(&PlayerInput {
            session_id: "session-db".to_string(),
            round: 1,
            action_type: PlayerActionType::SelectedOption,
            action: "绕到钟楼背面".to_string(),
        })
        .await
        .expect("player input should save");
        let inputs = repo
            .load_player_inputs("session-db")
            .await
            .expect("player inputs should load");
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].round, 1);
        assert_eq!(inputs[0].action_type, PlayerActionType::SelectedOption);
        assert_eq!(inputs[0].action, "绕到钟楼背面");

        repo.record_flow_turn_error(&FlowTurnError {
            session_id: "session-db".to_string(),
            round: 2,
            stage: TurnPhase::Simulation,
            entity_name: "FateWeaver".to_string(),
            msg: "boom".to_string(),
        })
        .await
        .expect("flow error should save");
        let metadata = repo
            .load_session_metadata("session-db")
            .await
            .expect("metadata should load")
            .expect("metadata should exist");
        assert_eq!(metadata.phase, TurnPhase::Failed);
    }

    fn test_repo() -> SessionArchiveRepository {
        SessionArchiveRepository::new(AppDatabase::new(std::env::temp_dir().join(format!(
            "akasa-session-archives-{}.sqlite3",
            Uuid::new_v4().simple()
        ))))
    }

    fn round_entry(round: u64, narration: &str) -> RoundHistoryEntry {
        RoundHistoryEntry {
            round,
            narration_text: Some(narration.to_string()),
            ..RoundHistoryEntry::default()
        }
    }
}

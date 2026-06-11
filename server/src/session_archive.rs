use agent::agent::Context as AgentContext;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use story_engine::{
    components::{
        agent::AgentOutputType,
        outcome::{PlayerActionType, ProtagonistOptions},
        world_snapshot::WorldSnapshot,
    },
    resources::session_events::{
        AgentContextUpdate, FlowTurnError, FlowTurnUpdate, PlayerInput, SessionCreated,
    },
};

use crate::session_history::{RoundHistoryEntry, TurnPhase};

use crate::database::AppDatabase;

const CREATE_SESSIONS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    world_profile TEXT NOT NULL,
    protagonist_profile TEXT NOT NULL,
    key_story_beats TEXT NOT NULL,
    phase TEXT NOT NULL,
    turn_index INTEGER NOT NULL,
    active_turn_id INTEGER NOT NULL,
    flow_end INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_accessed_at TEXT NOT NULL
);
"#;

const CREATE_FLOW_TURNS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS flow_turns (
    session_id TEXT NOT NULL,
    round INTEGER NOT NULL,
    stage TEXT NOT NULL,
    entity_name TEXT NOT NULL,
    output_type TEXT NOT NULL,
    content TEXT NOT NULL,
    PRIMARY KEY (session_id, round, stage, entity_name, output_type)
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
pub struct StoredSessionMetadata {
    pub session_id: String,
    pub world_profile: String,
    pub protagonist_profile: String,
    pub key_story_beats: String,
    pub phase: TurnPhase,
    pub turn_index: u64,
    pub active_turn_id: u64,
    pub flow_end: bool,
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
                flow_end,
                created_at,
                updated_at,
                last_accessed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 0, ?6, ?6, ?6)
            ON CONFLICT(session_id) DO UPDATE SET
                world_profile = excluded.world_profile,
                protagonist_profile = excluded.protagonist_profile,
                key_story_beats = excluded.key_story_beats,
                phase = excluded.phase,
                turn_index = excluded.turn_index,
                active_turn_id = excluded.active_turn_id,
                flow_end = excluded.flow_end,
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
                flow_end,
                created_at,
                updated_at,
                last_accessed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?9)
            ON CONFLICT(session_id) DO UPDATE SET
                world_profile = excluded.world_profile,
                protagonist_profile = excluded.protagonist_profile,
                key_story_beats = excluded.key_story_beats,
                phase = excluded.phase,
                turn_index = excluded.turn_index,
                active_turn_id = excluded.active_turn_id,
                flow_end = excluded.flow_end,
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
                metadata.flow_end,
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
                active_turn_id,
                flow_end
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
                    flow_end: row.get(7)?,
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

    pub async fn replace_player_inputs_from_rounds(
        &self,
        session_id: &str,
        rounds: &[RoundHistoryEntry],
    ) -> Result<()> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }

        let _guard = self.db.lock().await;
        let mut conn = self.db.open_connection("player inputs")?;
        init_schema(&conn)?;
        let tx = conn
            .transaction()
            .context("failed to start player inputs replacement transaction")?;
        tx.execute(
            "DELETE FROM player_inputs WHERE session_id = ?1",
            params![session_id],
        )
        .context("failed to clear existing player inputs")?;

        for round in rounds {
            let Some(action) = round
                .committed_action
                .as_deref()
                .map(str::trim)
                .filter(|action| !action.is_empty())
            else {
                continue;
            };
            let round_index =
                i64::try_from(round.round).context("player input round exceeds SQLite range")?;
            let action_type = if round
                .choices
                .iter()
                .any(|choice| choice.option.action == action)
            {
                PlayerActionType::SelectedOption
            } else {
                PlayerActionType::FreeText
            };
            tx.execute(
                r#"
                INSERT INTO player_inputs (
                    session_id,
                    round,
                    action_type,
                    action,
                    created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
                params![
                    session_id,
                    round_index,
                    serialize_player_action_type(action_type)?,
                    action,
                    chrono::Utc::now().to_rfc3339(),
                ],
            )
            .context("failed to insert archived player input")?;
        }

        tx.commit()
            .context("failed to commit player inputs replacement")?;
        Ok(())
    }

    pub async fn record_flow_turn_completed(&self, session_id: &str, round: u64) -> Result<()> {
        self.update_session_turn_state(session_id, TurnPhase::TurnCompleted, round, round)
            .await
    }

    pub async fn record_flow_turn_end(&self, session_id: &str, round: u64) -> Result<()> {
        self.update_session_turn_state(session_id, TurnPhase::Ended, round, round)
            .await?;
        self.mark_session_flow_end(session_id).await
    }

    pub async fn record_flow_turn_error(&self, error: &FlowTurnError) -> Result<()> {
        self.update_session_turn_state(
            &error.session_id,
            TurnPhase::Failed,
            error.round,
            error.round,
        )
        .await
    }

    async fn mark_session_flow_end(&self, session_id: &str) -> Result<()> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }

        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("sessions")?;
        init_schema(&conn)?;
        conn.execute(
            r#"
            UPDATE sessions
            SET flow_end = 1,
                updated_at = ?2,
                last_accessed_at = ?2
            WHERE session_id = ?1
            "#,
            params![session_id, chrono::Utc::now().to_rfc3339()],
        )
        .context("failed to mark session flow end")?;
        Ok(())
    }

    pub async fn save_flow_turn_update(&self, update: &FlowTurnUpdate) -> Result<()> {
        let session_id = update.session_id.trim();
        let entity_name = update.entity_name.trim();
        if session_id.is_empty() || entity_name.is_empty() {
            return Ok(());
        }

        let round = i64::try_from(update.round).context("flow turn round exceeds SQLite range")?;
        let stage = serialize_phase(update.stage)?;
        let output_type = serialize_agent_output_type(update.output_type)?;
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("flow turns")?;
        init_schema(&conn)?;
        conn.execute(
            r#"
            INSERT INTO flow_turns (
                session_id,
                round,
                stage,
                entity_name,
                output_type,
                content
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(session_id, round, stage, entity_name, output_type) DO UPDATE SET
                content = excluded.content
            "#,
            params![
                session_id,
                round,
                stage,
                entity_name,
                output_type,
                update.content
            ],
        )
        .context("failed to upsert flow turn output")?;
        Ok(())
    }

    pub async fn save_rounds(&self, session_id: &str, rounds: &[RoundHistoryEntry]) -> Result<()> {
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

        for round in rounds {
            let round_index =
                i64::try_from(round.round).context("session round exceeds SQLite INTEGER range")?;
            tx.execute(
                "DELETE FROM flow_turns WHERE session_id = ?1 AND round = ?2",
                params![session_id, round_index],
            )
            .context("failed to clear existing flow turn outputs for round")?;
            for output in flow_outputs_from_round(session_id, round)? {
                tx.execute(
                    r#"
                    INSERT INTO flow_turns (
                        session_id,
                        round,
                        stage,
                        entity_name,
                        output_type,
                        content
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    "#,
                    params![
                        output.session_id,
                        output.round,
                        output.stage,
                        output.entity_name,
                        output.output_type,
                        output.content
                    ],
                )
                .context("failed to insert archived flow turn output")?;
            }
        }

        tx.commit()
            .context("failed to commit session rounds transaction")?;
        Ok(())
    }

    pub async fn load_rounds(&self, session_id: &str) -> Result<Vec<RoundHistoryEntry>> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("session rounds")?;
        init_schema(&conn)?;
        load_rounds_from_outputs(&conn, session_id)
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
        load_round_page_from_outputs(&conn, session_id, before_round, limit)
    }
}

#[derive(Debug)]
struct FlowOutputRow {
    session_id: String,
    round: i64,
    stage: String,
    entity_name: String,
    output_type: String,
    content: String,
}

fn flow_outputs_from_round(
    session_id: &str,
    round: &RoundHistoryEntry,
) -> Result<Vec<FlowOutputRow>> {
    let round_index =
        i64::try_from(round.round).context("session round exceeds SQLite INTEGER range")?;
    let mut outputs = Vec::new();

    if let Some(world_snapshot) = &round.world_snapshot {
        outputs.push(FlowOutputRow {
            session_id: session_id.to_string(),
            round: round_index,
            stage: serialize_phase(TurnPhase::Simulation)?,
            entity_name: "FateWeaver".to_string(),
            output_type: serialize_agent_output_type(AgentOutputType::Json)?,
            content: serde_json::to_string(world_snapshot)
                .context("failed to serialize world snapshot output")?,
        });
    }

    if let Some(narration) = round
        .narration_text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        outputs.push(FlowOutputRow {
            session_id: session_id.to_string(),
            round: round_index,
            stage: serialize_phase(TurnPhase::Application)?,
            entity_name: "UpperNarrator".to_string(),
            output_type: serialize_agent_output_type(AgentOutputType::Text)?,
            content: narration.to_string(),
        });
    }

    if !round.choices.is_empty() {
        let options = ProtagonistOptions {
            options: round
                .choices
                .iter()
                .map(|choice| choice.option.clone())
                .collect(),
        };
        outputs.push(FlowOutputRow {
            session_id: session_id.to_string(),
            round: round_index,
            stage: serialize_phase(TurnPhase::Application)?,
            entity_name: "Protagonist".to_string(),
            output_type: serialize_agent_output_type(AgentOutputType::Json)?,
            content: serde_json::to_string(&options)
                .context("failed to serialize protagonist options output")?,
        });
    }

    Ok(outputs)
}

fn load_rounds_from_outputs(conn: &Connection, session_id: &str) -> Result<Vec<RoundHistoryEntry>> {
    let round_numbers = select_round_numbers(conn, session_id, None, None)?;
    load_rounds_by_numbers(conn, session_id, &round_numbers)
}

fn load_round_page_from_outputs(
    conn: &Connection,
    session_id: &str,
    before_round: Option<u64>,
    limit: usize,
) -> Result<StoredSessionRoundPage> {
    let limit = limit.max(1);
    let fetch_limit = limit + 1;
    let mut round_numbers =
        select_round_numbers(conn, session_id, before_round, Some(fetch_limit))?;
    let has_more = round_numbers.len() > limit;
    if has_more {
        round_numbers.truncate(limit);
    }
    round_numbers.reverse();
    let rounds = load_rounds_by_numbers(conn, session_id, &round_numbers)?;
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

fn select_round_numbers(
    conn: &Connection,
    session_id: &str,
    before_round: Option<u64>,
    limit: Option<usize>,
) -> Result<Vec<u64>> {
    let before_round = before_round
        .map(i64::try_from)
        .transpose()
        .context("before round exceeds SQLite INTEGER range")?;
    let limit = limit
        .map(i64::try_from)
        .transpose()
        .context("session round page limit too large")?;

    let mut rounds = match (before_round, limit) {
        (Some(before_round), Some(limit)) => {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT DISTINCT round
                    FROM flow_turns
                    WHERE session_id = ?1 AND round < ?2
                    ORDER BY round DESC
                    LIMIT ?3
                    "#,
                )
                .context("failed to prepare older flow turn round query")?;
            stmt.query_map(params![session_id, before_round, limit], |row| {
                row.get::<_, i64>(0)
            })
            .context("failed to query older flow turn rounds")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read older flow turn rounds")?
        }
        (Some(before_round), None) => {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT DISTINCT round
                    FROM flow_turns
                    WHERE session_id = ?1 AND round < ?2
                    ORDER BY round DESC
                    "#,
                )
                .context("failed to prepare bounded flow turn round query")?;
            stmt.query_map(params![session_id, before_round], |row| {
                row.get::<_, i64>(0)
            })
            .context("failed to query bounded flow turn rounds")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read bounded flow turn rounds")?
        }
        (None, Some(limit)) => {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT DISTINCT round
                    FROM flow_turns
                    WHERE session_id = ?1
                    ORDER BY round DESC
                    LIMIT ?2
                    "#,
                )
                .context("failed to prepare latest flow turn round query")?;
            stmt.query_map(params![session_id, limit], |row| row.get::<_, i64>(0))
                .context("failed to query latest flow turn rounds")?
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("failed to read latest flow turn rounds")?
        }
        (None, None) => {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT DISTINCT round
                    FROM flow_turns
                    WHERE session_id = ?1
                    ORDER BY round ASC
                    "#,
                )
                .context("failed to prepare flow turn round query")?;
            stmt.query_map(params![session_id], |row| row.get::<_, i64>(0))
                .context("failed to query flow turn rounds")?
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("failed to read flow turn rounds")?
        }
    };

    if before_round.is_some() || limit.is_some() {
        rounds.retain(|round| *round >= 0);
    }

    rounds
        .into_iter()
        .map(|round| round.try_into().context("flow turn round is negative"))
        .collect()
}

fn load_rounds_by_numbers(
    conn: &Connection,
    session_id: &str,
    round_numbers: &[u64],
) -> Result<Vec<RoundHistoryEntry>> {
    round_numbers
        .iter()
        .map(|round| load_round_by_number(conn, session_id, *round))
        .collect()
}

fn load_round_by_number(
    conn: &Connection,
    session_id: &str,
    round: u64,
) -> Result<RoundHistoryEntry> {
    let round_index = i64::try_from(round).context("flow turn round exceeds SQLite range")?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT stage, entity_name, output_type, content
            FROM flow_turns
            WHERE session_id = ?1 AND round = ?2
            ORDER BY
                CASE stage
                    WHEN 'simulation' THEN 0
                    WHEN 'application' THEN 1
                    ELSE 2
                END,
                entity_name ASC,
                output_type ASC
            "#,
        )
        .context("failed to prepare flow turn output query")?;
    let rows = stmt
        .query_map(params![session_id, round_index], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .context("failed to query flow turn outputs")?;
    let mut entry = RoundHistoryEntry {
        round,
        ..RoundHistoryEntry::default()
    };
    for row in rows {
        let (stage, _entity_name, output_type, content) =
            row.context("failed to read flow turn output")?;
        let stage = deserialize_phase(&stage).map_err(invalid_flow_turn_value)?;
        let output_type =
            deserialize_agent_output_type(&output_type).map_err(invalid_flow_turn_value)?;
        apply_flow_turn_output(&mut entry, stage, output_type, &content)?;
    }

    Ok(entry)
}

fn apply_flow_turn_output(
    entry: &mut RoundHistoryEntry,
    stage: TurnPhase,
    output_type: AgentOutputType,
    content: &str,
) -> Result<()> {
    match (stage, output_type) {
        (TurnPhase::Simulation, AgentOutputType::Json) => {
            entry.world_snapshot = Some(
                serde_json::from_str::<WorldSnapshot>(content)
                    .context("failed to deserialize world snapshot flow output")?,
            );
        }
        (TurnPhase::Application, AgentOutputType::Text) => {
            entry.narration_text = Some(content.to_string());
        }
        (TurnPhase::Application, AgentOutputType::Json) => {
            let options = serde_json::from_str::<ProtagonistOptions>(content)
                .context("failed to deserialize protagonist options flow output")?;
            entry.choices = pending_choices_from_options(options);
        }
        _ => {}
    }
    Ok(())
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(CREATE_SESSIONS_TABLE_SQL)
        .context("failed to initialize sessions schema")?;
    conn.execute_batch(CREATE_FLOW_TURNS_TABLE_SQL)
        .context("failed to initialize flow turns schema")?;
    conn.execute_batch(CREATE_AGENT_CONTEXTS_TABLE_SQL)
        .context("failed to initialize agent contexts schema")?;
    conn.execute_batch(CREATE_PLAYER_INPUTS_TABLE_SQL)
        .context("failed to initialize player inputs schema")
}

fn serialize_phase(phase: TurnPhase) -> Result<String> {
    serde_json::to_string(&phase)
        .map(|value| value.trim_matches('"').to_string())
        .context("failed to serialize turn phase")
}

fn deserialize_phase(value: &str) -> std::result::Result<TurnPhase, String> {
    serde_json::from_str(&format!("{value:?}")).map_err(|err| err.to_string())
}

fn serialize_agent_output_type(output_type: AgentOutputType) -> Result<String> {
    serde_json::to_string(&output_type)
        .map(|value| value.trim_matches('"').to_string())
        .context("failed to serialize agent output type")
}

fn deserialize_agent_output_type(value: &str) -> std::result::Result<AgentOutputType, String> {
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

fn pending_choices_from_options(
    options: ProtagonistOptions,
) -> Vec<story_engine::components::outcome::PendingProtagonistChoice> {
    options
        .options
        .into_iter()
        .enumerate()
        .map(
            |(index, option)| story_engine::components::outcome::PendingProtagonistChoice {
                id: format!("choice-{}", index + 1),
                option,
            },
        )
        .collect()
}

fn invalid_flow_turn_value(error: String) -> anyhow::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error).into()
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
    async fn shared_database_stores_analytics_and_session_metadata() {
        let db_path = std::env::temp_dir().join(format!(
            "akasa-shared-db-{}.sqlite3",
            Uuid::new_v4().simple()
        ));
        let db = AppDatabase::new(db_path.clone());
        let analytics = AnalyticsRepository::new(db.clone());
        let sessions = SessionArchiveRepository::new(db);

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
        sessions
            .save_session_created(&SessionCreated {
                session_id: "session-shared".to_string(),
                world_profile: "world".to_string(),
                protagonist_profile: "hero".to_string(),
                key_story_beats: "beats".to_string(),
            })
            .await
            .expect("session metadata should save");

        let summary = analytics
            .summary(24 * 30)
            .await
            .expect("summary should read");
        let metadata = sessions
            .load_session_metadata("session-shared")
            .await
            .expect("session metadata should load")
            .expect("session metadata should exist");
        let conn = Connection::open(db_path).expect("sqlite db should open");
        let deleted_table_count: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM sqlite_master
                WHERE type = 'table'
                    AND name IN ('session_rounds', 'game_session_archives')
                "#,
                [],
                |row| row.get(0),
            )
            .expect("schema should be queryable");

        assert_eq!(summary.totals.events, 1);
        assert_eq!(metadata.world_profile, "world");
        assert_eq!(deleted_table_count, 0);
    }

    #[tokio::test]
    async fn flow_turns_upsert_and_page_with_before_cursor() {
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
    async fn flow_turn_updates_store_entity_outputs() {
        let repo = test_repo();
        let snapshot = WorldSnapshot {
            round: 7,
            scene_title: "钟楼阴影".to_string(),
            ..WorldSnapshot::default()
        };

        repo.save_flow_turn_update(&FlowTurnUpdate {
            session_id: "session-flow-rows".to_string(),
            round: 7,
            stage: TurnPhase::Simulation,
            entity_name: "FateWeaver".to_string(),
            output_type: AgentOutputType::Json,
            content: serde_json::to_string(&snapshot).expect("snapshot should serialize"),
        })
        .await
        .expect("world output should save");
        repo.save_flow_turn_update(&FlowTurnUpdate {
            session_id: "session-flow-rows".to_string(),
            round: 7,
            stage: TurnPhase::Application,
            entity_name: "UpperNarrator".to_string(),
            output_type: AgentOutputType::Text,
            content: "钟声掠过雾墙。".to_string(),
        })
        .await
        .expect("narration output should save");

        let rounds = repo
            .load_rounds("session-flow-rows")
            .await
            .expect("flow outputs should load");

        assert_eq!(rounds.len(), 1);
        assert_eq!(rounds[0].round, 7);
        assert_eq!(
            rounds[0]
                .world_snapshot
                .as_ref()
                .map(|snapshot| snapshot.scene_title.as_str()),
            Some("钟楼阴影")
        );
        assert_eq!(rounds[0].narration_text.as_deref(), Some("钟声掠过雾墙。"));
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
        assert!(!metadata.flow_end);

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
        assert!(!metadata.flow_end);

        repo.record_flow_turn_end("session-db", 1)
            .await
            .expect("flow end should save");
        let metadata = repo
            .load_session_metadata("session-db")
            .await
            .expect("metadata should load")
            .expect("metadata should exist");
        assert_eq!(metadata.phase, TurnPhase::Ended);
        assert!(metadata.flow_end);

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

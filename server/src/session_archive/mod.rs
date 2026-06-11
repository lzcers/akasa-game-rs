use agent::{agent::Context as AgentContext, core::Message};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use story_engine::{
    components::{
        agent::AgentOutputType,
        outcome::{CharacterOption, CharacterOptions, PlayerActionItem, PlayerActionType},
        world_snapshot::WorldSnapshot,
    },
    resources::session_events::{
        AgentContextItemAppended, AgentContextRollback, AgentContextRollbackPolicy, FlowTurnError,
        FlowTurnUpdate, PlayerInput, SessionCreated,
    },
};

use crate::session_history::{RoundHistoryEntry, TurnPhase};

use crate::database::AppDatabase;

mod schema;

#[derive(Debug, Clone)]
pub struct SessionArchiveRepository {
    db: AppDatabase,
}

#[derive(Debug, Clone)]
pub struct StoredSessionMetadata {
    pub session_id: String,
    pub character_name: String,
    pub world_profile: String,
    pub character_profile: String,
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
pub struct StoredStoryEdgeAction {
    pub round: u64,
    pub action: PlayerActionItem,
}

#[derive(Debug, Clone)]
pub struct StoredSessionRoundPage {
    pub rounds: Vec<RoundHistoryEntry>,
    pub next_before_round: Option<u64>,
    pub has_more: bool,
}

#[derive(Debug, Clone)]
struct StoryPathNode {
    node_id: String,
    node_depth: u64,
}

const ROOT_NODE_ID: &str = "start";
const DEFAULT_PLAYER_CHARACTER_NAME: &str = "玩家角色";

fn normalized_character_name(character_name: &str) -> String {
    let character_name = character_name.trim();
    if character_name.is_empty() {
        DEFAULT_PLAYER_CHARACTER_NAME.to_string()
    } else {
        character_name.to_string()
    }
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
        schema::init(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        upsert_session_base(
            &conn,
            SessionBaseRecord {
                session_id,
                character_name: &event.character_name,
                world_profile: &event.world_profile,
                character_profile: &event.character_profile,
                key_story_beats: &event.key_story_beats,
                active_node_id: ROOT_NODE_ID,
                total_node_count: 0,
            },
            &now,
        )?;
        ensure_linear_story_path(&conn, session_id, 0, &now)?;
        update_story_node_state(
            &conn,
            session_id,
            ROOT_NODE_ID,
            TurnPhase::Start,
            Some(false),
            &now,
        )?;
        Ok(())
    }

    pub async fn save_session_metadata(&self, metadata: &StoredSessionMetadata) -> Result<()> {
        let session_id = metadata.session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }

        let active_node_depth = metadata.active_turn_id.max(metadata.turn_index);
        let active_node_id = linear_node_id_for_depth(active_node_depth);
        let total_node_count = i64::try_from(active_node_depth)
            .context("total node count exceeds SQLite INTEGER range")?;
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("sessions")?;
        schema::init(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        upsert_session_base(
            &conn,
            SessionBaseRecord {
                session_id,
                character_name: &metadata.character_name,
                world_profile: &metadata.world_profile,
                character_profile: &metadata.character_profile,
                key_story_beats: &metadata.key_story_beats,
                active_node_id: &active_node_id,
                total_node_count,
            },
            &now,
        )?;
        ensure_linear_story_path(&conn, session_id, active_node_depth, &now)?;
        update_story_node_state(
            &conn,
            session_id,
            &active_node_id,
            metadata.phase,
            Some(metadata.flow_end),
            &now,
        )?;
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

        let active_node_depth = active_turn_id.max(turn_index);
        let active_node_id = linear_node_id_for_depth(active_node_depth);
        let total_node_count =
            i64::try_from(active_node_depth).context("total node count exceeds SQLite range")?;
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("sessions")?;
        schema::init(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        ensure_linear_story_path(&conn, session_id, active_node_depth, &now)?;
        update_story_node_state(&conn, session_id, &active_node_id, phase, None, &now)?;
        conn.execute(
            r#"
            UPDATE sessions
            SET active_node_id = ?2,
                total_node_count = MAX(total_node_count, ?3),
                updated_at = ?4,
                last_accessed_at = ?4
            WHERE session_id = ?1
            "#,
            params![session_id, active_node_id, total_node_count, now],
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
        schema::init(&conn)?;
        conn.query_row(
            r#"
            SELECT
                s.session_id,
                c.character_name,
                w.world_profile,
                c.character_profile,
                COALESCE(c.key_story_beats, w.global_key_story_beats),
                n.phase,
                n.node_depth,
                n.flow_end
            FROM sessions s
            JOIN session_worlds w
                ON w.session_id = s.session_id
            JOIN session_characters c
                ON c.session_id = s.session_id
                AND c.is_playable = 1
            JOIN story_nodes n
                ON n.session_id = s.session_id
                AND n.node_id = s.active_node_id
            WHERE s.session_id = ?1
            ORDER BY c.created_at ASC, c.character_name ASC
            LIMIT 1
            "#,
            params![session_id],
            |row| {
                let phase: String = row.get(5)?;
                let phase = deserialize_phase(&phase).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                    )
                })?;
                let node_depth: u64 = row.get::<_, i64>(6)?.try_into().unwrap_or_default();
                let (turn_index, active_turn_id) = turn_state_from_active_node(phase, node_depth);
                Ok(StoredSessionMetadata {
                    session_id: row.get(0)?,
                    character_name: row.get(1)?,
                    world_profile: row.get(2)?,
                    character_profile: row.get(3)?,
                    key_story_beats: row.get(4)?,
                    phase,
                    turn_index,
                    active_turn_id,
                    flow_end: row.get(7)?,
                })
            },
        )
        .optional()
        .context("failed to load session metadata")
    }

    pub async fn save_agent_context_item(&self, update: &AgentContextItemAppended) -> Result<()> {
        let session_id = update.session_id.trim();
        let agent_name = update.agent_name.trim();
        if session_id.is_empty() || agent_name.is_empty() {
            return Ok(());
        }

        let node_id = linear_node_id_for_depth(update.round);
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("agent context items")?;
        schema::init(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        ensure_linear_story_path(&conn, session_id, update.round, &now)?;
        insert_agent_context_message(
            &conn,
            session_id,
            &node_id,
            agent_name,
            &update.message,
            &now,
        )?;
        Ok(())
    }

    pub async fn save_agent_context_rollback(&self, rollback: &AgentContextRollback) -> Result<()> {
        let session_id = rollback.session_id.trim();
        let agent_name = rollback.agent_name.trim();
        if session_id.is_empty() || agent_name.is_empty() {
            return Ok(());
        }

        let node_id = linear_node_id_for_depth(rollback.round);
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("agent context rollbacks")?;
        schema::init(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        ensure_linear_story_path(&conn, session_id, rollback.round, &now)?;
        match rollback.policy {
            AgentContextRollbackPolicy::LatestInput => {
                insert_agent_context_rollback(&conn, session_id, &node_id, agent_name, &now)?;
            }
        }
        Ok(())
    }

    pub async fn replace_agent_contexts_from_contexts(
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
        let mut conn = self.db.open_connection("agent context replacement")?;
        schema::init(&conn)?;
        let tx = conn
            .transaction()
            .context("failed to start agent context replacement transaction")?;
        let now = chrono::Utc::now().to_rfc3339();
        ensure_linear_story_path(&tx, session_id, round, &now)?;
        tx.execute(
            "DELETE FROM agent_context_items WHERE session_id = ?1",
            params![session_id],
        )
        .context("failed to clear existing agent context items")?;
        for (agent_name, context) in contexts {
            for message in context_messages_for_storage(context) {
                insert_agent_context_message(
                    &tx, session_id, &node_id, agent_name, &message, &now,
                )?;
            }
        }
        tx.commit()
            .context("failed to commit agent context replacement")?;
        Ok(())
    }

    pub async fn load_agent_contexts(&self, session_id: &str) -> Result<Vec<StoredAgentContext>> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("agent context items")?;
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
                    item.agent_name,
                    item.item_kind,
                    item.message_role,
                    item.content
                FROM agent_context_items item
                JOIN active_path
                    ON active_path.node_id = item.node_id
                WHERE item.session_id = ?1
                ORDER BY
                    item.agent_name ASC,
                    active_path.node_depth ASC,
                    item.item_index ASC,
                    item.id ASC
                "#,
            )
            .context("failed to prepare agent context load")?;
        let mut rows = stmt
            .query(params![session_id])
            .context("failed to query agent context items")?;
        let mut contexts = std::collections::BTreeMap::<String, AgentContext>::new();
        while let Some(row) = rows
            .next()
            .context("failed to read agent context item row")?
        {
            let agent_name: String = row.get(0)?;
            let item_kind: String = row.get(1)?;
            let context = contexts.entry(agent_name).or_default();
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
            .map(|(agent_name, context)| StoredAgentContext {
                agent_name,
                context,
            })
            .collect())
    }

    pub async fn save_player_input(&self, input: &PlayerInput) -> Result<()> {
        let session_id = input.session_id.trim();
        let actions = input
            .actions
            .iter()
            .cloned()
            .map(PlayerActionItem::normalized)
            .filter(|action| !action.action.is_empty())
            .collect::<Vec<_>>();
        if session_id.is_empty() || actions.is_empty() {
            return Ok(());
        }

        let from_node_id = linear_node_id_for_depth(input.round);
        let to_node_id = linear_node_id_for_depth(input.round + 1);
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("story edges")?;
        schema::init(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        ensure_linear_story_path(&conn, session_id, input.round + 1, &now)?;
        conn.execute(
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
            params![session_id, from_node_id, to_node_id, now,],
        )
        .context("failed to upsert story edge")?;
        for action in &actions {
            let choice_option =
                choice_option_for_story_edge(&conn, session_id, input.round, action)?;
            upsert_story_edge_action(
                &conn,
                StoryEdgeActionRecord {
                    session_id,
                    from_node_id: &from_node_id,
                    to_node_id: &to_node_id,
                    action,
                    choice_option: &choice_option,
                    submitted_at: &now,
                },
            )?;
        }
        Ok(())
    }

    pub async fn load_story_edge_actions(
        &self,
        session_id: &str,
    ) -> Result<Vec<StoredStoryEdgeAction>> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("story edges")?;
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
                    active_path.node_depth,
                    action.character_name,
                    action.player_id,
                    action.action_type,
                    action.title,
                    action.action,
                    action.motivation_and_risk
                FROM story_edge_actions action
                JOIN story_edges e
                    ON e.session_id = action.session_id
                    AND e.from_node_id = action.from_node_id
                    AND e.to_node_id = action.to_node_id
                JOIN active_path
                    ON active_path.node_id = e.from_node_id
                WHERE action.session_id = ?1
                    AND active_path.node_depth > 0
                ORDER BY active_path.node_depth ASC, action.character_name ASC
                "#,
            )
            .context("failed to prepare story edge action load")?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                let action_type: String = row.get(3)?;
                Ok(StoredStoryEdgeAction {
                    round: row.get::<_, i64>(0)?.try_into().unwrap_or_default(),
                    action: PlayerActionItem {
                        character_name: row.get(1)?,
                        player_id: row.get(2)?,
                        action_type: deserialize_player_action_type(&action_type).map_err(
                            |err| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    3,
                                    rusqlite::types::Type::Text,
                                    Box::new(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        err,
                                    )),
                                )
                            },
                        )?,
                        title: row.get(4)?,
                        action: row.get(5)?,
                        motivation_and_risk: row.get(6)?,
                    },
                })
            })
            .context("failed to query story edge actions")?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read story edge actions")
    }

    pub async fn replace_story_edges_from_rounds(
        &self,
        session_id: &str,
        rounds: &[RoundHistoryEntry],
    ) -> Result<()> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }

        let _guard = self.db.lock().await;
        let mut conn = self.db.open_connection("story edges")?;
        schema::init(&conn)?;
        let tx = conn
            .transaction()
            .context("failed to start story edges replacement transaction")?;
        tx.execute(
            "DELETE FROM story_edge_actions WHERE session_id = ?1",
            params![session_id],
        )
        .context("failed to clear existing story edge actions")?;
        tx.execute(
            "DELETE FROM story_edges WHERE session_id = ?1",
            params![session_id],
        )
        .context("failed to clear existing story edges")?;

        let now = chrono::Utc::now().to_rfc3339();
        for round in rounds {
            let actions = round
                .committed_actions
                .iter()
                .cloned()
                .map(PlayerActionItem::normalized)
                .filter(|action| !action.action.is_empty())
                .collect::<Vec<_>>();
            if actions.is_empty() {
                continue;
            }
            ensure_linear_story_path(&tx, session_id, round.round + 1, &now)?;
            let from_node_id = linear_node_id_for_depth(round.round);
            let to_node_id = linear_node_id_for_depth(round.round + 1);
            tx.execute(
                r#"
                INSERT INTO story_edges (
                    session_id,
                    from_node_id,
                    to_node_id,
                    created_at
                ) VALUES (?1, ?2, ?3, ?4)
                "#,
                params![session_id, from_node_id, to_node_id, now,],
            )
            .context("failed to insert archived story edge")?;
            for action in &actions {
                let choice_option = choice_option_from_round_action(round, action);
                upsert_story_edge_action(
                    &tx,
                    StoryEdgeActionRecord {
                        session_id,
                        from_node_id: &from_node_id,
                        to_node_id: &to_node_id,
                        action,
                        choice_option: &choice_option,
                        submitted_at: &now,
                    },
                )?;
            }
        }

        tx.commit()
            .context("failed to commit story edges replacement")?;
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
        schema::init(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            r#"
            UPDATE story_nodes
            SET flow_end = 1,
                updated_at = ?2,
                last_accessed_at = ?2
            WHERE session_id = ?1
                AND node_id = (
                    SELECT active_node_id
                    FROM sessions
                    WHERE session_id = ?1
                )
            "#,
            params![session_id, now],
        )
        .context("failed to mark session flow end")?;
        conn.execute(
            r#"
            UPDATE sessions
            SET updated_at = ?2,
                last_accessed_at = ?2
            WHERE session_id = ?1
            "#,
            params![session_id, now],
        )
        .context("failed to touch session after flow end")?;
        Ok(())
    }

    pub async fn save_flow_turn_update(&self, update: &FlowTurnUpdate) -> Result<()> {
        let session_id = update.session_id.trim();
        let entity_name = update.entity_name.trim();
        if session_id.is_empty() || entity_name.is_empty() {
            return Ok(());
        }

        let node_id = linear_node_id_for_depth(update.round);
        let stage = serialize_phase(update.stage)?;
        let output_type = serialize_agent_output_type(update.output_type)?;
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("flow outputs")?;
        schema::init(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        ensure_linear_story_path(&conn, session_id, update.round, &now)?;
        conn.execute(
            r#"
            INSERT INTO flow_outputs (
                session_id,
                node_id,
                stage,
                entity_name,
                output_type,
                content,
                created_at,
                updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
            ON CONFLICT(session_id, node_id, stage, entity_name, output_type) DO UPDATE SET
                content = excluded.content,
                updated_at = excluded.updated_at
            "#,
            params![
                session_id,
                node_id,
                stage,
                entity_name,
                output_type,
                update.content,
                now,
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
        let mut conn = self.db.open_connection("story nodes")?;
        schema::init(&conn)?;
        let tx = conn
            .transaction()
            .context("failed to start session rounds transaction")?;

        for round in rounds {
            let now = chrono::Utc::now().to_rfc3339();
            ensure_linear_story_path(&tx, session_id, round.round, &now)?;
            let node_id = linear_node_id_for_depth(round.round);
            tx.execute(
                "DELETE FROM flow_outputs WHERE session_id = ?1 AND node_id = ?2",
                params![session_id, node_id],
            )
            .context("failed to clear existing flow turn outputs for round")?;
            for output in flow_outputs_from_round(session_id, round)? {
                tx.execute(
                    r#"
                    INSERT INTO flow_outputs (
                        session_id,
                        node_id,
                        stage,
                        entity_name,
                        output_type,
                        content,
                        created_at,
                        updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                    "#,
                    params![
                        output.session_id,
                        output.node_id,
                        output.stage,
                        output.entity_name,
                        output.output_type,
                        output.content,
                        now,
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
        schema::init(&conn)?;
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
        schema::init(&conn)?;
        load_round_page_from_outputs(&conn, session_id, before_round, limit)
    }
}

struct SessionBaseRecord<'a> {
    session_id: &'a str,
    character_name: &'a str,
    world_profile: &'a str,
    character_profile: &'a str,
    key_story_beats: &'a str,
    active_node_id: &'a str,
    total_node_count: i64,
}

struct StoryNodeSeed<'a> {
    session_id: &'a str,
    node_id: &'a str,
    parent_node_id: Option<&'a str>,
    node_depth: u64,
    phase: TurnPhase,
    flow_end: bool,
}

fn upsert_session_base(conn: &Connection, record: SessionBaseRecord<'_>, now: &str) -> Result<()> {
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

fn ensure_linear_story_path(
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

fn update_story_node_state(
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

fn linear_node_id_for_depth(depth: u64) -> String {
    if depth == 0 {
        ROOT_NODE_ID.to_string()
    } else {
        format!("node-{depth}")
    }
}

fn turn_state_from_active_node(phase: TurnPhase, node_depth: u64) -> (u64, u64) {
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

fn choice_option_for_story_edge(
    conn: &Connection,
    session_id: &str,
    round: u64,
    action: &PlayerActionItem,
) -> Result<CharacterOption> {
    if action.action_type == PlayerActionType::SelectedOption
        && let Some(choice) =
            selected_choice_option_for_action(conn, session_id, round, &action.action)?
    {
        return Ok(choice);
    }

    Ok(choice_option_from_action(action))
}

fn selected_choice_option_for_action(
    conn: &Connection,
    session_id: &str,
    round: u64,
    action: &str,
) -> Result<Option<CharacterOption>> {
    let node_id = linear_node_id_for_depth(round);
    let stage = serialize_phase(TurnPhase::Application)?;
    let output_type = serialize_agent_output_type(AgentOutputType::Json)?;
    let content = conn
        .query_row(
            r#"
            SELECT content
            FROM flow_outputs
            WHERE session_id = ?1
                AND node_id = ?2
                AND stage = ?3
                AND output_type = ?4
            ORDER BY entity_name ASC
            LIMIT 1
            "#,
            params![session_id, node_id, stage, output_type],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .context("failed to load character options for story edge")?;

    let Some(content) = content else {
        return Ok(None);
    };
    let options = serde_json::from_str::<CharacterOptions>(&content)
        .context("failed to deserialize character options for story edge")?;
    Ok(options
        .options
        .into_iter()
        .find(|option| option.action == action))
}

fn choice_option_from_action(action: &PlayerActionItem) -> CharacterOption {
    CharacterOption {
        title: action.title.clone(),
        action: action.action.clone(),
        motivation_and_risk: action.motivation_and_risk.clone(),
    }
}

fn choice_option_from_round_action(
    round: &RoundHistoryEntry,
    action: &PlayerActionItem,
) -> CharacterOption {
    if action.action_type == PlayerActionType::SelectedOption
        && let Some(choice) = round
            .choices
            .iter()
            .find(|choice| choice.option.action == action.action)
    {
        return choice.option.clone();
    }
    choice_option_from_action(action)
}

struct StoryEdgeActionRecord<'a> {
    session_id: &'a str,
    from_node_id: &'a str,
    to_node_id: &'a str,
    action: &'a PlayerActionItem,
    choice_option: &'a CharacterOption,
    submitted_at: &'a str,
}

fn upsert_story_edge_action(conn: &Connection, record: StoryEdgeActionRecord<'_>) -> Result<()> {
    conn.execute(
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
            record.session_id,
            record.from_node_id,
            record.to_node_id,
            record.action.character_name.as_str(),
            record.action.player_id.as_deref(),
            serialize_player_action_type(record.action.action_type)?,
            record.choice_option.title.as_str(),
            record.choice_option.action.as_str(),
            record.choice_option.motivation_and_risk.as_str(),
            record.submitted_at,
        ],
    )
    .context("failed to upsert story edge action")?;
    Ok(())
}

#[derive(Debug)]
struct FlowOutputRow {
    session_id: String,
    node_id: String,
    stage: String,
    entity_name: String,
    output_type: String,
    content: String,
}

fn flow_outputs_from_round(
    session_id: &str,
    round: &RoundHistoryEntry,
) -> Result<Vec<FlowOutputRow>> {
    let node_id = linear_node_id_for_depth(round.round);
    let mut outputs = Vec::new();

    if let Some(world_snapshot) = &round.world_snapshot {
        outputs.push(FlowOutputRow {
            session_id: session_id.to_string(),
            node_id: node_id.clone(),
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
            node_id: node_id.clone(),
            stage: serialize_phase(TurnPhase::Application)?,
            entity_name: "UpperNarrator".to_string(),
            output_type: serialize_agent_output_type(AgentOutputType::Text)?,
            content: narration.to_string(),
        });
    }

    if !round.choices.is_empty() {
        let options = CharacterOptions {
            options: round
                .choices
                .iter()
                .map(|choice| choice.option.clone())
                .collect(),
        };
        outputs.push(FlowOutputRow {
            session_id: session_id.to_string(),
            node_id,
            stage: serialize_phase(TurnPhase::Application)?,
            entity_name: DEFAULT_PLAYER_CHARACTER_NAME.to_string(),
            output_type: serialize_agent_output_type(AgentOutputType::Json)?,
            content: serde_json::to_string(&options)
                .context("failed to serialize character options output")?,
        });
    }

    Ok(outputs)
}

fn load_rounds_from_outputs(conn: &Connection, session_id: &str) -> Result<Vec<RoundHistoryEntry>> {
    let mut path_nodes = select_story_path_nodes(conn, session_id, None, None)?;
    path_nodes.reverse();
    load_rounds_by_nodes(conn, session_id, &path_nodes)
}

fn load_round_page_from_outputs(
    conn: &Connection,
    session_id: &str,
    before_round: Option<u64>,
    limit: usize,
) -> Result<StoredSessionRoundPage> {
    let limit = limit.max(1);
    let fetch_limit = limit + 1;
    let mut path_nodes =
        select_story_path_nodes(conn, session_id, before_round, Some(fetch_limit))?;
    let has_more = path_nodes.len() > limit;
    if has_more {
        path_nodes.truncate(limit);
    }
    path_nodes.reverse();
    let rounds = load_rounds_by_nodes(conn, session_id, &path_nodes)?;
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

fn select_story_path_nodes(
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
                    FROM flow_outputs output
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
                    FROM flow_outputs output
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

fn load_rounds_by_nodes(
    conn: &Connection,
    session_id: &str,
    path_nodes: &[StoryPathNode],
) -> Result<Vec<RoundHistoryEntry>> {
    path_nodes
        .iter()
        .map(|node| load_round_by_node(conn, session_id, node))
        .collect()
}

fn load_round_by_node(
    conn: &Connection,
    session_id: &str,
    path_node: &StoryPathNode,
) -> Result<RoundHistoryEntry> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT stage, entity_name, output_type, content
            FROM flow_outputs
            WHERE session_id = ?1 AND node_id = ?2
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
        .query_map(params![session_id, path_node.node_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .context("failed to query flow turn outputs")?;
    let mut entry = RoundHistoryEntry {
        round: path_node.node_depth,
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
            let options = serde_json::from_str::<CharacterOptions>(content)
                .context("failed to deserialize character options flow output")?;
            entry.choices = pending_choices_from_options(options);
        }
        _ => {}
    }
    Ok(())
}

fn insert_agent_context_message(
    conn: &Connection,
    session_id: &str,
    node_id: &str,
    agent_name: &str,
    message: &Message,
    now: &str,
) -> Result<()> {
    let item_index = next_agent_context_item_index(conn, session_id, node_id, agent_name)?;
    conn.execute(
        r#"
        INSERT INTO agent_context_items (
            session_id,
            node_id,
            agent_name,
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
            agent_name,
            item_index,
            message_role(message),
            message.content(),
            now,
        ],
    )
    .context("failed to insert agent context message")?;
    Ok(())
}

fn insert_agent_context_rollback(
    conn: &Connection,
    session_id: &str,
    node_id: &str,
    agent_name: &str,
    now: &str,
) -> Result<()> {
    let item_index = next_agent_context_item_index(conn, session_id, node_id, agent_name)?;
    conn.execute(
        r#"
        INSERT INTO agent_context_items (
            session_id,
            node_id,
            agent_name,
            item_index,
            item_kind,
            created_at
        ) VALUES (?1, ?2, ?3, ?4, 'rollback_latest_input', ?5)
        "#,
        params![session_id, node_id, agent_name, item_index, now],
    )
    .context("failed to insert agent context rollback")?;
    Ok(())
}

fn next_agent_context_item_index(
    conn: &Connection,
    session_id: &str,
    node_id: &str,
    agent_name: &str,
) -> Result<i64> {
    conn.query_row(
        r#"
        SELECT COALESCE(MAX(item_index), 0) + 1
        FROM agent_context_items
        WHERE session_id = ?1
            AND node_id = ?2
            AND agent_name = ?3
        "#,
        params![session_id, node_id, agent_name],
        |row| row.get(0),
    )
    .context("failed to compute next agent context item index")
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
        other => Err(format!("unknown agent context message role: {other}")),
    }
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
    options: CharacterOptions,
) -> Vec<story_engine::components::outcome::PendingCharacterChoice> {
    options
        .options
        .into_iter()
        .enumerate()
        .map(
            |(index, option)| story_engine::components::outcome::PendingCharacterChoice {
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
    use story_engine::components::outcome::PendingCharacterChoice;
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
            .append_events(
                &[AnalyticsEventInput {
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
                }],
                None,
            )
            .await
            .expect("analytics event should save");
        sessions
            .save_session_created(&SessionCreated {
                session_id: "session-shared".to_string(),
                character_name: "hero".to_string(),
                world_profile: "world".to_string(),
                character_profile: "hero".to_string(),
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
        let removed_table_count: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM sqlite_master
                WHERE type = 'table'
                    AND name IN (
                        'session_rounds',
                        'game_session_archives',
                        'flow_turns',
                        'player_inputs',
                        'agent_contexts'
                    )
                "#,
                [],
                |row| row.get(0),
            )
            .expect("schema should be queryable");
        let story_graph_table_count: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM sqlite_master
                WHERE type = 'table'
                    AND name IN (
                        'sessions',
                        'session_worlds',
                        'session_characters',
                        'story_nodes',
                        'story_edges',
                        'story_edge_actions',
                        'flow_outputs',
                        'agent_context_items'
                    )
                "#,
                [],
                |row| row.get(0),
            )
            .expect("story graph schema should be queryable");
        let story_edge_columns = conn
            .prepare("PRAGMA table_info(story_edges)")
            .expect("story_edges columns should be inspectable")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("story_edges columns should query")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("story_edges columns should read");
        let session_columns = conn
            .prepare("PRAGMA table_info(sessions)")
            .expect("sessions columns should be inspectable")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("sessions columns should query")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("sessions columns should read");
        let session_world_columns = conn
            .prepare("PRAGMA table_info(session_worlds)")
            .expect("session_worlds columns should be inspectable")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("session_worlds columns should query")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("session_worlds columns should read");
        let session_character_columns = conn
            .prepare("PRAGMA table_info(session_characters)")
            .expect("session_characters columns should be inspectable")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("session_characters columns should query")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("session_characters columns should read");
        let story_edge_action_columns = conn
            .prepare("PRAGMA table_info(story_edge_actions)")
            .expect("story_edge_actions columns should be inspectable")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("story_edge_actions columns should query")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("story_edge_actions columns should read");
        let agent_context_item_columns = conn
            .prepare("PRAGMA table_info(agent_context_items)")
            .expect("agent_context_items columns should be inspectable")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("agent_context_items columns should query")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("agent_context_items columns should read");

        assert_eq!(summary.totals.events, 1);
        assert_eq!(metadata.world_profile, "world");
        assert_eq!(removed_table_count, 0);
        assert_eq!(story_graph_table_count, 8);
        assert!(!session_columns.contains(&"world_profile".to_string()));
        assert!(!session_columns.contains(&"character_profile".to_string()));
        assert!(!session_columns.contains(&"key_story_beats".to_string()));
        assert!(session_world_columns.contains(&"world_profile".to_string()));
        assert!(session_world_columns.contains(&"global_key_story_beats".to_string()));
        assert!(session_character_columns.contains(&"character_name".to_string()));
        assert!(session_character_columns.contains(&"character_profile".to_string()));
        assert!(session_character_columns.contains(&"key_story_beats".to_string()));
        assert!(session_character_columns.contains(&"player_id".to_string()));
        assert!(session_character_columns.contains(&"is_playable".to_string()));
        assert!(!story_edge_columns.contains(&"action".to_string()));
        assert!(!story_edge_columns.contains(&"action_type".to_string()));
        assert!(story_edge_action_columns.contains(&"character_name".to_string()));
        assert!(story_edge_action_columns.contains(&"player_id".to_string()));
        assert!(story_edge_action_columns.contains(&"action_type".to_string()));
        assert!(story_edge_action_columns.contains(&"title".to_string()));
        assert!(story_edge_action_columns.contains(&"action".to_string()));
        assert!(story_edge_action_columns.contains(&"motivation_and_risk".to_string()));
        assert!(agent_context_item_columns.contains(&"message_role".to_string()));
        assert!(agent_context_item_columns.contains(&"content".to_string()));
        assert!(!agent_context_item_columns.contains(&"message_json".to_string()));
    }

    #[tokio::test]
    async fn flow_outputs_upsert_and_page_with_before_cursor() {
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
    async fn story_edge_actions_store_choice_option_fields() {
        let db_path = std::env::temp_dir().join(format!(
            "akasa-story-edge-choice-{}.sqlite3",
            Uuid::new_v4().simple()
        ));
        let repo = SessionArchiveRepository::new(AppDatabase::new(db_path.clone()));
        let choice = PendingCharacterChoice {
            id: "choice-1".to_string(),
            option: CharacterOption {
                title: "绕行".to_string(),
                action: "绕到钟楼背面".to_string(),
                motivation_and_risk: "视野更好，但会暴露脚步声".to_string(),
            },
        };

        repo.save_session_created(&SessionCreated {
            session_id: "session-choice-edge".to_string(),
            character_name: "hero".to_string(),
            world_profile: "world".to_string(),
            character_profile: "hero".to_string(),
            key_story_beats: "beats".to_string(),
        })
        .await
        .expect("session metadata should save");
        repo.save_rounds(
            "session-choice-edge",
            &[RoundHistoryEntry {
                round: 1,
                choices: vec![choice.clone()],
                ..RoundHistoryEntry::default()
            }],
        )
        .await
        .expect("round choices should save");
        repo.update_session_turn_state("session-choice-edge", TurnPhase::AwaitingPlayer, 1, 1)
            .await
            .expect("session active node should update");
        repo.save_player_input(&PlayerInput {
            session_id: "session-choice-edge".to_string(),
            round: 1,
            actions: vec![PlayerActionItem::character_selected_option(&choice.option)],
        })
        .await
        .expect("story edge should save");

        let conn = Connection::open(db_path).expect("sqlite db should open");
        let edge_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM story_edges WHERE session_id = ?1",
                params!["session-choice-edge"],
                |row| row.get(0),
            )
            .expect("story edge should be stored");
        let stored_action: (String, Option<String>, String, String, String, String) = conn
            .query_row(
                r#"
                SELECT character_name, player_id, action_type, title, action, motivation_and_risk
                FROM story_edge_actions
                WHERE session_id = ?1
                "#,
                params!["session-choice-edge"],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .expect("story edge action should be stored");
        let loaded_actions = repo
            .load_story_edge_actions("session-choice-edge")
            .await
            .expect("story edge actions should load");

        assert_eq!(edge_count, 1);
        assert_eq!(stored_action.0, DEFAULT_PLAYER_CHARACTER_NAME);
        assert_eq!(stored_action.1, None);
        assert_eq!(stored_action.2, "selected_option");
        assert_eq!(stored_action.3, choice.option.title);
        assert_eq!(stored_action.4, choice.option.action);
        assert_eq!(stored_action.5, choice.option.motivation_and_risk);
        assert_eq!(loaded_actions.len(), 1);
        assert_eq!(loaded_actions[0].action.action, "绕到钟楼背面");
    }

    #[tokio::test]
    async fn story_edge_actions_store_empty_title_for_free_text() {
        let db_path = std::env::temp_dir().join(format!(
            "akasa-free-text-edge-{}.sqlite",
            Uuid::new_v4().simple()
        ));
        let repo = SessionArchiveRepository::new(AppDatabase::new(db_path.clone()));

        repo.save_session_created(&SessionCreated {
            session_id: "session-free-text-edge".to_string(),
            character_name: "hero".to_string(),
            world_profile: "world".to_string(),
            character_profile: "hero".to_string(),
            key_story_beats: "beats".to_string(),
        })
        .await
        .expect("session metadata should save");
        repo.update_session_turn_state("session-free-text-edge", TurnPhase::AwaitingPlayer, 1, 1)
            .await
            .expect("session active node should update");
        repo.save_player_input(&PlayerInput {
            session_id: "session-free-text-edge".to_string(),
            round: 1,
            actions: vec![PlayerActionItem::character_free_text("检查密室暗门")],
        })
        .await
        .expect("story edge should save");

        let conn = Connection::open(db_path).expect("sqlite db should open");
        let stored_action: (String, String, String) = conn
            .query_row(
                r#"
                SELECT action_type, title, action
                FROM story_edge_actions
                WHERE session_id = ?1
                "#,
                params!["session-free-text-edge"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("story edge action should be stored");

        assert_eq!(stored_action.0, "free_text");
        assert_eq!(stored_action.1, "");
        assert_eq!(stored_action.2, "检查密室暗门");
    }

    #[tokio::test]
    async fn session_flow_turn_and_agent_context_tables_round_trip() {
        let repo = test_repo();
        repo.save_session_created(&SessionCreated {
            session_id: "session-db".to_string(),
            character_name: "hero".to_string(),
            world_profile: "world".to_string(),
            character_profile: "hero".to_string(),
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
        assert_eq!(metadata.phase, TurnPhase::Start);
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

        repo.save_agent_context_item(&AgentContextItemAppended {
            session_id: "session-db".to_string(),
            round: 1,
            agent_name: "UpperNarrator".to_string(),
            message: Message::user("latest context"),
        })
        .await
        .expect("agent context item should save");
        repo.save_agent_context_item(&AgentContextItemAppended {
            session_id: "session-db".to_string(),
            round: 1,
            agent_name: "UpperNarrator".to_string(),
            message: Message::assistant("discarded response"),
        })
        .await
        .expect("assistant context item should save");
        repo.save_agent_context_rollback(&AgentContextRollback {
            session_id: "session-db".to_string(),
            round: 1,
            agent_name: "UpperNarrator".to_string(),
            policy: AgentContextRollbackPolicy::LatestInput,
        })
        .await
        .expect("agent context rollback should save");
        let contexts = repo
            .load_agent_contexts("session-db")
            .await
            .expect("agent contexts should load");
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].agent_name, "UpperNarrator");
        assert!(contexts[0].context.conversation().is_empty());

        repo.save_player_input(&PlayerInput {
            session_id: "session-db".to_string(),
            round: 1,
            actions: vec![PlayerActionItem {
                action_type: PlayerActionType::SelectedOption,
                action: "绕到钟楼背面".to_string(),
                ..PlayerActionItem::default()
            }],
        })
        .await
        .expect("story edge action should save");
        let inputs = repo
            .load_story_edge_actions("session-db")
            .await
            .expect("story edge actions should load");
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].round, 1);
        assert_eq!(
            inputs[0].action.action_type,
            PlayerActionType::SelectedOption
        );
        assert_eq!(inputs[0].action.action, "绕到钟楼背面");

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

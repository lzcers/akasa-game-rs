use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};
use story_engine::resources::session_events::{FlowTurnError, SessionCreated};

use crate::{database::AppDatabase, session_history::TurnPhase};

use super::codec::deserialize_phase;
use super::story_path::{
    SessionBaseRecord, ensure_linear_story_path, linear_node_id_for_depth,
    turn_state_from_active_node, update_story_node_state, upsert_session_base,
};
use super::{ROOT_NODE_ID, SessionArchiveRepository, StoredSessionMetadata, schema};

impl SessionArchiveRepository {
    pub fn new(db: AppDatabase) -> Self {
        Self { db }
    }
    pub async fn clear_session_state(&self, session_id: &str) -> Result<()> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }

        let _guard = self.db.lock().await;
        let mut conn = self.db.open_connection("session replacement")?;
        schema::init(&conn)?;
        let tx = conn
            .transaction()
            .context("failed to start session state clearing transaction")?;
        for table_name in [
            "entity_context_items",
            "entity_flow_outputs",
            "story_edge_actions",
            "story_edges",
            "story_nodes",
            "session_characters",
            "session_worlds",
            "sessions",
        ] {
            tx.execute(
                &format!("DELETE FROM {table_name} WHERE session_id = ?1"),
                params![session_id],
            )
            .with_context(|| format!("failed to clear {table_name} for session"))?;
        }
        tx.commit()
            .context("failed to commit session state clearing transaction")?;
        Ok(())
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
}

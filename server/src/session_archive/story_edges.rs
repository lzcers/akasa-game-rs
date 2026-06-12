use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use story_engine::{
    components::{
        agent::AgentOutputType,
        outcome::{CharacterOption, CharacterOptions, PlayerActionItem, PlayerActionType},
    },
    resources::session_events::PlayerInput,
};

use crate::session_history::{RoundHistoryEntry, TurnPhase};

use super::codec::{
    deserialize_player_action_type, serialize_agent_output_type, serialize_phase,
    serialize_player_action_type,
};
use super::story_path::{ensure_linear_story_path, linear_node_id_for_depth};
use super::{
    SessionArchiveRepository, StoredStoryEdgeAction, normalize_action_character_name, schema,
    session_character_name,
};

impl SessionArchiveRepository {
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
        let session_character_name = session_character_name(&conn, session_id)?;
        let actions = actions
            .into_iter()
            .map(|action| normalize_action_character_name(action, &session_character_name))
            .collect::<Vec<_>>();
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
    pub async fn has_story_edge_action_for_round(
        &self,
        session_id: &str,
        round: u64,
    ) -> Result<bool> {
        let session_id = session_id.trim();
        if session_id.is_empty() || round == 0 {
            return Ok(false);
        }

        let from_node_id = linear_node_id_for_depth(round);
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("story edge duplicate check")?;
        schema::init(&conn)?;
        let count = conn
            .query_row(
                r#"
                SELECT COUNT(1)
                FROM story_edge_actions
                WHERE session_id = ?1
                    AND from_node_id = ?2
                "#,
                params![session_id, from_node_id],
                |row| row.get::<_, i64>(0),
            )
            .context("failed to check story edge action duplicate")?;
        Ok(count > 0)
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
        let session_character_name = session_character_name(&tx, session_id)?;
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
                .map(|action| normalize_action_character_name(action, &session_character_name))
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
            FROM entity_flow_outputs
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

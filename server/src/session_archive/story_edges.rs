use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use story_engine::{
    components::{
        agent::AgentOutputType,
        outcome::{CharacterOption, CharacterOptions, PlayerActionItem, PlayerActionType},
    },
    resources::session_events::PlayerInput,
};

use crate::session_history::TurnPhase;

use super::codec::{
    deserialize_player_action_type, serialize_agent_output_type, serialize_phase,
    serialize_player_action_type,
};
use super::story_path::{
    activate_story_node, active_or_linear_node_id_for_depth, create_branch_story_node,
    story_node_id_for_active_path_depth, update_story_node_state,
};
use super::{
    PreparedBacktrackBranch, SessionArchiveRepository, StoredBranchExploration,
    StoredChoiceExploration, StoredStoryEdgeAction, normalize_action_character_name, schema,
    session_character_name,
};

impl SessionArchiveRepository {
    pub async fn save_player_input(&self, input: &PlayerInput) -> Result<Option<String>> {
        let session_id = input.session_id.trim();
        let actions = input
            .actions
            .iter()
            .cloned()
            .map(PlayerActionItem::normalized)
            .filter(|action| !action.action.is_empty())
            .collect::<Vec<_>>();
        if session_id.is_empty() || actions.is_empty() {
            return Ok(None);
        }

        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("story edges")?;
        schema::init(&conn)?;
        let session_character_name = session_character_name(&conn, session_id)?;
        let actions = actions
            .into_iter()
            .map(|action| normalize_action_character_name(action, &session_character_name))
            .collect::<Vec<_>>();
        let now = chrono::Utc::now().to_rfc3339();
        let from_node_id =
            active_or_linear_node_id_for_depth(&conn, session_id, input.round, &now)?;
        let prepared_to_node_id = match actions.first() {
            Some(action) => branch_node_for_action(&conn, session_id, &from_node_id, action)?,
            None => None,
        };
        let to_node_id = match prepared_to_node_id {
            Some(node_id) => node_id,
            None => active_or_linear_node_id_for_depth(&conn, session_id, input.round + 1, &now)?,
        };
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
                choice_option_for_story_edge(&conn, session_id, &from_node_id, action)?;
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
        Ok(Some(to_node_id))
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
                    from_path.node_depth,
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
                JOIN active_path from_path
                    ON from_path.node_id = e.from_node_id
                JOIN active_path to_path
                    ON to_path.node_id = e.to_node_id
                WHERE action.session_id = ?1
                    AND from_path.node_depth > 0
                    AND to_path.node_depth = from_path.node_depth + 1
                ORDER BY from_path.node_depth ASC, action.character_name ASC
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

    pub async fn load_choice_explorations(
        &self,
        session_id: &str,
    ) -> Result<Vec<StoredChoiceExploration>> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("choice explorations")?;
        schema::init(&conn)?;
        let Some(active_node_id) = active_story_node_id(&conn, session_id)? else {
            return Ok(Vec::new());
        };
        choice_explorations_for_node(&conn, session_id, &active_node_id)
    }

    pub async fn load_choice_explorations_for_node(
        &self,
        session_id: &str,
        node_id: &str,
    ) -> Result<Vec<StoredChoiceExploration>> {
        let node_id = node_id.trim();
        if node_id.is_empty() {
            return Ok(Vec::new());
        }

        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("choice explorations for node")?;
        schema::init(&conn)?;
        choice_explorations_for_node(&conn, session_id, node_id)
    }

    pub async fn load_branch_explorations(
        &self,
        session_id: &str,
    ) -> Result<Vec<StoredBranchExploration>> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("branch explorations")?;
        schema::init(&conn)?;
        let Some(active_node_id) = active_story_node_id(&conn, session_id)? else {
            return Ok(Vec::new());
        };
        branch_explorations_for_node(&conn, session_id, &active_node_id)
    }

    pub async fn load_branch_explorations_for_node(
        &self,
        session_id: &str,
        node_id: &str,
    ) -> Result<Vec<StoredBranchExploration>> {
        let node_id = node_id.trim();
        if node_id.is_empty() {
            return Ok(Vec::new());
        }

        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("branch explorations for node")?;
        schema::init(&conn)?;
        branch_explorations_for_node(&conn, session_id, node_id)
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

        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("story edge duplicate check")?;
        schema::init(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        let from_node_id = active_or_linear_node_id_for_depth(&conn, session_id, round, &now)?;
        let count = conn
            .query_row(
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
                SELECT COUNT(1)
                FROM story_edge_actions action
                JOIN active_path to_path
                    ON to_path.node_id = action.to_node_id
                WHERE action.session_id = ?1
                    AND action.from_node_id = ?2
                "#,
                params![session_id, from_node_id],
                |row| row.get::<_, i64>(0),
            )
            .context("failed to check story edge action duplicate")?;
        Ok(count > 0)
    }
    pub async fn prepare_backtrack_branch(
        &self,
        session_id: &str,
        source_round: u64,
        actions: &[PlayerActionItem],
    ) -> Result<PreparedBacktrackBranch> {
        let session_id = session_id.trim();
        if session_id.is_empty() || source_round == 0 || actions.is_empty() {
            anyhow::bail!("invalid backtrack branch request");
        }

        let _guard = self.db.lock().await;
        let mut conn = self.db.open_connection("backtrack branch")?;
        schema::init(&conn)?;
        let tx = conn
            .transaction()
            .context("failed to start backtrack branch transaction")?;
        let session_character_name = session_character_name(&tx, session_id)?;
        let actions = actions
            .iter()
            .cloned()
            .map(PlayerActionItem::normalized)
            .map(|action| normalize_action_character_name(action, &session_character_name))
            .filter(|action| !action.action.is_empty())
            .collect::<Vec<_>>();
        let Some(primary_action) = actions.first() else {
            anyhow::bail!("backtrack branch action is empty");
        };

        let source_node_id = story_node_id_for_active_path_depth(&tx, session_id, source_round)?
            .ok_or_else(|| anyhow::anyhow!("source round is not on the active story path"))?;
        let branch_round = source_round + 1;
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(existing_branch_node_id) =
            branch_node_for_action(&tx, session_id, &source_node_id, primary_action)?
        {
            let existing_branch_round =
                activate_story_node(&tx, session_id, &existing_branch_node_id, &now)?;
            let has_existing_narration =
                story_node_has_narration_output(&tx, session_id, &existing_branch_node_id)?;
            if has_existing_narration {
                let phase =
                    story_node_reactivation_phase(&tx, session_id, &existing_branch_node_id)?;
                update_story_node_state(
                    &tx,
                    session_id,
                    &existing_branch_node_id,
                    phase,
                    None,
                    &now,
                )?;
            }
            tx.commit()
                .context("failed to commit existing backtrack branch activation")?;
            return Ok(PreparedBacktrackBranch {
                source_round,
                branch_round: existing_branch_round,
                branch_node_id: existing_branch_node_id,
                reused_existing_branch: has_existing_narration,
                requires_generation: !has_existing_narration,
            });
        }

        let branch_node_id = create_branch_story_node(
            &tx,
            session_id,
            &source_node_id,
            branch_round,
            TurnPhase::Simulation,
            &now,
        )?;
        tx.execute(
            r#"
            INSERT INTO story_edges (
                session_id,
                from_node_id,
                to_node_id,
                created_at
            ) VALUES (?1, ?2, ?3, ?4)
            "#,
            params![session_id, source_node_id, branch_node_id, now],
        )
        .context("failed to insert backtrack story edge")?;
        for action in &actions {
            let choice_option =
                choice_option_for_story_edge(&tx, session_id, &source_node_id, action)?;
            upsert_story_edge_action(
                &tx,
                StoryEdgeActionRecord {
                    session_id,
                    from_node_id: &source_node_id,
                    to_node_id: &branch_node_id,
                    action,
                    choice_option: &choice_option,
                    submitted_at: &now,
                },
            )?;
        }

        tx.commit()
            .context("failed to commit backtrack branch creation")?;
        Ok(PreparedBacktrackBranch {
            source_round,
            branch_round,
            branch_node_id,
            reused_existing_branch: false,
            requires_generation: true,
        })
    }
}

fn story_node_has_narration_output(
    conn: &Connection,
    session_id: &str,
    node_id: &str,
) -> Result<bool> {
    let count: i64 = conn
        .query_row(
            r#"
            SELECT COUNT(*)
            FROM entity_flow_outputs
            WHERE session_id = ?1
                AND node_id = ?2
                AND stage = 'application'
                AND output_type = 'text'
                AND length(trim(content)) > 0
            "#,
            params![session_id, node_id],
            |row| row.get(0),
        )
        .context("failed to check story node narration output")?;
    Ok(count > 0)
}

fn story_node_reactivation_phase(
    conn: &Connection,
    session_id: &str,
    node_id: &str,
) -> Result<TurnPhase> {
    let flow_end: bool = conn
        .query_row(
            r#"
            SELECT flow_end
            FROM story_nodes
            WHERE session_id = ?1
                AND node_id = ?2
            "#,
            params![session_id, node_id],
            |row| row.get(0),
        )
        .context("failed to load story node flow end flag")?;
    Ok(if flow_end {
        TurnPhase::Ended
    } else {
        TurnPhase::AwaitingPlayer
    })
}

fn active_story_node_id(conn: &Connection, session_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT active_node_id FROM sessions WHERE session_id = ?1",
        params![session_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .context("failed to load active story node id")
}

fn choice_explorations_for_node(
    conn: &Connection,
    session_id: &str,
    node_id: &str,
) -> Result<Vec<StoredChoiceExploration>> {
    let mut stmt = conn
        .prepare(
            r#"
            WITH RECURSIVE selected_path(node_id, parent_node_id, node_depth) AS (
                SELECT node_id, parent_node_id, node_depth
                FROM story_nodes
                WHERE session_id = ?1
                    AND node_id = ?2
                UNION ALL
                SELECT parent.node_id, parent.parent_node_id, parent.node_depth
                FROM story_nodes parent
                JOIN selected_path child
                    ON child.parent_node_id = parent.node_id
                WHERE parent.session_id = ?1
            )
            SELECT DISTINCT
                selected_path.node_depth,
                action.action
            FROM story_edge_actions action
            JOIN selected_path
                ON selected_path.node_id = action.from_node_id
            WHERE action.session_id = ?1
                AND selected_path.node_depth > 0
                AND length(trim(action.action)) > 0
                AND EXISTS (
                    SELECT 1
                    FROM entity_flow_outputs output
                    WHERE output.session_id = action.session_id
                        AND output.node_id = action.to_node_id
                        AND output.stage = 'application'
                        AND output.output_type = 'text'
                        AND length(trim(output.content)) > 0
                )
            ORDER BY selected_path.node_depth ASC, action.action ASC
            "#,
        )
        .context("failed to prepare choice exploration load")?;
    let rows = stmt
        .query_map(params![session_id, node_id], |row| {
            Ok(StoredChoiceExploration {
                round: row.get::<_, i64>(0)?.try_into().unwrap_or_default(),
                action: row.get(1)?,
            })
        })
        .context("failed to query choice explorations")?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read choice explorations")
}

fn branch_explorations_for_node(
    conn: &Connection,
    session_id: &str,
    node_id: &str,
) -> Result<Vec<StoredBranchExploration>> {
    let mut stmt = conn
        .prepare(
            r#"
            WITH RECURSIVE selected_path(node_id, parent_node_id, node_depth) AS (
                SELECT node_id, parent_node_id, node_depth
                FROM story_nodes
                WHERE session_id = ?1
                    AND node_id = ?2
                UNION ALL
                SELECT parent.node_id, parent.parent_node_id, parent.node_depth
                FROM story_nodes parent
                JOIN selected_path child
                    ON child.parent_node_id = parent.node_id
                WHERE parent.session_id = ?1
            )
            SELECT
                selected_path.node_depth,
                action.character_name,
                action.player_id,
                action.action_type,
                action.title,
                action.action,
                action.motivation_and_risk
            FROM story_edge_actions action
            JOIN selected_path
                ON selected_path.node_id = action.from_node_id
            WHERE action.session_id = ?1
                AND selected_path.node_depth > 0
                AND length(trim(action.action)) > 0
                AND EXISTS (
                    SELECT 1
                    FROM entity_flow_outputs output
                    WHERE output.session_id = action.session_id
                        AND output.node_id = action.to_node_id
                        AND output.stage = 'application'
                        AND output.output_type = 'text'
                        AND length(trim(output.content)) > 0
                )
            ORDER BY selected_path.node_depth ASC, action.submitted_at ASC, action.action ASC
            "#,
        )
        .context("failed to prepare branch exploration load")?;
    let rows = stmt
        .query_map(params![session_id, node_id], |row| {
            let action_type: String = row.get(3)?;
            Ok(StoredBranchExploration {
                round: row.get::<_, i64>(0)?.try_into().unwrap_or_default(),
                action: PlayerActionItem {
                    character_name: row.get(1)?,
                    player_id: row.get(2)?,
                    action_type: deserialize_player_action_type(&action_type).map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                        )
                    })?,
                    title: row.get(4)?,
                    action: row.get(5)?,
                    motivation_and_risk: row.get(6)?,
                },
                visited: true,
            })
        })
        .context("failed to query branch explorations")?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read branch explorations")
}

fn choice_option_for_story_edge(
    conn: &Connection,
    session_id: &str,
    source_node_id: &str,
    action: &PlayerActionItem,
) -> Result<CharacterOption> {
    if action.action_type == PlayerActionType::SelectedOption
        && let Some(choice) =
            selected_choice_option_for_action(conn, session_id, source_node_id, &action.action)?
    {
        return Ok(choice);
    }

    Ok(choice_option_from_action(action))
}
fn selected_choice_option_for_action(
    conn: &Connection,
    session_id: &str,
    node_id: &str,
    action: &str,
) -> Result<Option<CharacterOption>> {
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
fn branch_node_for_action(
    conn: &Connection,
    session_id: &str,
    source_node_id: &str,
    action: &PlayerActionItem,
) -> Result<Option<String>> {
    let action_type = serialize_player_action_type(action.action_type)?;
    conn.query_row(
        r#"
        SELECT edge.to_node_id
        FROM story_edges edge
        JOIN story_edge_actions action
            ON action.session_id = edge.session_id
            AND action.from_node_id = edge.from_node_id
            AND action.to_node_id = edge.to_node_id
        WHERE edge.session_id = ?1
            AND edge.from_node_id = ?2
            AND action.action = ?3
            AND action.action_type = ?4
        ORDER BY edge.created_at DESC, edge.to_node_id DESC
        LIMIT 1
        "#,
        params![session_id, source_node_id, action.action, action_type],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .context("failed to find branch node for action")
}
fn choice_option_from_action(action: &PlayerActionItem) -> CharacterOption {
    CharacterOption {
        title: action.title.clone(),
        action: action.action.clone(),
        motivation_and_risk: action.motivation_and_risk.clone(),
    }
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

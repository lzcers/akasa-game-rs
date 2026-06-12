use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use story_engine::{
    components::{
        agent::AgentOutputType, outcome::CharacterOptions, world_snapshot::WorldSnapshot,
    },
    resources::session_events::FlowTurnUpdate,
};

use crate::session_history::{RoundHistoryEntry, TurnPhase};

use super::codec::{
    deserialize_agent_output_type, deserialize_phase, serialize_agent_output_type, serialize_phase,
};
use super::story_path::{
    StoryPathNode, active_or_linear_node_id_for_depth, ensure_linear_story_path,
    linear_node_id_for_depth, select_story_path_nodes,
};
use super::{
    DEFAULT_PLAYER_CHARACTER_NAME, SessionArchiveRepository, StoredSessionRoundPage, schema,
};

impl SessionArchiveRepository {
    pub async fn save_flow_turn_update(&self, update: &FlowTurnUpdate) -> Result<()> {
        let session_id = update.session_id.trim();
        let entity_name = update.entity_name.trim();
        if session_id.is_empty() || entity_name.is_empty() {
            return Ok(());
        }

        let stage = serialize_phase(update.stage)?;
        let output_type = serialize_agent_output_type(update.output_type)?;
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("entity flow outputs")?;
        schema::init(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        let node_id = active_or_linear_node_id_for_depth(&conn, session_id, update.round, &now)?;
        conn.execute(
            r#"
            INSERT INTO entity_flow_outputs (
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
        .context("failed to upsert entity flow output")?;
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
                "DELETE FROM entity_flow_outputs WHERE session_id = ?1 AND node_id = ?2",
                params![session_id, node_id],
            )
            .context("failed to clear existing entity flow outputs for round")?;
            for output in entity_flow_outputs_from_round(session_id, round)? {
                tx.execute(
                    r#"
                    INSERT INTO entity_flow_outputs (
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
                .context("failed to insert archived entity flow output")?;
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

#[derive(Debug)]
struct FlowOutputRow {
    session_id: String,
    node_id: String,
    stage: String,
    entity_name: String,
    output_type: String,
    content: String,
}
fn entity_flow_outputs_from_round(
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
            FROM entity_flow_outputs
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
        .context("failed to prepare entity flow output query")?;
    let rows = stmt
        .query_map(params![session_id, path_node.node_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .context("failed to query entity flow outputs")?;
    let mut entry = RoundHistoryEntry {
        round: path_node.node_depth,
        ..RoundHistoryEntry::default()
    };
    for row in rows {
        let (stage, _entity_name, output_type, content) =
            row.context("failed to read entity flow output")?;
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

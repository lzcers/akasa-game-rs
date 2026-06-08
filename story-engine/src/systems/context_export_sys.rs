use std::{fs, path::PathBuf};

use agent::agent::Context;
use bevy_ecs::{
    component::Component,
    entity::Entity,
    hierarchy::ChildOf,
    system::{Commands, Query},
};
use serde::Serialize;

use crate::components::{
    agent::Agent,
    session::StorySession,
    turn_flow::{TurnFlow, TurnStage},
};

const FATE_WEAVER_CONTEXT_FILE: &str = "fate_weaver_context.json";
const PROTAGONIST_CONTEXT_FILE: &str = "protagonist_context.json";
const UPPER_NARRATOR_CONTEXT_FILE: &str = "upper_narrator_context.json";

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContextExportedTurn {
    turn_id: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportedAgentContext<'a> {
    session_id: &'a str,
    turn_index: u64,
    active_turn_id: u64,
    agent: &'a str,
    context: &'a Context,
}

pub(crate) fn context_export_system(
    mut commands: Commands,
    sessions: Query<(
        Entity,
        &StorySession,
        &TurnFlow,
        Option<&ContextExportedTurn>,
    )>,
    agents: Query<(&Agent, &ChildOf)>,
) {
    for (session_entity, session, flow, exported) in sessions.iter().filter(|(_, _, flow, _)| {
        matches!(
            flow.stage,
            TurnStage::TurnCompleted | TurnStage::Ended | TurnStage::Failed
        )
    }) {
        if exported.is_some_and(|exported| exported.turn_id == flow.active_turn_id) {
            continue;
        }

        export_contexts_for_turn(session_entity, session, flow, &agents);
        commands.entity(session_entity).insert(ContextExportedTurn {
            turn_id: flow.active_turn_id,
        });
    }
}

fn export_contexts_for_turn(
    session_entity: Entity,
    session: &StorySession,
    flow: &TurnFlow,
    agents: &Query<(&Agent, &ChildOf)>,
) {
    for (agent_name, file_name) in [
        ("FateWeaver", FATE_WEAVER_CONTEXT_FILE),
        ("Protagonist", PROTAGONIST_CONTEXT_FILE),
        ("UpperNarrator", UPPER_NARRATOR_CONTEXT_FILE),
    ] {
        let Some(agent) = agents
            .iter()
            .find(|(agent, owner)| owner.parent() == session_entity && agent.name == agent_name)
            .map(|(agent, _)| agent)
        else {
            eprintln!("[context-export] missing agent context: {agent_name}");
            continue;
        };

        let payload = ExportedAgentContext {
            session_id: &session.id,
            turn_index: flow.turn_index,
            active_turn_id: flow.active_turn_id,
            agent: &agent.name,
            context: &agent.context,
        };
        let Ok(content) = serde_json::to_string_pretty(&payload) else {
            eprintln!("[context-export] failed to serialize context: {agent_name}");
            continue;
        };
        let path = workspace_root().join(file_name);
        if let Err(error) = fs::write(&path, content) {
            eprintln!(
                "[context-export] failed to write {}: {error}",
                path.display()
            );
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

use bevy_ecs::{
    entity::Entity,
    hierarchy::ChildOf,
    query::{With, Without},
    system::{Commands, Query, Res, ResMut},
};
use serde_json::json;

use crate::{
    components::{
        agent::{Agent, AgentOutputType, PendingReasoning, Simulator},
        flow::{FlowEnd, SimulationCompleted},
        outcome::CharacterDecisionState,
        outcome::SimulationOutcome,
        session_event_sink::SessionEventSink,
        turn_flow::{TurnFlow, TurnStage},
        world_snapshot::WorldSnapshot,
    },
    resources::agent_task_manager::{AgentTaskManager, TaskStatus},
    resources::session_events::AgentContextRollbackPolicy,
    utils::parse_json_response,
};

use super::{output_preview, publish_apply_error};

#[allow(clippy::type_complexity)]
pub fn fate_weaver_dispatch_system(
    mut commands: Commands,
    sessions: Query<(
        Entity,
        &SessionEventSink,
        &TurnFlow,
        &CharacterDecisionState,
        &WorldSnapshot,
    )>,
    agent_tasks: Res<AgentTaskManager>,
    mut agents: Query<
        (Entity, &mut Agent, &ChildOf),
        (
            With<Simulator>,
            Without<PendingReasoning>,
            Without<SimulationOutcome>,
        ),
    >,
) {
    for (session_entity, event_sink, flow, decision_state, world_snapshot) in sessions
        .iter()
        .filter(|(_, _, flow, ..)| flow.stage == TurnStage::Simulation)
    {
        if world_snapshot.is_ending {
            commands.entity(session_entity).insert(FlowEnd);
            continue;
        }

        for (entity, mut agent, _) in agents.iter_mut().filter(|(entity, _, owner)| {
            owner.parent() == session_entity && agent_tasks.task_result(*entity).is_none()
        }) {
            let action_round = flow.active_turn_id().saturating_sub(1);
            let actions = decision_state
                .committed_actions()
                .iter()
                .map(|action| {
                    json!({
                        "character_name": &action.character_name,
                        "action": &action.action,
                    })
                })
                .collect::<Vec<_>>();
            let prompt = json!({
                "round": action_round,
                "actions": actions,
            })
            .to_string();
            let message = agent.append_user_message(&prompt);
            event_sink.publish_agent_context_item_appended(
                flow.active_turn_id().max(1),
                agent.name.clone(),
                message,
            );
            commands.entity(entity).insert(PendingReasoning);
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn fate_weaver_apply_system(
    mut commands: Commands,
    mut sessions: Query<(Entity, &SessionEventSink, &mut TurnFlow, &mut WorldSnapshot)>,
    mut agents: Query<(Entity, &mut Agent, &ChildOf), With<Simulator>>,
    mut agent_tasks: ResMut<AgentTaskManager>,
) {
    for (session_entity, event_sink, mut flow, mut world_snapshot) in sessions
        .iter_mut()
        .filter(|(_, _, flow, _)| flow.stage == TurnStage::Simulation)
    {
        for (entity, mut agent, _) in agents
            .iter_mut()
            .filter(|(_, _, owner)| owner.parent() == session_entity)
        {
            let Some(result) = agent_tasks.task_result(entity).cloned() else {
                continue;
            };
            match result.status {
                TaskStatus::Done => {
                    let Some(mut output) = result.output.clone() else {
                        continue;
                    };

                    if agent.output_type == AgentOutputType::Json {
                        let mut snapshot = match parse_json_response::<WorldSnapshot>(&output) {
                            Ok(snapshot) => snapshot,
                            Err(error) => {
                                let error = format!(
                                    "FateWeaver 输出无法解析为 WorldSnapshot：{error}。输出预览：{}",
                                    output_preview(&output)
                                );
                                if agent_tasks.retry_task(entity, error.clone()) {
                                    break;
                                }
                                publish_apply_error(
                                    event_sink,
                                    &flow,
                                    &agent.name,
                                    format!("{error}；已达到最大重试次数。"),
                                );
                                agent_tasks.clear_task(entity);
                                if agent.revert() {
                                    event_sink.publish_agent_context_rollback(
                                        flow.active_turn_id().max(1),
                                        agent.name.clone(),
                                        AgentContextRollbackPolicy::LatestInput,
                                    );
                                }
                                flow.stage = TurnStage::Failed;
                                break;
                            }
                        };
                        snapshot.round = flow.active_turn_id();
                        if let Ok(normalized_output) = serde_json::to_string_pretty(&snapshot) {
                            output = normalized_output;
                        }
                        *world_snapshot = snapshot;
                    }
                    let _ = agent_tasks.take_result(entity);
                    let message = agent.append_assistant_message(&output);
                    event_sink.publish_agent_context_item_appended(
                        flow.active_turn_id().max(1),
                        agent.name.clone(),
                        message,
                    );
                    event_sink.publish_flow_turn_update(
                        flow.active_turn_id().max(1),
                        flow.stage,
                        agent.name.clone(),
                        agent.output_type,
                        output.clone(),
                    );
                    commands
                        .entity(entity)
                        .insert(SimulationOutcome {
                            turn_id: flow.active_turn_id(),
                            content: output,
                        })
                        .insert(SimulationCompleted {
                            turn_id: flow.active_turn_id(),
                        });
                }
                TaskStatus::Error => {
                    flow.stage = TurnStage::Failed;
                    break;
                }
                TaskStatus::Pending | TaskStatus::Running => {}
            }
        }
    }
}

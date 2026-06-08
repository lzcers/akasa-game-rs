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
        outcome::SimulationOutcome,
        turn_flow::{TurnFlow, TurnStage},
    },
    resources::{
        agent_task::{AgentTaskManager, TaskStatus},
        protagonist_action::ProtagonistDecisionState,
        world_snapshot::WorldSnapshot,
    },
    utils::parse_json_response,
};

#[allow(clippy::type_complexity)]
pub fn fate_weaver_dispatch_system(
    mut commands: Commands,
    sessions: Query<(Entity, &TurnFlow, &ProtagonistDecisionState, &WorldSnapshot)>,
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
    for (session_entity, flow, decision_state, world_snapshot) in sessions
        .iter()
        .filter(|(_, flow, ..)| flow.stage == TurnStage::Simulation)
    {
        if world_snapshot.is_ending {
            commands.entity(session_entity).insert(FlowEnd);
            continue;
        }

        for (entity, mut agent, _) in agents.iter_mut().filter(|(entity, _, owner)| {
            owner.parent() == session_entity && agent_tasks.task_result(*entity).is_none()
        }) {
            let action_round = flow.active_turn_id.saturating_sub(1);
            agent.append_user_message(
                &json!({
                    "round": action_round,
                    "protagonist_action": decision_state.committed_action(),
                })
                .to_string(),
            );
            commands.entity(entity).insert(PendingReasoning);
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn fate_weaver_apply_system(
    mut commands: Commands,
    mut sessions: Query<(Entity, &mut TurnFlow, &mut WorldSnapshot)>,
    mut agents: Query<(Entity, &mut Agent, &ChildOf), With<Simulator>>,
    mut agent_tasks: ResMut<AgentTaskManager>,
) {
    for (session_entity, mut flow, mut world_snapshot) in sessions
        .iter_mut()
        .filter(|(_, flow, _)| flow.stage == TurnStage::Simulation)
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
                    let Some(mut output) = agent_tasks
                        .take_result(entity)
                        .and_then(|result| result.output)
                    else {
                        continue;
                    };

                    if agent.output_type == AgentOutputType::Json {
                        let Ok(mut snapshot) = parse_json_response::<WorldSnapshot>(&output) else {
                            agent.revert();
                            flow.stage = TurnStage::Failed;
                            break;
                        };
                        snapshot.round = flow.active_turn_id;
                        if let Ok(normalized_output) = serde_json::to_string_pretty(&snapshot) {
                            output = normalized_output;
                        }
                        *world_snapshot = snapshot;
                    }
                    agent.append_assistant_message(&output);
                    commands
                        .entity(entity)
                        .insert(SimulationOutcome {
                            turn_id: flow.active_turn_id,
                            content: output,
                        })
                        .insert(SimulationCompleted {
                            turn_id: flow.active_turn_id,
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

#[cfg(test)]
mod tests {
    use agent::core::Message;
    use agent::models::ChatModel;
    use bevy_ecs::{hierarchy::ChildOf, prelude::*, schedule::Schedule};

    use super::*;
    use crate::resources::agent_task::{TaskKind, TaskResult};

    #[test]
    fn dispatches_initial_action_as_round_zero_start() {
        let mut world = World::new();
        world.insert_resource(AgentTaskManager::new(ChatModel::new()));
        let session = world
            .spawn((
                TurnFlow {
                    turn_index: 0,
                    active_turn_id: 1,
                    stage: TurnStage::Simulation,
                },
                ProtagonistDecisionState::default(),
                WorldSnapshot::default(),
            ))
            .id();
        let agent = world
            .spawn((
                Agent::new(AgentOutputType::Json, "FateWeaver", "weave".to_string()),
                ChildOf(session),
                Simulator,
            ))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(fate_weaver_dispatch_system);

        schedule.run(&mut world);

        let agent = world.get::<Agent>(agent).expect("agent should exist");
        let conversation = agent.context.conversation();
        let Message::User { content } = conversation
            .last()
            .expect("user message should be appended")
        else {
            panic!("latest conversation message should be a user message");
        };
        let payload: serde_json::Value =
            serde_json::from_str(content).expect("user message should be JSON");
        assert_eq!(payload["round"], 0);
        assert_eq!(payload["protagonist_action"], "start");
    }

    #[test]
    fn normalizes_world_snapshot_and_outcome_content_to_active_turn() {
        let mut world = World::new();
        world.insert_resource(AgentTaskManager::new(ChatModel::new()));
        let session = world
            .spawn((
                TurnFlow {
                    turn_index: 0,
                    active_turn_id: 1,
                    stage: TurnStage::Simulation,
                },
                WorldSnapshot::default(),
            ))
            .id();
        let agent = world
            .spawn((
                Agent::new(AgentOutputType::Json, "FateWeaver", "weave".to_string()),
                ChildOf(session),
                Simulator,
            ))
            .id();
        let output = serde_json::to_string(&WorldSnapshot {
            round: 99,
            scene_title: "opening".to_string(),
            ..WorldSnapshot::default()
        })
        .expect("world snapshot should serialize");
        world
            .resource_mut::<AgentTaskManager>()
            .set_result_for_test(
                agent,
                TaskResult {
                    kind: TaskKind::Simulation,
                    status: TaskStatus::Done,
                    attempts: 1,
                    max_attempts: 1,
                    last_error: None,
                    chunks: Vec::new(),
                    output: Some(output),
                    error: None,
                },
            );
        let mut schedule = Schedule::default();
        schedule.add_systems(fate_weaver_apply_system);

        schedule.run(&mut world);

        let snapshot = world
            .query::<&WorldSnapshot>()
            .single(&world)
            .expect("world snapshot should exist");
        assert_eq!(snapshot.round, 1);

        let outcome = world
            .get::<SimulationOutcome>(agent)
            .expect("simulation outcome should exist");
        let payload: serde_json::Value =
            serde_json::from_str(&outcome.content).expect("outcome content should be JSON");
        assert_eq!(payload["round"], 1);
    }
}

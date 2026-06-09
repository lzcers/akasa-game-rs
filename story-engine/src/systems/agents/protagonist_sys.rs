use bevy_ecs::{
    entity::Entity,
    hierarchy::ChildOf,
    query::{With, Without},
    system::{Commands, Query, Res, ResMut},
};

use crate::{
    components::{
        agent::{Agent, AgentOutputType, Applicator, PendingReasoning},
        flow::ApplicationCompleted,
        session::StorySession,
        turn_flow::{TurnFlow, TurnStage},
    },
    engine::RuntimeDebugObserverResource,
    resources::{
        agent_task::{AgentTaskManager, TaskKind, TaskStatus},
        export::ExportState,
        protagonist_action::{ProtagonistDecisionState, ProtagonistOptions},
        world_snapshot::WorldSnapshot,
    },
    utils::parse_json_response,
};

use super::{output_preview, publish_apply_error};

#[allow(clippy::type_complexity)]
pub fn protagonist_dispatch_system(
    mut commands: Commands,
    sessions: Query<(Entity, &TurnFlow, &ProtagonistDecisionState, &WorldSnapshot)>,
    agent_tasks: Res<AgentTaskManager>,
    mut agents: Query<
        (Entity, &mut Agent, &ChildOf, Option<&ApplicationCompleted>),
        (With<Applicator>, Without<PendingReasoning>),
    >,
) {
    for (session_entity, flow, decision_state, world_snapshot) in
        sessions.iter().filter(|(_, flow, _, world_snapshot)| {
            flow.stage == TurnStage::Application && !world_snapshot.is_ending
        })
    {
        let prompt = world_snapshot.to_protagonist_prompt(Some(decision_state.committed_action()));

        for (entity, mut agent, _, _) in
            agents.iter_mut().filter(|(entity, agent, owner, outcome)| {
                owner.parent() == session_entity
                    && agent.output_type == AgentOutputType::Json
                    && agent_tasks.task_result(*entity).is_none()
                    && !outcome.is_some_and(|outcome| outcome.turn_id == flow.active_turn_id)
            })
        {
            agent.append_user_message(&prompt);
            commands.entity(entity).insert(PendingReasoning);
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn protagonist_apply_system(
    mut commands: Commands,
    mut sessions: Query<(
        Entity,
        Option<&StorySession>,
        &ExportState,
        &mut TurnFlow,
        &mut ProtagonistDecisionState,
    )>,
    mut agents: Query<(Entity, &mut Agent, &ChildOf), With<Applicator>>,
    mut agent_tasks: ResMut<AgentTaskManager>,
    debug_observer: Option<Res<RuntimeDebugObserverResource>>,
) {
    for (session_entity, session, export_state, mut flow, mut decision_state) in sessions
        .iter_mut()
        .filter(|(_, _, _, flow, _)| flow.stage == TurnStage::Application)
    {
        for (entity, mut agent, _) in agents.iter_mut().filter(|(_, agent, owner)| {
            owner.parent() == session_entity && agent.output_type == AgentOutputType::Json
        }) {
            let Some(result) = agent_tasks.task_result(entity).cloned() else {
                continue;
            };
            match result.status {
                TaskStatus::Done => {
                    let Some(output) = result.output.clone() else {
                        continue;
                    };
                    let mut options = match parse_json_response::<ProtagonistOptions>(&output) {
                        Ok(options) => options,
                        Err(error) => {
                            let error = format!(
                                "Protagonist 输出无法解析为行动选项：{error}。输出预览：{}",
                                output_preview(&output)
                            );
                            if agent_tasks.retry_task(entity, error.clone()) {
                                break;
                            }
                            publish_apply_error(
                                export_state,
                                debug_observer.as_deref(),
                                session,
                                &flow,
                                entity,
                                TaskKind::ProtagonistAction,
                                format!("{error}；已达到最大重试次数。"),
                            );
                            agent_tasks.clear_task(entity);
                            agent.revert();
                            flow.stage = TurnStage::Failed;
                            break;
                        }
                    };
                    options
                        .options
                        .retain(|option| !option.action.trim().is_empty());
                    let _ = agent_tasks.take_result(entity);
                    agent.append_assistant_message(&output);
                    if let (Some(session), Some(observer)) = (
                        session,
                        debug_observer
                            .as_ref()
                            .and_then(|debug| debug.observer.as_ref()),
                    ) {
                        observer.on_agent_context_updated(
                            &session.id,
                            flow.turn_index,
                            flow.active_turn_id,
                            &agent.name,
                            &agent.context,
                        );
                    }
                    decision_state.replace_with_options(options);
                    commands.entity(entity).insert(ApplicationCompleted {
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
    use agent::models::ChatModel;
    use bevy_ecs::{prelude::*, schedule::Schedule};

    use super::*;
    use crate::{
        components::agent::PendingReasoning,
        resources::agent_task::{AgentTaskManager, TaskResult},
    };

    #[test]
    fn skips_protagonist_dispatch_when_world_snapshot_is_ending() {
        let mut world = World::new();
        world.insert_resource(AgentTaskManager::new(ChatModel::new()));
        let session = world
            .spawn((
                TurnFlow {
                    turn_index: 6,
                    active_turn_id: 7,
                    stage: TurnStage::Application,
                },
                ProtagonistDecisionState::default(),
                WorldSnapshot {
                    is_ending: true,
                    ..WorldSnapshot::default()
                },
            ))
            .id();
        let protagonist = world
            .spawn((
                Agent::new(
                    AgentOutputType::Json,
                    "Protagonist",
                    "choose action".to_string(),
                ),
                ChildOf(session),
                Applicator,
            ))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(protagonist_dispatch_system);

        schedule.run(&mut world);

        assert!(world.get::<PendingReasoning>(protagonist).is_none());
    }

    #[test]
    fn accepts_empty_protagonist_options_as_continue_state() {
        let mut world = World::new();
        world.insert_resource(AgentTaskManager::new(ChatModel::new()));
        let session = world
            .spawn((
                TurnFlow {
                    turn_index: 2,
                    active_turn_id: 3,
                    stage: TurnStage::Application,
                },
                ProtagonistDecisionState::default(),
                WorldSnapshot::default(),
                ExportState::new(),
            ))
            .id();
        let protagonist = world
            .spawn((
                Agent::new(
                    AgentOutputType::Json,
                    "Protagonist",
                    "choose action".to_string(),
                ),
                ChildOf(session),
                Applicator,
            ))
            .id();
        world
            .resource_mut::<AgentTaskManager>()
            .set_result_for_test(
                protagonist,
                TaskResult {
                    kind: TaskKind::ProtagonistAction,
                    status: TaskStatus::Done,
                    attempts: 1,
                    max_attempts: 1,
                    last_error: None,
                    chunks: Vec::new(),
                    output: Some(r#"{"options":[]}"#.to_string()),
                    error: None,
                },
            );
        let mut schedule = Schedule::default();
        schedule.add_systems(protagonist_apply_system);

        schedule.run(&mut world);

        let decision_state = world
            .get::<ProtagonistDecisionState>(session)
            .expect("decision state should exist");
        assert!(decision_state.choices().is_empty());
        assert!(world.get::<ApplicationCompleted>(protagonist).is_some());
        let flow = world.get::<TurnFlow>(session).expect("flow should exist");
        assert_eq!(flow.stage, TurnStage::Application);
    }
}

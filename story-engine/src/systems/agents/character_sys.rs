use bevy_ecs::{
    entity::Entity,
    hierarchy::ChildOf,
    query::{With, Without},
    system::{Commands, Query, Res, ResMut},
};

use crate::{
    components::{
        agent::{Agent, AgentRole, Applicator, PendingReasoning},
        flow::{ApplicationCompleted, ApplicationSkipped, FlowEnd},
        outcome::{CharacterDecisionState, CharacterOptions},
        session_event_sink::SessionEventSink,
        turn_flow::{TurnFlow, TurnStage},
        world_snapshot::WorldSnapshot,
    },
    prompts::world_prompt,
    resources::agent_task_manager::{AgentTaskManager, TaskStatus},
    resources::session_events::AgentContextRollbackPolicy,
    utils::parse_json_response,
};

use super::{output_preview, publish_apply_error};

#[allow(clippy::type_complexity)]
pub fn character_dispatch_system(
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
        (Entity, &mut Agent, &ChildOf, Option<&ApplicationCompleted>),
        (With<Applicator>, Without<PendingReasoning>),
    >,
) {
    for (session_entity, event_sink, flow, decision_state, world_snapshot) in sessions
        .iter()
        .filter(|(_, _, flow, ..)| flow.stage == TurnStage::Application)
    {
        if world_snapshot.is_ending {
            commands.entity(session_entity).insert(FlowEnd);
            for (entity, ..) in agents.iter_mut().filter(|(_, agent, owner, completed)| {
                owner.parent() == session_entity
                    && agent.role == AgentRole::Character
                    && !completed
                        .is_some_and(|completed| completed.turn_id == flow.active_turn_id())
            }) {
                commands.entity(entity).insert((
                    ApplicationCompleted {
                        turn_id: flow.active_turn_id(),
                    },
                    ApplicationSkipped {
                        turn_id: flow.active_turn_id(),
                    },
                ));
            }
            continue;
        }

        let committed_action = decision_state.committed_action();
        let prompt = world_prompt::character_prompt(world_snapshot, Some(&committed_action));

        for (entity, mut agent, ..) in
            agents
                .iter_mut()
                .filter(|(entity, agent, owner, completed)| {
                    owner.parent() == session_entity
                        && agent.role == AgentRole::Character
                        && agent_tasks.task_result(*entity).is_none()
                        && !completed
                            .is_some_and(|completed| completed.turn_id == flow.active_turn_id())
                })
        {
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
pub fn character_apply_system(
    mut commands: Commands,
    mut sessions: Query<(
        Entity,
        &SessionEventSink,
        &mut TurnFlow,
        &mut CharacterDecisionState,
    )>,
    mut agents: Query<(Entity, &mut Agent, &ChildOf), With<Applicator>>,
    mut agent_tasks: ResMut<AgentTaskManager>,
) {
    for (session_entity, event_sink, mut flow, mut decision_state) in sessions
        .iter_mut()
        .filter(|(_, _, flow, _)| flow.stage == TurnStage::Application)
    {
        for (entity, mut agent, _) in agents.iter_mut().filter(|(_, agent, owner)| {
            owner.parent() == session_entity && agent.role == AgentRole::Character
        }) {
            let Some(result) = agent_tasks.task_result(entity).cloned() else {
                continue;
            };
            match result.status {
                TaskStatus::Done => {
                    let Some(output) = result.output.clone() else {
                        continue;
                    };
                    let mut options = match parse_json_response::<CharacterOptions>(&output) {
                        Ok(options) => options,
                        Err(error) => {
                            let error = format!(
                                "CharacterAgent 输出无法解析为行动选项：{error}。输出预览：{}",
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
                    options
                        .options
                        .retain(|option| !option.action.trim().is_empty());
                    let normalized_output =
                        serde_json::to_string_pretty(&options).unwrap_or_else(|_| output.clone());
                    let _ = agent_tasks.take_result(entity);
                    let message = agent.append_assistant_message(&normalized_output);
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
                        normalized_output,
                    );
                    decision_state.replace_with_options(options);
                    commands.entity(entity).insert(ApplicationCompleted {
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

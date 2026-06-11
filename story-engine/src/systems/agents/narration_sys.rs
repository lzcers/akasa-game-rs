use bevy_ecs::{
    entity::Entity,
    hierarchy::ChildOf,
    query::{With, Without},
    system::{Commands, Query, Res, ResMut},
};

use crate::{
    components::{
        agent::{Agent, AgentRole, Applicator, PendingReasoning},
        flow::ApplicationCompleted,
        outcome::CharacterDecisionState,
        outcome::NarrationOutcome,
        session_event_sink::SessionEventSink,
        turn_flow::{TurnFlow, TurnStage},
        world_snapshot::WorldSnapshot,
    },
    prompts::world_prompt,
    resources::agent_task_manager::{AgentTaskManager, TaskStatus},
};

#[allow(clippy::type_complexity)]
pub fn narration_dispatch_system(
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
        let committed_action = decision_state.committed_action();
        let prompt = format!(
            "{}\n",
            world_prompt::story_prompt(world_snapshot, Some(&committed_action)),
        );

        for (entity, mut agent, _, _) in
            agents.iter_mut().filter(|(entity, agent, owner, outcome)| {
                owner.parent() == session_entity
                    && agent.role == AgentRole::Narrator
                    && agent_tasks.task_result(*entity).is_none()
                    && !outcome.is_some_and(|outcome| outcome.turn_id == flow.active_turn_id())
            })
        {
            let message = agent.append_user_message(&prompt);
            event_sink.publish_entity_context_item_appended(
                flow.active_turn_id().max(1),
                agent.name.clone(),
                message,
            );
            commands.entity(entity).insert(PendingReasoning);
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn narration_apply_system(
    mut commands: Commands,
    mut sessions: Query<(Entity, &SessionEventSink, &mut TurnFlow)>,
    mut agents: Query<(Entity, &mut Agent, &ChildOf), With<Applicator>>,
    mut agent_tasks: ResMut<AgentTaskManager>,
) {
    for (session_entity, event_sink, mut flow) in sessions
        .iter_mut()
        .filter(|(_, _, flow)| flow.stage == TurnStage::Application)
    {
        for (entity, mut agent, _) in agents.iter_mut().filter(|(_, agent, owner)| {
            owner.parent() == session_entity && agent.role == AgentRole::Narrator
        }) {
            let Some(result) = agent_tasks.task_result(entity).cloned() else {
                continue;
            };
            match result.status {
                TaskStatus::Done => {
                    let Some(output) = agent_tasks
                        .take_result(entity)
                        .and_then(|result| result.output)
                    else {
                        continue;
                    };
                    let message = agent.append_assistant_message(&output);
                    event_sink.publish_entity_context_item_appended(
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
                        .insert(NarrationOutcome {
                            turn_id: flow.active_turn_id(),
                            content: output,
                        })
                        .insert(ApplicationCompleted {
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

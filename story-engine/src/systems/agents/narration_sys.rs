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
        outcome::NarrationOutcome,
        session::StorySession,
        turn_flow::{TurnFlow, TurnStage},
    },
    engine::RuntimeDebugObserverResource,
    resources::{
        agent_task::{AgentTaskManager, TaskStatus},
        protagonist_action::ProtagonistDecisionState,
        world_snapshot::WorldSnapshot,
    },
};

#[allow(clippy::type_complexity)]
pub fn narration_dispatch_system(
    mut commands: Commands,
    sessions: Query<(Entity, &TurnFlow, &ProtagonistDecisionState, &WorldSnapshot)>,
    agent_tasks: Res<AgentTaskManager>,
    mut agents: Query<
        (Entity, &mut Agent, &ChildOf, Option<&ApplicationCompleted>),
        (With<Applicator>, Without<PendingReasoning>),
    >,
) {
    for (session_entity, flow, decision_state, world_snapshot) in sessions
        .iter()
        .filter(|(_, flow, ..)| flow.stage == TurnStage::Application)
    {
        let prompt = format!(
            "{}\n",
            world_snapshot.to_story_prompt(Some(decision_state.committed_action())),
        );

        for (entity, mut agent, _, _) in
            agents.iter_mut().filter(|(entity, agent, owner, outcome)| {
                owner.parent() == session_entity
                    && agent.output_type == AgentOutputType::Text
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
pub fn narration_apply_system(
    mut commands: Commands,
    mut sessions: Query<(Entity, Option<&StorySession>, &mut TurnFlow)>,
    mut agents: Query<(Entity, &mut Agent, &ChildOf), With<Applicator>>,
    mut agent_tasks: ResMut<AgentTaskManager>,
    debug_observer: Option<Res<RuntimeDebugObserverResource>>,
) {
    for (session_entity, session, mut flow) in sessions
        .iter_mut()
        .filter(|(_, _, flow)| flow.stage == TurnStage::Application)
    {
        for (entity, mut agent, _) in agents.iter_mut().filter(|(_, agent, owner)| {
            owner.parent() == session_entity && agent.output_type == AgentOutputType::Text
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
                    commands
                        .entity(entity)
                        .insert(NarrationOutcome {
                            turn_id: flow.active_turn_id,
                            content: output,
                        })
                        .insert(ApplicationCompleted {
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

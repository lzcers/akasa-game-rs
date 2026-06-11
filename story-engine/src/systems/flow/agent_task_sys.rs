use bevy_ecs::{
    entity::Entity,
    hierarchy::ChildOf,
    query::With,
    system::{Commands, Query, ResMut},
};

use crate::{
    components::{
        agent::{Agent, PendingReasoning},
        session_event_sink::SessionEventSink,
        turn_flow::TurnFlow,
    },
    resources::agent_task_manager::AgentTaskManager,
};

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn agent_task_system(
    mut commands: Commands,
    mut agent_tasks: ResMut<AgentTaskManager>,
    pending_agents: Query<(Entity, &Agent), With<PendingReasoning>>,
    agents: Query<(&Agent, &ChildOf)>,
    sessions: Query<(&SessionEventSink, &TurnFlow)>,
) {
    for (entity, agent) in pending_agents.iter() {
        agent_tasks.spawn_task(entity, agent.output_type, &agent.context);
        commands.entity(entity).remove::<PendingReasoning>();
    }

    agent_tasks.poll_all_tasks();
    for (agent_entity, update) in agent_tasks.take_updates() {
        let Ok((agent, owner)) = agents.get(agent_entity) else {
            continue;
        };
        let Ok((event_sink, flow)) = sessions.get(owner.parent()) else {
            continue;
        };
        let round = flow.active_turn_id.max(1);
        if let Some(chunk) = update.chunk {
            event_sink.publish_task_update(round, agent.name.clone(), chunk);
        }
        if update.status == crate::resources::agent_task_manager::TaskStatus::Done
            && let Some(output) = update.output
        {
            event_sink.publish_task_completed(round, agent.name.clone(), output);
        }
        if update.status == crate::resources::agent_task_manager::TaskStatus::Error
            && let Some(error) = update.error
        {
            event_sink.publish_flow_turn_error(round, flow.stage, agent.name.clone(), error);
        }
    }
}

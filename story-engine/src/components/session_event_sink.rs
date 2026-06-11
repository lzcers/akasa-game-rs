use agent::core::Message;
use bevy_ecs::component::Component;

use crate::{
    components::{agent::AgentOutputType, outcome::PlayerActionItem, turn_flow::TurnStage},
    resources::session_events::{
        AgentContextItemAppended, AgentContextRollback, AgentContextRollbackPolicy, EngineEvent,
        EventPipeline, FlowTurnCompleted, FlowTurnEnd, FlowTurnError, FlowTurnUpdate, PlayerInput,
        SessionCreated, SessionEventHandle, TaskCompleted, TaskUpdate,
    },
};

const ENGINE_EVENT_BUFFER: usize = 4096;

#[derive(Component)]
pub struct SessionEventSink {
    session_id: String,
    event_pipeline: EventPipeline,
}

impl SessionEventSink {
    pub fn new(session_id: impl Into<String>) -> (Self, SessionEventHandle) {
        let sink = Self {
            session_id: session_id.into(),
            event_pipeline: EventPipeline::with_buffer(ENGINE_EVENT_BUFFER),
        };
        let handle = SessionEventHandle::new(sink.event_pipeline.handle());
        (sink, handle)
    }

    pub fn publish_session_created(
        &self,
        character_name: impl Into<String>,
        world_profile: impl Into<String>,
        character_profile: impl Into<String>,
        key_story_beats: impl Into<String>,
    ) {
        let created = SessionCreated {
            session_id: self.session_id.clone(),
            character_name: character_name.into(),
            world_profile: world_profile.into(),
            character_profile: character_profile.into(),
            key_story_beats: key_story_beats.into(),
        };
        self.event_pipeline
            .publish(EngineEvent::SessionCreated(created));
    }

    pub fn publish_task_update(
        &self,
        round: u64,
        entity_name: impl Into<String>,
        chunk: impl Into<String>,
    ) {
        let update = TaskUpdate {
            session_id: self.session_id.clone(),
            round,
            entity_name: entity_name.into(),
            chunk: chunk.into(),
        };
        self.event_pipeline.publish(EngineEvent::TaskUpdate(update));
    }

    pub fn publish_task_completed(
        &self,
        round: u64,
        entity_name: impl Into<String>,
        content: impl Into<String>,
    ) {
        let completed = TaskCompleted {
            session_id: self.session_id.clone(),
            round,
            entity_name: entity_name.into(),
            content: content.into(),
        };
        self.event_pipeline
            .publish(EngineEvent::TaskCompleted(completed));
    }

    pub fn publish_player_input(&self, round: u64, actions: Vec<PlayerActionItem>) {
        let input = PlayerInput {
            session_id: self.session_id.clone(),
            round,
            actions,
        };
        self.event_pipeline.publish(EngineEvent::PlayerInput(input));
    }

    pub fn publish_agent_context_item_appended(
        &self,
        round: u64,
        agent_name: impl Into<String>,
        message: Message,
    ) {
        let update = AgentContextItemAppended {
            session_id: self.session_id.clone(),
            round,
            agent_name: agent_name.into(),
            message,
        };
        self.event_pipeline
            .publish(EngineEvent::AgentContextItemAppended(update));
    }

    pub fn publish_agent_context_rollback(
        &self,
        round: u64,
        agent_name: impl Into<String>,
        policy: AgentContextRollbackPolicy,
    ) {
        let rollback = AgentContextRollback {
            session_id: self.session_id.clone(),
            round,
            agent_name: agent_name.into(),
            policy,
        };
        self.event_pipeline
            .publish(EngineEvent::AgentContextRollback(rollback));
    }

    pub fn publish_flow_turn_update(
        &self,
        round: u64,
        stage: TurnStage,
        entity_name: impl Into<String>,
        output_type: AgentOutputType,
        content: impl Into<String>,
    ) {
        let update = FlowTurnUpdate {
            session_id: self.session_id.clone(),
            round,
            stage,
            entity_name: entity_name.into(),
            output_type,
            content: content.into(),
        };
        self.event_pipeline
            .publish(EngineEvent::FlowTurnUpdate(update));
    }

    pub fn publish_flow_turn_completed(&self, round: u64) {
        let completed = FlowTurnCompleted {
            session_id: self.session_id.clone(),
            round,
        };
        self.event_pipeline
            .publish(EngineEvent::FlowTurnCompleted(completed));
    }

    pub fn publish_flow_turn_end(&self, round: u64) {
        let end = FlowTurnEnd {
            session_id: self.session_id.clone(),
            round,
        };
        self.event_pipeline.publish(EngineEvent::FlowTurnEnd(end));
    }

    pub fn publish_flow_turn_error(
        &self,
        round: u64,
        stage: TurnStage,
        entity_name: impl Into<String>,
        msg: impl Into<String>,
    ) {
        let error = FlowTurnError {
            session_id: self.session_id.clone(),
            round,
            stage,
            entity_name: entity_name.into(),
            msg: msg.into(),
        };
        self.event_pipeline
            .publish(EngineEvent::FlowTurnError(error));
    }
}

#[cfg(test)]
mod tests {
    use agent::core::Message;

    use super::*;

    #[tokio::test]
    async fn publishes_requested_event_payloads_with_session_id() {
        let (sink, handle) = SessionEventSink::new("session-1");
        let mut events = handle.subscribe_events();

        sink.publish_session_created("洛寒", "world", "hero", "beats");
        match events.recv().await.unwrap() {
            EngineEvent::SessionCreated(created) => {
                assert_eq!(created.session_id, "session-1");
                assert_eq!(created.character_name, "洛寒");
                assert_eq!(created.world_profile, "world");
                assert_eq!(created.character_profile, "hero");
                assert_eq!(created.key_story_beats, "beats");
            }
            other => panic!("expected session created, got {other:?}"),
        }

        sink.publish_task_update(3, "UpperNarrator", "雨声");
        match events.recv().await.unwrap() {
            EngineEvent::TaskUpdate(update) => {
                assert_eq!(update.session_id, "session-1");
                assert_eq!(update.round, 3);
                assert_eq!(update.entity_name, "UpperNarrator");
                assert_eq!(update.chunk, "雨声");
            }
            other => panic!("expected task update, got {other:?}"),
        }

        sink.publish_player_input(3, vec![PlayerActionItem::character_free_text("推开门")]);
        match events.recv().await.unwrap() {
            EngineEvent::PlayerInput(input) => {
                assert_eq!(input.session_id, "session-1");
                assert_eq!(input.round, 3);
                assert_eq!(input.actions.len(), 1);
                assert_eq!(input.actions[0].action, "推开门");
            }
            other => panic!("expected player input, got {other:?}"),
        }

        sink.publish_agent_context_item_appended(
            3,
            "UpperNarrator",
            Message::user("latest context"),
        );
        match events.recv().await.unwrap() {
            EngineEvent::AgentContextItemAppended(update) => {
                assert_eq!(update.session_id, "session-1");
                assert_eq!(update.round, 3);
                assert_eq!(update.agent_name, "UpperNarrator");
                assert!(
                    matches!(update.message, Message::User { content } if content == "latest context")
                );
            }
            other => panic!("expected agent context item append, got {other:?}"),
        }

        sink.publish_agent_context_rollback(
            3,
            "UpperNarrator",
            AgentContextRollbackPolicy::LatestInput,
        );
        match events.recv().await.unwrap() {
            EngineEvent::AgentContextRollback(rollback) => {
                assert_eq!(rollback.session_id, "session-1");
                assert_eq!(rollback.round, 3);
                assert_eq!(rollback.agent_name, "UpperNarrator");
                assert_eq!(rollback.policy, AgentContextRollbackPolicy::LatestInput);
            }
            other => panic!("expected agent context rollback, got {other:?}"),
        }

        sink.publish_flow_turn_update(
            3,
            TurnStage::Application,
            "UpperNarrator",
            AgentOutputType::Text,
            "完整叙事",
        );
        match events.recv().await.unwrap() {
            EngineEvent::FlowTurnUpdate(update) => {
                assert_eq!(update.session_id, "session-1");
                assert_eq!(update.round, 3);
                assert_eq!(update.stage, TurnStage::Application);
                assert_eq!(update.entity_name, "UpperNarrator");
                assert_eq!(update.output_type, AgentOutputType::Text);
                assert_eq!(update.content, "完整叙事");
            }
            other => panic!("expected flow turn update, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn publishes_repeated_payloads_as_distinct_stream_events() {
        let (sink, handle) = SessionEventSink::new("session-1");
        let mut events = handle.subscribe_events();

        sink.publish_task_update(1, "UpperNarrator", "...");
        match events.recv().await.unwrap() {
            EngineEvent::TaskUpdate(update) => {
                assert_eq!(update.chunk, "...");
            }
            other => panic!("expected task update, got {other:?}"),
        }

        sink.publish_task_update(1, "UpperNarrator", "...");
        match events.recv().await.unwrap() {
            EngineEvent::TaskUpdate(update) => {
                assert_eq!(update.chunk, "...");
            }
            other => panic!("expected repeated task update, got {other:?}"),
        }
    }
}

use agent::agent::Context;
use tokio::sync::broadcast;

use crate::components::{agent::AgentOutputType, outcome::PlayerActionType, turn_flow::TurnStage};

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    SessionCreated(SessionCreated),
    TaskUpdate(TaskUpdate),
    TaskCompleted(TaskCompleted),
    PlayerInput(PlayerInput),
    AgentContextUpdate(AgentContextUpdate),
    FlowTurnUpdate(FlowTurnUpdate),
    FlowTurnCompleted(FlowTurnCompleted),
    FlowTurnEnd(FlowTurnEnd),
    FlowTurnError(FlowTurnError),
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct SessionCreated {
    pub session_id: String,
    pub world_profile: String,
    pub protagonist_profile: String,
    pub key_story_beats: String,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TaskUpdate {
    pub session_id: String,
    pub round: u64,
    pub entity_name: String,
    pub chunk: String,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TaskCompleted {
    pub session_id: String,
    pub round: u64,
    pub entity_name: String,
    pub content: String,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PlayerInput {
    pub session_id: String,
    pub round: u64,
    pub action_type: PlayerActionType,
    pub action: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentContextUpdate {
    pub session_id: String,
    pub round: u64,
    pub agent_name: String,
    pub context: Context,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct FlowTurnUpdate {
    pub session_id: String,
    pub round: u64,
    pub stage: TurnStage,
    pub entity_name: String,
    pub output_type: AgentOutputType,
    pub content: String,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct FlowTurnCompleted {
    pub session_id: String,
    pub round: u64,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct FlowTurnEnd {
    pub session_id: String,
    pub round: u64,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct FlowTurnError {
    pub session_id: String,
    pub round: u64,
    pub stage: TurnStage,
    pub entity_name: String,
    pub msg: String,
}

#[derive(Clone)]
pub struct EventPipeline {
    event_tx: broadcast::Sender<EngineEvent>,
}

#[derive(Clone)]
pub struct EventPipelineHandle {
    event_tx: broadcast::Sender<EngineEvent>,
}

impl EventPipeline {
    pub fn with_buffer(event_buffer: usize) -> Self {
        let (event_tx, _) = broadcast::channel(event_buffer.max(1));
        Self { event_tx }
    }

    pub fn handle(&self) -> EventPipelineHandle {
        EventPipelineHandle {
            event_tx: self.event_tx.clone(),
        }
    }

    pub fn publish(&self, event: EngineEvent) {
        let _ = self.event_tx.send(event);
    }
}

impl EventPipelineHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.event_tx.subscribe()
    }
}

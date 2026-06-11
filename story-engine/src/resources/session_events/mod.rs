mod event_pipeline;

pub use event_pipeline::{
    AgentContextUpdate, EngineEvent, FlowTurnCompleted, FlowTurnEnd, FlowTurnError, FlowTurnUpdate,
    PlayerInput, SessionCreated, TaskCompleted, TaskUpdate,
};
pub(crate) use event_pipeline::{EventPipeline, EventPipelineHandle};

#[derive(Clone)]
pub struct SessionEventHandle {
    event_pipeline: EventPipelineHandle,
}

impl SessionEventHandle {
    pub(crate) fn new(event_pipeline: EventPipelineHandle) -> Self {
        Self { event_pipeline }
    }

    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<EngineEvent> {
        self.event_pipeline.subscribe()
    }
}

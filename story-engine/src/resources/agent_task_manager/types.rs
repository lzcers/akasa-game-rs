use bevy_ecs::entity::Entity;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Done,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskChunkKind {
    Text,
    Reasoning,
}

#[derive(Clone, Debug)]
pub struct TaskResult {
    pub status: TaskStatus,
    pub attempts: usize,
    pub max_attempts: usize,
    pub last_error: Option<String>,
    pub chunks: Vec<String>,
    pub output: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskUpdate {
    pub entity: String,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_kind: Option<TaskChunkKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TaskUpdate {
    pub(super) fn pending(entity: Entity, error: Option<String>) -> Self {
        Self::new(entity, TaskStatus::Pending).with_error(error)
    }

    pub(super) fn running(entity: Entity, error: Option<String>) -> Self {
        Self::new(entity, TaskStatus::Running).with_error(error)
    }

    pub(super) fn chunk(entity: Entity, kind: TaskChunkKind, content: String) -> Self {
        Self::new(entity, TaskStatus::Running).with_chunk(kind, content)
    }

    pub(super) fn done(entity: Entity, output: String) -> Self {
        Self::new(entity, TaskStatus::Done).with_output(output)
    }

    pub(super) fn failed(entity: Entity, error: String) -> Self {
        Self::new(entity, TaskStatus::Error).with_error(Some(error))
    }

    fn new(entity: Entity, status: TaskStatus) -> Self {
        Self {
            entity: task_entity_label(entity),
            status,
            chunk_kind: None,
            chunk: None,
            output: None,
            error: None,
        }
    }

    fn with_chunk(mut self, kind: TaskChunkKind, content: String) -> Self {
        self.chunk_kind = Some(kind);
        self.chunk = Some(content);
        self
    }

    fn with_output(mut self, output: String) -> Self {
        self.output = Some(output);
        self
    }

    fn with_error(mut self, error: Option<String>) -> Self {
        self.error = error;
        self
    }
}

impl TaskResult {
    pub(super) fn pending(max_attempts: usize) -> Self {
        Self {
            status: TaskStatus::Pending,
            attempts: 1,
            max_attempts,
            last_error: None,
            chunks: Vec::new(),
            output: None,
            error: None,
        }
    }

    pub(super) fn mark_running(&mut self) {
        self.status = TaskStatus::Running;
    }

    pub(super) fn mark_done(&mut self, content: String) {
        self.status = TaskStatus::Done;
        self.last_error = None;
        self.output = Some(content);
        self.error = None;
    }

    pub(super) fn mark_retrying(&mut self, message: String) {
        self.status = TaskStatus::Running;
        self.last_error = Some(message);
        self.chunks.clear();
        self.output = None;
        self.error = None;
    }

    pub(super) fn mark_failed(&mut self, message: String) {
        self.status = TaskStatus::Error;
        self.last_error = Some(message.clone());
        self.output = None;
        self.error = Some(message);
    }
}

fn task_entity_label(entity: Entity) -> String {
    format!("{entity:?}")
}

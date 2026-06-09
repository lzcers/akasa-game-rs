use std::{collections::HashMap, env};

use agent::{
    agent::{CallModelEvent, Context, call_model},
    core::Message,
    models::ChatModel,
};
use bevy_ecs::{entity::Entity, resource::Resource};
use futures::StreamExt;
use serde::Serialize;
use tokio::{
    sync::mpsc::{self, UnboundedReceiver, error::TryRecvError},
    task::JoinHandle,
    time::{Duration, Instant, timeout},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Simulation,
    ProtagonistAction,
    Narration,
}

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
    pub kind: TaskKind,
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
    pub kind: TaskKind,
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

#[derive(Resource)]
pub struct AgentTaskManager {
    model: ChatModel,
    config: AgentTaskConfig,
    tasks: HashMap<Entity, RunningTask>,
    task_specs: HashMap<Entity, TaskSpec>,
    results: HashMap<Entity, TaskResult>,
    emitted_updates: Vec<(Entity, TaskUpdate)>,
}

#[derive(Clone)]
struct TaskSpec {
    model: ChatModel,
    messages: Vec<Message>,
}

struct RunningTask {
    runtime: TaskRuntime,
}

struct TaskRuntime {
    rx: UnboundedReceiver<TaskRuntimeEvent>,
    handle: JoinHandle<()>,
}

#[derive(Clone, Debug)]
enum TaskRuntimeEvent {
    Chunk {
        kind: TaskChunkKind,
        content: String,
    },
    Completed(String),
    Failed(String),
}

#[derive(Clone, Copy, Debug)]
pub struct AgentTaskConfig {
    task_timeout: Duration,
    initial_output_timeout: Duration,
    no_progress_timeout: Duration,
    max_retries: usize,
}

const DEFAULT_TASK_TIMEOUT_SECS: u64 = 300;
const DEFAULT_INITIAL_OUTPUT_TIMEOUT_SECS: u64 = 45;
const DEFAULT_NO_PROGRESS_TIMEOUT_SECS: u64 = 90;
const DEFAULT_MAX_RETRIES: usize = 2;

impl Default for AgentTaskConfig {
    fn default() -> Self {
        Self {
            task_timeout: Duration::from_secs(DEFAULT_TASK_TIMEOUT_SECS),
            initial_output_timeout: Duration::from_secs(DEFAULT_INITIAL_OUTPUT_TIMEOUT_SECS),
            no_progress_timeout: Duration::from_secs(DEFAULT_NO_PROGRESS_TIMEOUT_SECS),
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }
}

impl AgentTaskConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            task_timeout: env_duration_secs("AKASHIC_TASK_TIMEOUT_SECS", defaults.task_timeout),
            initial_output_timeout: env_duration_secs(
                "AKASHIC_TASK_INITIAL_OUTPUT_TIMEOUT_SECS",
                defaults.initial_output_timeout,
            ),
            no_progress_timeout: env_duration_secs(
                "AKASHIC_TASK_NO_PROGRESS_TIMEOUT_SECS",
                defaults.no_progress_timeout,
            ),
            max_retries: env_usize("AKASHIC_TASK_MAX_RETRIES", defaults.max_retries),
        }
    }

    fn max_attempts(self) -> usize {
        self.max_retries.saturating_add(1).max(1)
    }
}

impl AgentTaskManager {
    pub fn new(model: ChatModel) -> Self {
        Self::with_config(model, AgentTaskConfig::from_env())
    }

    pub fn with_config(model: ChatModel, config: AgentTaskConfig) -> Self {
        Self {
            model,
            config,
            tasks: HashMap::new(),
            task_specs: HashMap::new(),
            results: HashMap::new(),
            emitted_updates: Vec::new(),
        }
    }

    pub fn spawn_task(&mut self, entity: Entity, kind: TaskKind, ctx: &Context) {
        if let Some(existing_task) = self.tasks.remove(&entity) {
            existing_task.runtime.handle.abort();
        }

        let model = self.model_for_kind(kind);
        let messages = ctx.to_messages();
        let spec = TaskSpec { model, messages };
        self.results.insert(
            entity,
            TaskResult::pending(kind, self.config.max_attempts()),
        );
        self.task_specs.insert(entity, spec.clone());
        self.emitted_updates.push((
            entity,
            TaskUpdate {
                entity: task_entity_label(entity),
                kind,
                status: TaskStatus::Pending,
                chunk_kind: None,
                chunk: None,
                output: None,
                error: None,
            },
        ));
        self.tasks.insert(
            entity,
            RunningTask {
                runtime: Self::spawn_runtime_task(
                    spec.model.clone(),
                    spec.messages.clone(),
                    self.config,
                ),
            },
        );
    }

    fn model_for_kind(&self, kind: TaskKind) -> ChatModel {
        let mut model = self.model.clone();
        model.set_output_json(matches!(
            kind,
            TaskKind::Simulation | TaskKind::ProtagonistAction
        ));
        model
    }

    pub fn poll_all_tasks(&mut self) {
        let task_entities: Vec<Entity> = self.tasks.keys().copied().collect();
        for entity in task_entities {
            let _ = self.poll_task(entity);
        }
    }

    pub fn poll_task(&mut self, entity: Entity) -> TaskStatus {
        if !self.results.contains_key(&entity) {
            return TaskStatus::Error;
        }

        if let Some(status) = self.results.get(&entity).and_then(|result| {
            matches!(result.status, TaskStatus::Done | TaskStatus::Error).then_some(result.status)
        }) {
            return status;
        }

        if !self.tasks.contains_key(&entity) {
            let error = "task handle missing".to_string();
            self.mark_task_failed(entity, error);
            return TaskStatus::Error;
        };

        if self
            .results
            .get(&entity)
            .is_some_and(|result| result.status != TaskStatus::Running)
        {
            let result = self
                .results
                .get_mut(&entity)
                .expect("task result should exist after contains check");
            result.mark_running();
            self.emitted_updates.push((
                entity,
                TaskUpdate {
                    entity: task_entity_label(entity),
                    kind: result.kind,
                    status: TaskStatus::Running,
                    chunk_kind: None,
                    chunk: None,
                    output: None,
                    error: None,
                },
            ));
        }

        let mut completed = false;
        let mut failed = None;
        {
            let result = self
                .results
                .get_mut(&entity)
                .expect("task result should exist while polling");
            let task = self
                .tasks
                .get_mut(&entity)
                .expect("task runtime should exist while polling");
            loop {
                match task.runtime.rx.try_recv() {
                    Ok(TaskRuntimeEvent::Chunk { kind, content }) => {
                        result.chunks.push(content.clone());
                        self.emitted_updates.push((
                            entity,
                            TaskUpdate {
                                entity: task_entity_label(entity),
                                kind: result.kind,
                                status: TaskStatus::Running,
                                chunk_kind: Some(kind),
                                chunk: Some(content),
                                output: None,
                                error: None,
                            },
                        ));
                    }
                    Ok(TaskRuntimeEvent::Completed(content)) => {
                        result.mark_done(content.clone());
                        self.emitted_updates.push((
                            entity,
                            TaskUpdate {
                                entity: task_entity_label(entity),
                                kind: result.kind,
                                status: TaskStatus::Done,
                                chunk_kind: None,
                                chunk: None,
                                output: Some(content),
                                error: None,
                            },
                        ));
                        completed = true;
                        break;
                    }
                    Ok(TaskRuntimeEvent::Failed(error)) => {
                        failed = Some(error);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        failed = Some("task runtime ended without completion".to_string());
                        break;
                    }
                }
            }
        }

        if completed {
            self.tasks.remove(&entity);
            return TaskStatus::Done;
        }

        if let Some(error) = failed {
            if self.retry_task(entity, error.clone()) {
                return TaskStatus::Running;
            }
            self.mark_task_failed(entity, error);
            return TaskStatus::Error;
        }

        TaskStatus::Running
    }

    pub fn task_result(&self, entity: Entity) -> Option<&TaskResult> {
        self.results.get(&entity)
    }

    pub fn take_result(&mut self, entity: Entity) -> Option<TaskResult> {
        self.task_specs.remove(&entity);
        self.results.remove(&entity)
    }

    pub fn clear_task(&mut self, entity: Entity) {
        if let Some(task) = self.tasks.remove(&entity) {
            task.runtime.handle.abort();
        }
        self.task_specs.remove(&entity);
        self.results.remove(&entity);
    }

    pub fn retry_task(&mut self, entity: Entity, reason: String) -> bool {
        let Some(spec) = self.task_specs.get(&entity).cloned() else {
            return false;
        };
        let Some(result) = self.results.get_mut(&entity) else {
            return false;
        };
        if result.attempts >= result.max_attempts {
            return false;
        }

        if let Some(task) = self.tasks.remove(&entity) {
            task.runtime.handle.abort();
        }

        result.attempts += 1;
        result.mark_retrying(reason.clone());
        let retry_message = format!(
            "retrying attempt {}/{} after: {reason}",
            result.attempts, result.max_attempts
        );
        self.emitted_updates.push((
            entity,
            TaskUpdate {
                entity: task_entity_label(entity),
                kind: result.kind,
                status: TaskStatus::Pending,
                chunk_kind: None,
                chunk: None,
                output: None,
                error: Some(retry_message.clone()),
            },
        ));
        self.emitted_updates.push((
            entity,
            TaskUpdate {
                entity: task_entity_label(entity),
                kind: result.kind,
                status: TaskStatus::Running,
                chunk_kind: None,
                chunk: None,
                output: None,
                error: Some(retry_message),
            },
        ));
        self.tasks.insert(
            entity,
            RunningTask {
                runtime: Self::spawn_runtime_task(
                    spec.model.clone(),
                    spec.messages.clone(),
                    self.config,
                ),
            },
        );
        true
    }

    pub fn take_updates(&mut self) -> Vec<(Entity, TaskUpdate)> {
        std::mem::take(&mut self.emitted_updates)
    }

    #[cfg(test)]
    pub fn set_result_for_test(&mut self, entity: Entity, result: TaskResult) {
        self.results.insert(entity, result);
    }

    fn spawn_runtime_task(
        model: ChatModel,
        msgs: Vec<Message>,
        config: AgentTaskConfig,
    ) -> TaskRuntime {
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = tokio::spawn(async move {
            let mut stream = Box::pin(call_model(&model, &msgs, None));
            let started_at = Instant::now();
            let mut saw_output = false;
            loop {
                let Some(total_remaining) = config.task_timeout.checked_sub(started_at.elapsed())
                else {
                    let _ = tx.send(TaskRuntimeEvent::Failed(format!(
                        "model task exceeded {} seconds",
                        config.task_timeout.as_secs()
                    )));
                    return;
                };
                let progress_timeout = if saw_output {
                    config.no_progress_timeout
                } else {
                    config.initial_output_timeout
                };
                let wait_timeout = progress_timeout.min(total_remaining);

                let event = match timeout(wait_timeout, stream.next()).await {
                    Ok(Some(event)) => event,
                    Ok(None) => {
                        let _ = tx.send(TaskRuntimeEvent::Failed(
                            "task ended without completion".to_string(),
                        ));
                        return;
                    }
                    Err(_) => {
                        let timeout_name = if saw_output {
                            "no progress"
                        } else {
                            "initial output"
                        };
                        let _ = tx.send(TaskRuntimeEvent::Failed(format!(
                            "model stream hit {timeout_name} timeout after {} seconds",
                            wait_timeout.as_secs()
                        )));
                        return;
                    }
                };

                match event {
                    CallModelEvent::TextChunk(content) => {
                        saw_output = true;
                        let _ = tx.send(TaskRuntimeEvent::Chunk {
                            kind: TaskChunkKind::Text,
                            content,
                        });
                    }
                    CallModelEvent::ReasoningChunk(content) => {
                        saw_output = true;
                        let _ = tx.send(TaskRuntimeEvent::Chunk {
                            kind: TaskChunkKind::Reasoning,
                            content,
                        });
                    }
                    CallModelEvent::Completed { content, .. } => {
                        let _ = tx.send(TaskRuntimeEvent::Completed(content));
                        return;
                    }
                    CallModelEvent::Error(error) => {
                        let _ = tx.send(TaskRuntimeEvent::Failed(error));
                        return;
                    }
                }
            }
        });

        TaskRuntime { rx, handle }
    }

    fn mark_task_failed(&mut self, entity: Entity, error: String) {
        if let Some(task) = self.tasks.remove(&entity) {
            task.runtime.handle.abort();
        }
        self.task_specs.remove(&entity);
        let Some(result) = self.results.get_mut(&entity) else {
            return;
        };
        result.mark_failed(error.clone());
        self.emitted_updates.push((
            entity,
            TaskUpdate {
                entity: task_entity_label(entity),
                kind: result.kind,
                status: TaskStatus::Error,
                chunk_kind: None,
                chunk: None,
                output: None,
                error: Some(error),
            },
        ));
    }
}

impl TaskResult {
    fn pending(kind: TaskKind, max_attempts: usize) -> Self {
        Self {
            kind,
            status: TaskStatus::Pending,
            attempts: 1,
            max_attempts,
            last_error: None,
            chunks: Vec::new(),
            output: None,
            error: None,
        }
    }

    fn mark_running(&mut self) {
        self.status = TaskStatus::Running;
    }

    fn mark_done(&mut self, content: String) {
        self.status = TaskStatus::Done;
        self.last_error = None;
        self.output = Some(content);
        self.error = None;
    }

    fn mark_retrying(&mut self, message: String) {
        self.status = TaskStatus::Running;
        self.last_error = Some(message);
        self.chunks.clear();
        self.output = None;
        self.error = None;
    }

    fn mark_failed(&mut self, message: String) {
        self.status = TaskStatus::Error;
        self.last_error = Some(message.clone());
        self.output = None;
        self.error = Some(message);
    }
}

fn task_entity_label(entity: Entity) -> String {
    format!("{entity:?}")
}

fn env_duration_secs(name: &str, default: Duration) -> Duration {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(Duration::from_secs)
        .unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

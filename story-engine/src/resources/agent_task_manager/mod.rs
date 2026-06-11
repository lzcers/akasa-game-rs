mod config;
mod runtime;
mod types;

use std::collections::HashMap;

use agent::{agent::Context, core::Message, models::ChatModel};
use bevy_ecs::{entity::Entity, resource::Resource};
use tokio::sync::mpsc::error::TryRecvError;

use crate::components::agent::AgentOutputType;

pub use config::AgentTaskConfig;
use runtime::{RunningTask, TaskRuntimeEvent, spawn_runtime_task};
pub use types::{TaskChunkKind, TaskResult, TaskStatus, TaskUpdate};

#[derive(Resource)]
pub struct AgentTaskManager {
    model: ChatModel,
    config: AgentTaskConfig,
    tasks: HashMap<Entity, RunningTask>,
    task_specs: HashMap<Entity, TaskSpec>,
    results: HashMap<Entity, TaskResult>,
    emitted_updates: Vec<(Entity, TaskUpdate)>,
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

    pub fn spawn_task(&mut self, entity: Entity, output_type: AgentOutputType, ctx: &Context) {
        if let Some(existing_task) = self.tasks.remove(&entity) {
            existing_task.runtime.handle.abort();
        }

        let model = self.model_for_output_type(output_type);
        let spec = TaskSpec {
            model,
            messages: ctx.to_messages(),
        };
        self.results
            .insert(entity, TaskResult::pending(self.config.max_attempts()));
        self.task_specs.insert(entity, spec.clone());
        self.emit_update(entity, TaskUpdate::pending(entity, None));
        self.start_runtime_task(entity, &spec);
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
            self.mark_task_failed(entity, "task handle missing".to_string());
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
            self.emit_update(entity, TaskUpdate::running(entity, None));
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
                        self.emitted_updates
                            .push((entity, TaskUpdate::chunk(entity, kind, content)));
                    }
                    Ok(TaskRuntimeEvent::Completed(content)) => {
                        result.mark_done(content.clone());
                        self.emitted_updates
                            .push((entity, TaskUpdate::done(entity, content)));
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
        self.emit_update(
            entity,
            TaskUpdate::pending(entity, Some(retry_message.clone())),
        );
        self.emit_update(entity, TaskUpdate::running(entity, Some(retry_message)));
        self.start_runtime_task(entity, &spec);
        true
    }

    pub fn take_updates(&mut self) -> Vec<(Entity, TaskUpdate)> {
        std::mem::take(&mut self.emitted_updates)
    }

    #[cfg(test)]
    pub fn set_result_for_test(&mut self, entity: Entity, result: TaskResult) {
        self.results.insert(entity, result);
    }

    fn model_for_output_type(&self, output_type: AgentOutputType) -> ChatModel {
        let mut model = self.model.clone();
        model.set_output_json(output_type == AgentOutputType::Json);
        model
    }

    fn start_runtime_task(&mut self, entity: Entity, spec: &TaskSpec) {
        self.tasks.insert(
            entity,
            RunningTask {
                runtime: spawn_runtime_task(spec.model.clone(), spec.messages.clone(), self.config),
            },
        );
    }

    fn emit_update(&mut self, entity: Entity, update: TaskUpdate) {
        self.emitted_updates.push((entity, update));
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
        self.emit_update(entity, TaskUpdate::failed(entity, error));
    }
}

#[derive(Clone)]
pub(super) struct TaskSpec {
    model: ChatModel,
    messages: Vec<Message>,
}

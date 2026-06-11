use agent::{
    agent::{CallModelEvent, call_model},
    core::Message,
    models::ChatModel,
};
use futures::StreamExt;
use tokio::{
    sync::mpsc::{self, UnboundedReceiver},
    task::JoinHandle,
    time::{Instant, timeout},
};

use super::{AgentTaskConfig, TaskChunkKind};

pub(super) struct RunningTask {
    pub(super) runtime: TaskRuntime,
}

pub(super) struct TaskRuntime {
    pub(super) rx: UnboundedReceiver<TaskRuntimeEvent>,
    pub(super) handle: JoinHandle<()>,
}

#[derive(Clone, Debug)]
pub(super) enum TaskRuntimeEvent {
    Chunk {
        kind: TaskChunkKind,
        content: String,
    },
    Completed(String),
    Failed(String),
}

pub(super) fn spawn_runtime_task(
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

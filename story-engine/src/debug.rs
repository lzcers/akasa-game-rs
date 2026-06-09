use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use agent::{agent::Context, core::Message};

use crate::{
    engine::RuntimeDebugObserver,
    resources::agent_task::{TaskChunkKind, TaskKind, TaskStatus, TaskUpdate},
};

const FATE_WEAVER_CONTEXT_FILE: &str = "fate_weaver_context.md";
const PROTAGONIST_CONTEXT_FILE: &str = "protagonist_context.md";
const UPPER_NARRATOR_CONTEXT_FILE: &str = "upper_narrator_context.md";

pub struct LocalDebugObserver {
    output_dir: PathBuf,
    streams: Mutex<OrderedTaskStreams>,
}

impl LocalDebugObserver {
    pub fn for_workspace_root() -> Self {
        Self::new(workspace_root())
    }

    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            streams: Mutex::new(OrderedTaskStreams::default()),
        }
    }

    fn export_context(
        &self,
        session_id: &str,
        turn_index: u64,
        active_turn_id: u64,
        agent: &str,
        context: &Context,
    ) {
        let Some(file_name) = context_file_name(agent) else {
            return;
        };

        self.write_context(
            file_name,
            ExportedAgentContext {
                session_id,
                turn_index,
                active_turn_id,
                agent,
                context,
            },
        );
    }

    fn write_context(&self, file_name: &str, payload: ExportedAgentContext<'_>) {
        let content = format_context_markdown(payload);
        let path = self.output_dir.join(file_name);
        if let Err(error) = write_file(&path, content) {
            eprintln!("[local-debug] failed to write {}: {error}", path.display());
        }
    }
}

struct ExportedAgentContext<'a> {
    session_id: &'a str,
    turn_index: u64,
    active_turn_id: u64,
    agent: &'a str,
    context: &'a Context,
}

fn format_context_markdown(payload: ExportedAgentContext<'_>) -> String {
    let mut out = String::new();
    out.push_str("# Agent Context\n\n");
    out.push_str("## Metadata\n\n");
    out.push_str(&format!("- Agent: `{}`\n", payload.agent));
    out.push_str(&format!("- Session: `{}`\n", payload.session_id));
    out.push_str(&format!("- Turn index: `{}`\n", payload.turn_index));
    out.push_str(&format!(
        "- Active turn id: `{}`\n\n",
        payload.active_turn_id
    ));

    out.push_str("## Model Messages\n\n");
    let messages = payload.context.to_messages();
    if messages.is_empty() {
        out.push_str("_No model messages._\n\n");
    } else {
        for (index, message) in messages.iter().enumerate() {
            out.push_str(&format!(
                "### {}. {}\n\n",
                index + 1,
                message_role_label(message)
            ));
            if let Some(reasoning) = message
                .reasoning_content()
                .filter(|text| !text.trim().is_empty())
            {
                out.push_str("#### Reasoning\n\n");
                out.push_str(&fenced_block(reasoning));
                out.push('\n');
            }
            out.push_str(&fenced_block(message.content()));
            out.push('\n');
        }
    }

    out.push_str("## Context Layers\n\n");
    if payload.context.layers.is_empty() {
        out.push_str("_No context layers._\n");
    } else {
        for (index, layer) in payload.context.layers.iter().enumerate() {
            out.push_str(&format!(
                "### {}. {} ({:?})\n\n",
                index + 1,
                layer.name,
                layer.kind
            ));
            out.push_str(&format!(
                "- Priority: `{}`\n- Readonly: `{}`\n",
                layer.meta.priority, layer.meta.readonly
            ));
            if let Some(source) = &layer.meta.source {
                out.push_str(&format!("- Source: `{source}`\n"));
            }
            if !layer.meta.tags.is_empty() {
                out.push_str(&format!("- Tags: `{}`\n", layer.meta.tags.join(", ")));
            }
            out.push('\n');
        }
    }

    out
}

fn message_role_label(message: &Message) -> &'static str {
    match message {
        Message::System { .. } => "system",
        Message::User { .. } => "user",
        Message::Assistant { .. } => "assistant",
        Message::Tool { .. } => "tool",
    }
}

fn fenced_block(content: &str) -> String {
    let fence = if content.contains("```") {
        "````"
    } else {
        "```"
    };
    format!("{fence}\n{content}\n{fence}\n")
}

impl RuntimeDebugObserver for LocalDebugObserver {
    fn on_task_update(&self, session_id: &str, _round: u64, update: &TaskUpdate) {
        let Ok(mut streams) = self.streams.lock() else {
            eprintln!("[local-debug] stream printer lock is poisoned");
            return;
        };
        streams.handle(session_id, update.clone());
    }

    fn on_agent_context_updated(
        &self,
        session_id: &str,
        turn_index: u64,
        active_turn_id: u64,
        agent_name: &str,
        context: &Context,
    ) {
        self.export_context(session_id, turn_index, active_turn_id, agent_name, context);
    }
}

fn context_file_name(agent: &str) -> Option<&'static str> {
    match agent {
        "FateWeaver" => Some(FATE_WEAVER_CONTEXT_FILE),
        "Protagonist" => Some(PROTAGONIST_CONTEXT_FILE),
        "UpperNarrator" => Some(UPPER_NARRATOR_CONTEXT_FILE),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TaskStreamKey {
    session_id: String,
    entity: String,
    kind: TaskKind,
}

#[derive(Default)]
struct BufferedTaskStream {
    chunks: VecDeque<(TaskChunkKind, String)>,
    printed_kind: Option<TaskChunkKind>,
    completed: bool,
    error: Option<String>,
}

#[derive(Default)]
struct OrderedTaskStreams {
    order: VecDeque<TaskStreamKey>,
    streams: HashMap<TaskStreamKey, BufferedTaskStream>,
}

impl OrderedTaskStreams {
    fn handle(&mut self, session_id: &str, update: TaskUpdate) {
        let key = TaskStreamKey {
            session_id: session_id.to_string(),
            entity: update.entity,
            kind: update.kind,
        };

        if !self.streams.contains_key(&key) {
            self.order.push_back(key.clone());
            self.streams
                .insert(key.clone(), BufferedTaskStream::default());
        }

        let stream = self
            .streams
            .get_mut(&key)
            .expect("task stream must exist after insertion");
        let status = update.status;
        if let (Some(kind), Some(chunk)) = (update.chunk_kind, update.chunk) {
            stream.chunks.push_back((kind, chunk));
        }
        if let Some(error) = update.error {
            match status {
                TaskStatus::Error => stream.error = Some(error),
                TaskStatus::Pending | TaskStatus::Running => eprintln!(
                    "[stream retry {} {:?} {}] {error}",
                    key.session_id, key.kind, key.entity
                ),
                TaskStatus::Done => {}
            }
        }
        if status == TaskStatus::Done {
            stream.error = None;
        }
        stream.completed = matches!(status, TaskStatus::Done | TaskStatus::Error);

        self.flush();
    }

    fn flush(&mut self) {
        while let Some(key) = self.order.front().cloned() {
            let Some(stream) = self.streams.get_mut(&key) else {
                self.order.pop_front();
                continue;
            };

            while let Some((chunk_kind, chunk)) = stream.chunks.pop_front() {
                if stream.printed_kind != Some(chunk_kind) {
                    if stream.printed_kind.is_some() {
                        println!();
                    }
                    println!(
                        "\n[stream {} {:?} {} {:?}]",
                        key.session_id, key.kind, key.entity, chunk_kind
                    );
                    stream.printed_kind = Some(chunk_kind);
                }
                print!("{chunk}");
                let _ = io::stdout().flush();
            }

            if !stream.completed {
                break;
            }

            if stream.printed_kind.is_some() {
                println!();
            }
            if let Some(error) = &stream.error {
                eprintln!(
                    "[stream error {} {:?} {}] {error}",
                    key.session_id, key.kind, key.entity
                );
            }
            self.streams.remove(&key);
            self.order.pop_front();
        }
    }
}

fn write_file(path: &Path, content: String) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

use std::{
    collections::HashMap,
    env,
    error::Error,
    io::{self, Write},
    thread,
    time::Duration,
};

use story_engine::{
    AkashicEngine,
    components::{
        agent::AgentOutputType,
        outcome::{PlayerActionInput, PlayerActionType, ProtagonistOptions},
    },
    resources::session_events::EngineEvent,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();

    if env::var("DEEPSEEK_API_KEY").is_err() {
        eprintln!("请先设置 DEEPSEEK_API_KEY，再运行 `cargo run -p story_engine`");
        return Ok(());
    }

    let max_turns = env_usize("AKASHIC_STORY_ENGINE_MAX_TURNS", 200);
    let engine = AkashicEngine::new();
    let session = engine.create_default_session("story-engine-main").await?;
    let mut events = session.subscribe_session_events();
    session.start_next_turn()?;

    println!("== Akashic Story Engine ==");
    println!("[api] story_engine event loop started, max_turns={max_turns}");

    let mut completed_turns = 0;
    let mut last_completed_turn = None;
    let mut stream_output = StreamTypewriterOutput::new(Duration::from_millis(env_u64(
        "AKASHIC_STORY_ENGINE_TYPEWRITER_MS",
        1,
    )));
    let mut auto_choices = HashMap::<u64, AutoChoiceGate>::new();

    loop {
        match events.recv().await? {
            EngineEvent::TaskUpdate(update) => {
                stream_output.write_chunk(update.round, &update.entity_name, &update.chunk)?;
            }
            EngineEvent::TaskCompleted(completed) => {
                stream_output.complete_task(
                    completed.round,
                    &completed.entity_name,
                    &completed.content,
                )?;
                println!(
                    "[task] round={} entity={} completed",
                    completed.round, completed.entity_name
                );
            }
            EngineEvent::FlowTurnUpdate(update) => {
                stream_output.finish_line()?;
                println!(
                    "[turn] round={} stage={:?} entity={} output={:?}",
                    update.round, update.stage, update.entity_name, update.output_type
                );
                let choice_to_submit = {
                    let gate = auto_choices.entry(update.round).or_default();
                    gate.record_update(&update.entity_name, update.output_type, &update.content);
                    gate.take_ready_choice()
                };
                if let Some(content) = choice_to_submit {
                    submit_first_choice_or_continue(&session, &content)?;
                }
            }
            EngineEvent::PlayerInput(input) => {
                stream_output.finish_line()?;
                println!(
                    "[player] round={} type={:?} action={}",
                    input.round, input.action_type, input.action
                );
            }
            EngineEvent::FlowTurnCompleted(completed) => {
                stream_output.finish_line()?;
                if last_completed_turn == Some(completed.round) {
                    continue;
                }
                last_completed_turn = Some(completed.round);
                completed_turns += 1;
                println!(
                    "[api] turn complete: round={} completed_turns={}/{}",
                    completed.round, completed_turns, max_turns
                );
                if completed_turns >= max_turns {
                    println!("[api] reached max turns, exiting");
                    return Ok(());
                }
                auto_choices.remove(&completed.round);
                session.start_next_turn()?;
            }
            EngineEvent::FlowTurnEnd(end) => {
                stream_output.finish_line()?;
                println!("[api] story ended at round={}", end.round);
                return Ok(());
            }
            EngineEvent::FlowTurnError(error) => {
                stream_output.finish_line()?;
                return Err(format!(
                    "故事引擎在第 {} 轮 {:?}/{} 失败：{}",
                    error.round, error.stage, error.entity_name, error.msg
                )
                .into());
            }
            EngineEvent::SessionCreated(created) => {
                stream_output.finish_line()?;
                println!(
                    "[api] session created: {} world={} protagonist={}",
                    created.session_id, created.world_profile, created.protagonist_profile
                );
            }
            EngineEvent::AgentContextUpdate(update) => {
                stream_output.finish_line()?;
                println!(
                    "[context] round={} agent={} messages={}",
                    update.round,
                    update.agent_name,
                    update.context.to_messages().len()
                );
            }
        }
    }
}

#[derive(Default)]
struct AutoChoiceGate {
    protagonist_options: Option<String>,
    protagonist_updated: bool,
    narrator_updated: bool,
    submitted: bool,
}

impl AutoChoiceGate {
    fn record_update(&mut self, entity_name: &str, output_type: AgentOutputType, content: &str) {
        if entity_name == "Protagonist" && output_type == AgentOutputType::Json {
            self.protagonist_options = Some(content.to_string());
            self.protagonist_updated = true;
        }

        if entity_name == "UpperNarrator" && output_type == AgentOutputType::Text {
            self.narrator_updated = true;
        }
    }

    fn take_ready_choice(&mut self) -> Option<String> {
        if self.submitted || !self.protagonist_updated || !self.narrator_updated {
            return None;
        }
        self.submitted = true;
        self.protagonist_options.clone()
    }
}

struct StreamTypewriterOutput {
    current: Option<StreamOutputKey>,
    buffers: HashMap<StreamOutputKey, String>,
    char_delay: Duration,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct StreamOutputKey {
    round: u64,
    entity_name: String,
}

impl StreamTypewriterOutput {
    fn new(char_delay: Duration) -> Self {
        Self {
            current: None,
            buffers: HashMap::new(),
            char_delay,
        }
    }

    fn write_chunk(&mut self, round: u64, entity_name: &str, chunk: &str) -> io::Result<()> {
        if chunk.trim().is_empty() {
            return Ok(());
        }

        if should_typewrite(entity_name) {
            self.buffers
                .entry(StreamOutputKey {
                    round,
                    entity_name: entity_name.to_string(),
                })
                .or_default()
                .push_str(chunk);
            return Ok(());
        }

        self.write_live_chunk(round, entity_name, chunk)
    }

    fn complete_task(&mut self, round: u64, entity_name: &str, content: &str) -> io::Result<()> {
        if !should_typewrite(entity_name) {
            return self.finish_line();
        }

        let key = StreamOutputKey {
            round,
            entity_name: entity_name.to_string(),
        };
        let text = self
            .buffers
            .remove(&key)
            .filter(|buffered| !buffered.trim().is_empty())
            .unwrap_or_else(|| content.to_string());

        if text.trim().is_empty() {
            return Ok(());
        }

        self.finish_line()?;

        let mut stdout = io::stdout().lock();
        write!(
            stdout,
            "[stream] round={round} entity={entity_name} chunk=\n"
        )?;
        for ch in text.chars() {
            write!(stdout, "{ch}")?;
            stdout.flush()?;
            if !ch.is_whitespace() {
                thread::sleep(self.char_delay);
            }
        }
        writeln!(stdout)?;
        stdout.flush()
    }

    fn write_live_chunk(&mut self, round: u64, entity_name: &str, chunk: &str) -> io::Result<()> {
        let is_new_stream = self
            .current
            .as_ref()
            .map(|current| current.round != round || current.entity_name != entity_name)
            .unwrap_or(true);

        let mut stdout = io::stdout().lock();
        if is_new_stream {
            if self.current.is_some() {
                writeln!(stdout)?;
            }
            write!(
                stdout,
                "[stream] round={round} entity={entity_name} chunk=\n"
            )?;
            self.current = Some(StreamOutputKey {
                round,
                entity_name: entity_name.to_string(),
            });
        }

        write!(stdout, "{chunk}")?;
        stdout.flush()
    }

    fn finish_line(&mut self) -> io::Result<()> {
        if self.current.take().is_some() {
            let mut stdout = io::stdout().lock();
            writeln!(stdout)?;
            stdout.flush()?;
        }
        Ok(())
    }
}

fn should_typewrite(entity_name: &str) -> bool {
    matches!(
        entity_name,
        "FateWeaver" | "UpperNarrator" | "Protagonist" | "Fate" | "Narration"
    )
}

fn submit_first_choice_or_continue(
    session: &story_engine::AkashicSessionEngine,
    content: &str,
) -> Result<(), Box<dyn Error>> {
    let options = serde_json::from_str::<ProtagonistOptions>(content)?;
    if let Some(choice) = options.options.first() {
        println!("[api] auto selected choice: {}", choice.action);
        session.submit_player_action(PlayerActionInput {
            r#type: PlayerActionType::SelectedOption,
            action: choice.action.clone(),
        })?;
    } else {
        println!("[api] no player choices available; continuing");
        session.submit_player_action(PlayerActionInput {
            r#type: PlayerActionType::FreeText,
            action: "continue".to_string(),
        })?;
    }
    Ok(())
}

fn env_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

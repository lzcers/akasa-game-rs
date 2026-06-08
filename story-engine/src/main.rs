use std::{env, error::Error, sync::Arc, time::Duration};

use story_engine::{
    AkashicEngine, RuntimeDebugObserver,
    debug::LocalDebugObserver,
    resources::{
        protagonist_action::{PlayerActionInput, PlayerActionType},
        turn_state::TurnPhase,
    },
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();

    if env::var("DEEPSEEK_API_KEY").is_err() {
        eprintln!("请先设置 DEEPSEEK_API_KEY，再运行 `cargo run -p story_engine`");
        return Ok(());
    }

    let max_turns = env_usize("AKASHIC_STORY_ENGINE_MAX_TURNS", 200);
    let poll_interval = Duration::from_millis(env_u64("AKASHIC_STORY_ENGINE_POLL_MS", 100));
    let local_debug = local_debug_enabled();

    let engine = AkashicEngine::new_with_debug_observer(local_debug.then(|| {
        Arc::new(LocalDebugObserver::for_workspace_root()) as Arc<dyn RuntimeDebugObserver>
    }));
    let session = engine.create_default_session("story-engine-main").await?;
    session.start_next_turn()?;

    println!("== Akashic Story Engine ==");
    println!(
        "[api] story_engine main loop started, max_turns={}, poll_ms={}, local_debug={}",
        max_turns,
        poll_interval.as_millis(),
        local_debug
    );

    let mut frame = 0;
    let mut completed_turns = 0;
    let mut last_reported_phase = None;
    let mut last_completed_turn = None;

    loop {
        let snapshot = session.get_game_session();
        if last_reported_phase != Some(snapshot.phase) {
            print_frame_status(frame, &snapshot);
            last_reported_phase = Some(snapshot.phase);
        }

        match snapshot.phase {
            TurnPhase::Failed => {
                print_failure(&snapshot);
                return Err(format!("故事引擎在第 {} 轮失败", snapshot.active_turn_id).into());
            }
            TurnPhase::Ended => {
                println!(
                    "[api] story ended: turn_index={} active_turn_id={}",
                    snapshot.turn_index, snapshot.active_turn_id
                );
                return Ok(());
            }
            TurnPhase::AwaitingPlayer => {
                let Some(choice) = snapshot.choices.first() else {
                    return Err("故事引擎进入 AwaitingPlayer，但没有可选行动".into());
                };
                println!(
                    "[api] auto selected choice: {} -> {}",
                    choice.id, choice.option.action
                );
                session.submit_player_action(PlayerActionInput {
                    r#type: PlayerActionType::SelectedOption,
                    action: choice.option.action.clone(),
                })?;
            }
            TurnPhase::TurnCompleted if last_completed_turn != Some(snapshot.turn_index) => {
                completed_turns += 1;
                last_completed_turn = Some(snapshot.turn_index);
                println!(
                    "[api] turn complete: turn_index={} completed_turns={}/{}",
                    snapshot.turn_index, completed_turns, max_turns
                );
                if completed_turns >= max_turns {
                    println!("[api] reached max turns, exiting");
                    return Ok(());
                }
                session.start_next_turn()?;
            }
            TurnPhase::Idle => {
                session.start_next_turn()?;
            }
            _ => {}
        }

        frame += 1;
        tokio::time::sleep(poll_interval).await;
    }
}

fn print_frame_status(frame: usize, snapshot: &story_engine::Session) {
    println!(
        "[frame {frame:03}] phase={:?} turn_index={} active_turn_id={}",
        snapshot.phase, snapshot.turn_index, snapshot.active_turn_id
    );
}

fn print_failure(snapshot: &story_engine::Session) {
    for task in snapshot
        .tasks
        .iter()
        .filter(|task| task.status == story_engine::resources::agent_task::TaskStatus::Error)
    {
        eprintln!(
            "[error] agent task {:?} ({}) failed: {}",
            task.kind,
            task.entity,
            task.error.as_deref().unwrap_or("unknown error")
        );
    }
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

fn local_debug_enabled() -> bool {
    env_flag("AKASA_LOCAL_DEBUG")
        || env_flag("AKASA_STORY_ENGINE_LOCAL_DEBUG")
        || env::args().skip(1).any(|arg| arg == "--local-debug")
}

fn env_flag(key: &str) -> bool {
    env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

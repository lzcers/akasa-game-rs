use std::sync::Arc;

use agent::{agent::Context, models::ChatModel};
use bevy_ecs::{message::Messages, prelude::*};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::{
    archive::validate_archive_state,
    components::agent::Agent,
    profile::{DEFAULT_KEY_STORY_BEATS, DEFAULT_PROTAGONIST_PROFILE, DEFAULT_WORLD_PROFILE},
    resources::{
        agent_task::{AgentTaskManager, TaskUpdate},
        export::{ExportHandle, SessionSnapshot, TaskEvent, TaskView},
        history::RoundHistoryEntry,
        protagonist_action::{PendingProtagonistChoice, PlayerActionInput},
        turn_state::TurnPhase,
        world_snapshot::WorldSnapshot,
    },
    runtime::{EngineCommand, NewSessionState, SessionRegistry, SessionRuntime},
    schedule::build_schedule,
    turn_messages::PlayerCommand,
    utils::build_chat_model,
};

pub use crate::archive::{SessionArchiveState, SimulatorArchiveState};

pub trait RuntimeDebugObserver: Send + Sync {
    fn on_task_update(&self, session_id: &str, round: u64, update: &TaskUpdate);
    fn on_agent_context_updated(
        &self,
        session_id: &str,
        turn_index: u64,
        active_turn_id: u64,
        agent_name: &str,
        context: &Context,
    );
}

#[derive(Resource, Clone, Default)]
pub struct RuntimeDebugObserverResource {
    pub(crate) observer: Option<Arc<dyn RuntimeDebugObserver>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub phase: TurnPhase,
    pub turn_index: u64,
    pub active_turn_id: u64,
    pub history: Vec<RoundHistoryEntry>,
    pub current_task: Option<TaskView>,
    pub tasks: Vec<TaskView>,
    pub world_snapshot: WorldSnapshot,
    pub latest_narration: String,
    pub current_protagonist_action: String,
    pub choices: Vec<PendingProtagonistChoice>,
}

#[derive(Clone)]
pub struct AkashicEngine {
    command_tx: mpsc::UnboundedSender<EngineCommand>,
}

#[derive(Clone)]
pub struct AkashicSessionEngine {
    pub(crate) session_id: String,
    pub(crate) command_tx: mpsc::UnboundedSender<EngineCommand>,
    pub(crate) export_handle: ExportHandle,
}

impl Default for AkashicEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AkashicEngine {
    pub fn new() -> Self {
        Self::with_model(build_chat_model())
    }

    pub fn new_with_debug_observer(observer: Option<Arc<dyn RuntimeDebugObserver>>) -> Self {
        Self::with_model_and_debug_observer(build_chat_model(), observer)
    }

    pub fn with_model(model: ChatModel) -> Self {
        Self::with_model_and_debug_observer(model, None)
    }

    pub fn with_model_and_debug_observer(
        model: ChatModel,
        observer: Option<Arc<dyn RuntimeDebugObserver>>,
    ) -> Self {
        let mut world = World::new();
        world.insert_resource(AgentTaskManager::new(model));
        world.insert_resource(Messages::<PlayerCommand>::default());
        world.insert_resource(RuntimeDebugObserverResource {
            observer: observer.clone(),
        });
        world.init_resource::<SessionRegistry>();
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        SessionRuntime::spawn(world, build_schedule(), command_tx.clone(), command_rx);
        Self { command_tx }
    }

    pub async fn create_session(
        &self,
        session_id: impl Into<String>,
        world_profile: &str,
        protagonist_profile: &str,
        key_story_beats: &str,
    ) -> Result<AkashicSessionEngine, String> {
        self.create_session_from_state(
            session_id.into(),
            NewSessionState::Profiles {
                world_profile: world_profile.to_string(),
                protagonist_profile: protagonist_profile.to_string(),
                key_story_beats: key_story_beats.to_string(),
            },
        )
        .await
    }

    pub async fn create_default_session(
        &self,
        session_id: impl Into<String>,
    ) -> Result<AkashicSessionEngine, String> {
        self.create_session(
            session_id,
            DEFAULT_WORLD_PROFILE,
            DEFAULT_PROTAGONIST_PROFILE,
            DEFAULT_KEY_STORY_BEATS,
        )
        .await
    }

    pub async fn create_session_from_archive(
        &self,
        session_id: impl Into<String>,
        state: SessionArchiveState,
    ) -> Result<AkashicSessionEngine, String> {
        validate_archive_state(&state)?;
        self.create_session_from_state(session_id.into(), NewSessionState::Archive(Box::new(state)))
            .await
    }

    async fn create_session_from_state(
        &self,
        session_id: String,
        state: NewSessionState,
    ) -> Result<AkashicSessionEngine, String> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(EngineCommand::CreateSession {
                session_id,
                state,
                tx,
            })
            .map_err(|_| "故事引擎运行时已停止，无法创建会话".to_string())?;
        rx.await
            .map_err(|_| "故事引擎运行时已停止，无法接收会话创建结果".to_string())?
    }
}

impl AkashicSessionEngine {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn get_game_session(&self) -> Session {
        Session::from_snapshot(self.export_handle.current_snapshot())
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<TaskEvent> {
        self.export_handle.subscribe_events()
    }

    pub fn start_next_turn(&self) -> Result<(), String> {
        if self.get_game_session().phase == TurnPhase::Ended {
            return Err("故事已结束，无法继续推进".to_string());
        }
        self.command_tx
            .send(EngineCommand::StartNextTurn {
                session_id: self.session_id.clone(),
            })
            .map_err(|_| "故事引擎运行时已停止，无法继续推进".to_string())
    }

    pub fn submit_player_action(&self, input: PlayerActionInput) -> Result<(), String> {
        self.command_tx
            .send(EngineCommand::SubmitPlayerAction {
                session_id: self.session_id.clone(),
                input,
            })
            .map_err(|_| "故事引擎运行时已停止，无法提交行动".to_string())
    }

    pub async fn export_archive_state(&self) -> Result<SessionArchiveState, String> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(EngineCommand::ExportArchiveState {
                session_id: self.session_id.clone(),
                tx,
            })
            .map_err(|_| "故事引擎运行时已停止，无法导出存档".to_string())?;
        rx.await
            .map_err(|_| "故事引擎运行时已停止，无法接收存档导出结果".to_string())?
    }

    pub async fn add_simulator(&self, simulator: Agent) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(EngineCommand::AddSimulator {
                session_id: self.session_id.clone(),
                simulator,
                tx,
            })
            .map_err(|_| "故事引擎运行时已停止，无法添加 Simulator".to_string())?;
        rx.await
            .map_err(|_| "故事引擎运行时已停止，无法接收 Simulator 添加结果".to_string())?
    }

    pub async fn close(&self) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(EngineCommand::CloseSession {
                session_id: self.session_id.clone(),
                tx,
            })
            .map_err(|_| "故事引擎运行时已停止，无法关闭会话".to_string())?;
        rx.await
            .map_err(|_| "故事引擎运行时已停止，无法接收会话关闭结果".to_string())?
    }

    pub async fn wait_until_ready(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Session {
    fn from_snapshot(snapshot: SessionSnapshot) -> Self {
        Self {
            phase: snapshot.phase,
            turn_index: snapshot.turn_index,
            active_turn_id: snapshot.active_turn_id,
            history: snapshot.history,
            current_task: snapshot.current_task,
            tasks: snapshot.tasks,
            world_snapshot: snapshot.world,
            latest_narration: snapshot.latest_narration,
            current_protagonist_action: snapshot.current_protagonist_action,
            choices: snapshot.choices,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::agent::AgentOutputType;

    #[tokio::test]
    async fn keeps_profiles_isolated_across_sessions_in_one_world() {
        let engine = AkashicEngine::with_model(ChatModel::new());
        let first = engine
            .create_session("first", "world-a", "hero-a", "beats-a")
            .await
            .unwrap();
        let second = engine
            .create_session("second", "world-b", "hero-b", "beats-b")
            .await
            .unwrap();

        let first_archive = first.export_archive_state().await.unwrap();
        let second_archive = second.export_archive_state().await.unwrap();

        assert_eq!(first_archive.world_profile, "world-a");
        assert_eq!(second_archive.world_profile, "world-b");
        assert_eq!(first_archive.simulators.len(), 1);
        assert_eq!(second_archive.simulators.len(), 1);
        assert_eq!(first.session_id(), "first");
        assert_eq!(second.session_id(), "second");
    }

    #[tokio::test]
    async fn close_removes_only_target_session() {
        let engine = AkashicEngine::with_model(ChatModel::new());
        let first = engine
            .create_session("first", "world-a", "hero-a", "beats-a")
            .await
            .unwrap();
        let second = engine
            .create_session("second", "world-b", "hero-b", "beats-b")
            .await
            .unwrap();

        first.close().await.unwrap();

        assert_eq!(
            first.export_archive_state().await.err().unwrap(),
            "未找到会话 `first`"
        );
        assert_eq!(
            second.export_archive_state().await.unwrap().world_profile,
            "world-b"
        );
    }

    #[tokio::test]
    async fn rejects_dynamically_added_text_simulators() {
        let engine = AkashicEngine::with_model(ChatModel::new());
        let session = engine.create_default_session("session").await.unwrap();
        let result = session
            .add_simulator(Agent::new(
                AgentOutputType::Text,
                "WeatherSimulator",
                "simulate weather".to_string(),
            ))
            .await;

        assert_eq!(result.err().unwrap(), "动态 Simulator 只支持 JSON 输出");
    }

    #[tokio::test]
    async fn archive_rejects_text_simulators() {
        let engine = AkashicEngine::with_model(ChatModel::new());
        let session = engine.create_default_session("session").await.unwrap();
        let mut archive = session.export_archive_state().await.unwrap();
        archive.simulators.push(SimulatorArchiveState::new(
            AgentOutputType::Text,
            "WeatherSimulator",
            "",
            Context::default(),
        ));

        let result = engine
            .create_session_from_archive("restored", archive)
            .await;

        assert_eq!(
            result.err().unwrap(),
            "归档的 simulators 只能包含 JSON Simulator"
        );
    }

    #[tokio::test]
    async fn archive_rejects_narrator_in_simulators() {
        let engine = AkashicEngine::with_model(ChatModel::new());
        let session = engine.create_default_session("session").await.unwrap();
        let mut archive = session.export_archive_state().await.unwrap();
        archive.simulators.push(
            serde_json::from_value(serde_json::json!({
                "kind": "applicator",
                "output_type": "json",
                "name": "AnotherNarrator",
                "sys_prompt": "",
                "context": Context::default(),
            }))
            .expect("legacy simulator archive should deserialize"),
        );

        let result = engine
            .create_session_from_archive("restored", archive)
            .await;

        assert_eq!(
            result.err().unwrap(),
            "归档的 simulators 只能包含 Simulator"
        );
    }
}

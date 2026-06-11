mod runtime;
mod schedule;
mod session;
pub(crate) mod turn_messages;

use agent::models::ChatModel;
use bevy_ecs::{message::Messages, prelude::*};
use tokio::sync::{broadcast, oneshot};

use crate::{
    archive::validate_archive_state,
    components::{agent::Agent, outcome::PlayerActionInput},
    profile::{DEFAULT_CHARACTER_PROFILE, DEFAULT_KEY_STORY_BEATS, DEFAULT_WORLD_PROFILE},
    resources::{
        agent_task_manager::AgentTaskManager,
        session_events::{EngineEvent, SessionEventHandle},
        session_registry::SessionRegistry,
    },
    utils::build_chat_model,
};

use self::{
    runtime::{EngineCommand, SessionRuntimeHandle},
    schedule::build_schedule,
    session::NewSessionState,
    turn_messages::PlayerCommand,
};

pub use crate::archive::SessionArchiveState;

#[derive(Clone)]
pub struct AkashicEngine {
    runtime_handle: SessionRuntimeHandle,
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

    pub fn with_model(model: ChatModel) -> Self {
        let mut world = World::new();
        world.insert_resource(AgentTaskManager::new(model));
        world.insert_resource(Messages::<PlayerCommand>::default());
        world.init_resource::<SessionRegistry>();
        let runtime_handle = SessionRuntimeHandle::spawn(world, build_schedule());
        Self { runtime_handle }
    }

    pub async fn create_session(
        &self,
        session_id: impl Into<String>,
        character_name: &str,
        world_profile: &str,
        character_profile: &str,
        key_story_beats: &str,
    ) -> Result<AkashicSessionEngine, String> {
        self.create_session_from_state(
            session_id.into(),
            NewSessionState::Profiles {
                character_name: character_name.to_string(),
                world_profile: world_profile.to_string(),
                character_profile: character_profile.to_string(),
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
            crate::components::outcome::DEFAULT_PLAYER_CHARACTER_NAME,
            DEFAULT_WORLD_PROFILE,
            DEFAULT_CHARACTER_PROFILE,
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
        self.runtime_handle
            .send(EngineCommand::CreateSession {
                session_id,
                state,
                runtime_handle: self.runtime_handle.clone(),
                tx,
            })
            .map_err(|_| "故事引擎运行时已停止，无法创建会话".to_string())?;
        rx.await
            .map_err(|_| "故事引擎运行时已停止，无法接收会话创建结果".to_string())?
    }
}

#[derive(Clone)]
pub struct AkashicSessionEngine {
    pub(crate) session_id: String,
    pub(crate) runtime_handle: SessionRuntimeHandle,
    pub(crate) session_event_handle: SessionEventHandle,
}

impl AkashicSessionEngine {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn subscribe_session_events(&self) -> broadcast::Receiver<EngineEvent> {
        self.session_event_handle.subscribe_events()
    }

    pub fn start_next_turn(&self) -> Result<(), String> {
        self.runtime_handle
            .send(EngineCommand::StartNextTurn {
                session_id: self.session_id.clone(),
            })
            .map_err(|_| "故事引擎运行时已停止，无法继续推进".to_string())
    }

    pub fn submit_player_action(&self, input: PlayerActionInput) -> Result<(), String> {
        self.runtime_handle
            .send(EngineCommand::SubmitPlayerAction {
                session_id: self.session_id.clone(),
                input,
            })
            .map_err(|_| "故事引擎运行时已停止，无法提交行动".to_string())
    }

    pub async fn add_simulator(&self, simulator: Agent) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.runtime_handle
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
        self.runtime_handle
            .send(EngineCommand::CloseSession {
                session_id: self.session_id.clone(),
                tx,
            })
            .map_err(|_| "故事引擎运行时已停止，无法关闭会话".to_string())?;
        rx.await
            .map_err(|_| "故事引擎运行时已停止，无法接收会话关闭结果".to_string())?
    }
}

use bevy_ecs::{hierarchy::ChildOf, prelude::*, schedule::Schedule};
use tokio::sync::mpsc;

use crate::{
    archive::SessionArchiveState,
    components::{
        agent::{Agent, AgentOutputType, AgentRole, Applicator, Simulator},
        outcome::CharacterDecisionState,
        session::{SessionProfiles, StorySession},
        session_event_sink::SessionEventSink,
        turn_flow::TurnFlow,
        world_snapshot::WorldSnapshot,
    },
    resources::{agent_task_manager::AgentTaskManager, session_registry::SessionRegistry},
};

use super::runtime::SessionRuntimeHandle;
use super::{AkashicSessionEngine, runtime::EngineCommand};

pub(crate) enum NewSessionState {
    Profiles {
        character_name: String,
        world_profile: String,
        character_profile: String,
        key_story_beats: String,
    },
    Archive(Box<SessionArchiveState>),
}

pub(crate) struct SessionRuntime {
    pub(super) world: World,
    pub(super) schedule: Schedule,
    pub(super) command_rx: mpsc::UnboundedReceiver<EngineCommand>,
}

impl SessionRuntime {
    pub(super) fn create_session(
        &mut self,
        session_id: String,
        state: NewSessionState,
        runtime_handle: SessionRuntimeHandle,
    ) -> Result<AkashicSessionEngine, String> {
        self.remove_session(&session_id);

        let (event_sink, session_event_handle) = SessionEventSink::new(session_id.clone());
        let session_entity = match state {
            NewSessionState::Profiles {
                character_name,
                world_profile,
                character_profile,
                key_story_beats,
            } => {
                event_sink.publish_session_created(
                    character_name.clone(),
                    world_profile.clone(),
                    character_profile.clone(),
                    key_story_beats.clone(),
                );
                self.spawn_session_from_profiles(
                    &session_id,
                    character_name,
                    world_profile,
                    character_profile,
                    key_story_beats,
                    event_sink,
                )
            }
            NewSessionState::Archive(state) => {
                event_sink.publish_session_created(
                    state.character_name.clone(),
                    state.world_profile.clone(),
                    state.character_profile.clone(),
                    state.key_story_beats.clone(),
                );
                self.spawn_session_from_archive(&session_id, *state, event_sink)
            }
        }?;
        self.world
            .resource_mut::<SessionRegistry>()
            .entities
            .insert(session_id.clone(), session_entity);

        Ok(AkashicSessionEngine {
            session_id,
            runtime_handle,
            session_event_handle,
        })
    }

    pub(super) fn remove_session(&mut self, session_id: &str) {
        let Some(session_entity) = self
            .world
            .resource_mut::<SessionRegistry>()
            .entities
            .remove(session_id)
        else {
            return;
        };
        for agent_entity in self.owned_agent_entities(session_entity) {
            self.world
                .resource_mut::<AgentTaskManager>()
                .clear_task(agent_entity);
            self.world.despawn(agent_entity);
        }
        self.world.despawn(session_entity);
    }

    pub(super) fn add_simulator(
        &mut self,
        session_id: &str,
        simulator: Agent,
    ) -> Result<(), String> {
        let session_entity = self
            .session_entity(session_id)
            .ok_or_else(|| format!("未找到会话 `{session_id}`"))?;
        let flow = self
            .world
            .get::<TurnFlow>(session_entity)
            .ok_or_else(|| "会话缺少流程状态".to_string())?;
        if !flow.stage.is_stable() {
            return Err("只能在会话稳定阶段添加 Agent".to_string());
        }
        if simulator.output_type != AgentOutputType::Json {
            return Err("动态 Simulator 只支持 JSON 输出".to_string());
        }
        if simulator.output_type == AgentOutputType::Json && self.has_json_simulator(session_entity)
        {
            return Err("每个会话只能存在一个 JSON Simulator".to_string());
        }
        self.world
            .spawn((simulator, ChildOf(session_entity), Simulator));
        Ok(())
    }

    fn spawn_session_from_profiles(
        &mut self,
        session_id: &str,
        character_name: String,
        world_profile: String,
        character_profile: String,
        key_story_beats: String,
        event_sink: SessionEventSink,
    ) -> Result<Entity, String> {
        let session_entity = self
            .world
            .spawn((
                StorySession {
                    id: session_id.to_string(),
                },
                SessionProfiles {
                    world_profile: world_profile.clone(),
                    character_profile: character_profile.clone(),
                    key_story_beats: key_story_beats.clone(),
                },
                TurnFlow::default(),
                WorldSnapshot::default(),
                CharacterDecisionState::default(),
                event_sink,
            ))
            .id();
        self.spawn_agents(
            session_entity,
            vec![Agent::new_fate_weaver(
                &world_profile,
                &character_profile,
                &key_story_beats,
            )],
            Agent::new_upper_narrator(&world_profile, &character_profile),
            Agent::new_character_agent(&character_name, &world_profile, &character_profile),
        );
        Ok(session_entity)
    }

    fn spawn_session_from_archive(
        &mut self,
        session_id: &str,
        state: SessionArchiveState,
        event_sink: SessionEventSink,
    ) -> Result<Entity, String> {
        let session_entity = self
            .world
            .spawn((
                StorySession {
                    id: session_id.to_string(),
                },
                SessionProfiles {
                    world_profile: state.world_profile,
                    character_profile: state.character_profile,
                    key_story_beats: state.key_story_beats,
                },
                TurnFlow {
                    turn_index: state.turn_index,
                    stage: state.phase,
                },
                state.world_snapshot,
                CharacterDecisionState::from_archive(state.committed_actions, state.choices),
                event_sink,
            ))
            .id();
        self.spawn_agents(
            session_entity,
            vec![Agent::from_context_with_role(
                AgentRole::Simulator,
                AgentOutputType::Json,
                "FateWeaver",
                "",
                state.fate_weaver_context,
            )],
            Agent::from_context_with_role(
                AgentRole::Narrator,
                AgentOutputType::Text,
                "UpperNarrator",
                "",
                state.upper_narrator_context,
            ),
            Agent::from_context_with_role(
                AgentRole::Character,
                AgentOutputType::Json,
                state.character_name,
                "",
                state.character_agent_context,
            ),
        );
        Ok(session_entity)
    }

    fn spawn_agents(
        &mut self,
        session_entity: Entity,
        simulators: Vec<Agent>,
        narrator: Agent,
        character: Agent,
    ) {
        for agent in simulators {
            self.world
                .spawn((agent, ChildOf(session_entity), Simulator));
        }
        self.world
            .spawn((narrator, ChildOf(session_entity), Applicator));
        self.world
            .spawn((character, ChildOf(session_entity), Applicator));
    }

    pub(super) fn session_entity(&self, session_id: &str) -> Option<Entity> {
        self.world
            .resource::<SessionRegistry>()
            .entities
            .get(session_id)
            .copied()
    }

    pub(super) fn run_one_frame(&mut self) {
        self.schedule.run(&mut self.world);
    }

    pub(super) fn has_active_work(&mut self) -> bool {
        let mut query = self.world.query::<&TurnFlow>();
        query.iter(&self.world).any(|flow| !flow.stage.is_stable())
    }

    fn owned_agent_entities(&mut self, session_entity: Entity) -> Vec<Entity> {
        let mut agents = self.world.query::<(Entity, &ChildOf)>();
        agents
            .iter(&self.world)
            .filter_map(|(entity, owner)| (owner.parent() == session_entity).then_some(entity))
            .collect()
    }

    fn has_json_simulator(&mut self, session_entity: Entity) -> bool {
        let mut agents = self.world.query::<(&Agent, &ChildOf, &Simulator)>();
        agents.iter(&self.world).any(|(agent, owner, _)| {
            owner.parent() == session_entity
                && agent.role == AgentRole::Simulator
                && agent.output_type == AgentOutputType::Json
        })
    }
}

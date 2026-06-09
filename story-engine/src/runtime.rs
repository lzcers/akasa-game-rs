use std::{collections::HashMap, time::Duration};

use agent::agent::Context;
use bevy_ecs::{message::Messages, prelude::*, schedule::Schedule};
use tokio::{
    sync::{mpsc, oneshot},
    time::{MissedTickBehavior, interval},
};

use crate::{
    archive::{
        SessionArchiveState, SimulatorArchiveState, archive_view_from_current,
        completed_dialogue_archive_view,
    },
    components::{
        agent::{Agent, AgentOutputType, AgentRole, Applicator, Simulator},
        session::{SessionProfiles, StorySession},
        turn_flow::TurnFlow,
    },
    engine::AkashicSessionEngine,
    resources::{
        agent_task::AgentTaskManager,
        export::ExportState,
        history::SessionHistoryLog,
        player_input::PlayerInputConfig,
        protagonist_action::{PlayerActionInput, ProtagonistDecisionState},
        world_snapshot::WorldSnapshot,
    },
    turn_messages::PlayerCommand,
};

const DEFAULT_RUNTIME_TICK_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Resource, Default)]
pub(crate) struct SessionRegistry {
    entities: HashMap<String, Entity>,
}

pub(crate) enum NewSessionState {
    Profiles {
        world_profile: String,
        protagonist_profile: String,
        key_story_beats: String,
    },
    Archive(Box<SessionArchiveState>),
}

pub(crate) enum EngineCommand {
    CreateSession {
        session_id: String,
        state: NewSessionState,
        tx: oneshot::Sender<Result<AkashicSessionEngine, String>>,
    },
    StartNextTurn {
        session_id: String,
    },
    SubmitPlayerAction {
        session_id: String,
        input: PlayerActionInput,
    },
    AddSimulator {
        session_id: String,
        simulator: Agent,
        tx: oneshot::Sender<Result<(), String>>,
    },
    ExportArchiveState {
        session_id: String,
        tx: oneshot::Sender<Result<SessionArchiveState, String>>,
    },
}

pub(crate) struct SessionRuntime {
    world: World,
    schedule: Schedule,
    command_tx: mpsc::UnboundedSender<EngineCommand>,
    command_rx: mpsc::UnboundedReceiver<EngineCommand>,
}

impl SessionRuntime {
    pub(crate) fn spawn(
        world: World,
        schedule: Schedule,
        command_tx: mpsc::UnboundedSender<EngineCommand>,
        command_rx: mpsc::UnboundedReceiver<EngineCommand>,
    ) {
        tokio::spawn(async move {
            let mut runtime = Self {
                world,
                schedule,
                command_tx,
                command_rx,
            };
            let mut ticker = interval(DEFAULT_RUNTIME_TICK_INTERVAL);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    command = runtime.command_rx.recv() => {
                        let Some(command) = command else {
                            break;
                        };
                        runtime.handle_command(command);
                    }
                    _ = ticker.tick(), if runtime.has_active_work() => {
                        runtime.run_one_frame();
                    }
                }
            }
        });
    }

    fn handle_command(&mut self, command: EngineCommand) {
        match command {
            EngineCommand::CreateSession {
                session_id,
                state,
                tx,
            } => {
                let result = self.create_session(session_id, state);
                self.run_one_frame();
                let _ = tx.send(result);
            }
            EngineCommand::StartNextTurn { session_id } => {
                if let Some(entity) = self.session_entity(&session_id)
                    && let Some(mut flow) = self.world.get_mut::<TurnFlow>(entity)
                {
                    flow.advance();
                    self.run_one_frame();
                }
            }
            EngineCommand::SubmitPlayerAction { session_id, input } => {
                if let Some(entity) = self.session_entity(&session_id) {
                    let turn_id = self
                        .world
                        .get::<TurnFlow>(entity)
                        .map(|flow| flow.active_turn_id)
                        .unwrap_or_default();
                    self.world.resource_mut::<Messages<PlayerCommand>>().write(
                        PlayerCommand::SubmitPlayerAction {
                            session_entity: entity,
                            turn_id,
                            input,
                        },
                    );
                    self.run_one_frame();
                }
            }
            EngineCommand::AddSimulator {
                session_id,
                simulator,
                tx,
            } => {
                let _ = tx.send(self.add_simulator(&session_id, simulator));
            }
            EngineCommand::ExportArchiveState { session_id, tx } => {
                let _ = tx.send(self.export_archive_state(&session_id));
            }
        }
    }

    fn create_session(
        &mut self,
        session_id: String,
        state: NewSessionState,
    ) -> Result<AkashicSessionEngine, String> {
        self.remove_session(&session_id);

        let (export_state, export_handle) = ExportState::new_with_handle();
        let session_entity = match state {
            NewSessionState::Profiles {
                world_profile,
                protagonist_profile,
                key_story_beats,
            } => self.spawn_session_from_profiles(
                &session_id,
                world_profile,
                protagonist_profile,
                key_story_beats,
                export_state,
            ),
            NewSessionState::Archive(state) => {
                self.spawn_session_from_archive(&session_id, *state, export_state)
            }
        }?;
        self.world
            .resource_mut::<SessionRegistry>()
            .entities
            .insert(session_id.clone(), session_entity);

        Ok(AkashicSessionEngine {
            session_id,
            command_tx: self.command_tx.clone(),
            export_handle,
        })
    }

    fn remove_session(&mut self, session_id: &str) {
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

    fn add_simulator(&mut self, session_id: &str, simulator: Agent) -> Result<(), String> {
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
        world_profile: String,
        protagonist_profile: String,
        key_story_beats: String,
        export_state: ExportState,
    ) -> Result<Entity, String> {
        let session_entity = self
            .world
            .spawn((
                StorySession {
                    id: session_id.to_string(),
                },
                SessionProfiles {
                    world_profile: world_profile.clone(),
                    protagonist_profile: protagonist_profile.clone(),
                    key_story_beats: key_story_beats.clone(),
                },
                TurnFlow::default(),
                WorldSnapshot::default(),
                SessionHistoryLog::default(),
                PlayerInputConfig::wait_for_user(),
                ProtagonistDecisionState::default(),
                export_state,
            ))
            .id();
        self.spawn_agents(
            session_entity,
            vec![Agent::new_fate_weaver(
                &world_profile,
                &protagonist_profile,
                &key_story_beats,
            )],
            Agent::new_upper_narrator(&world_profile, &protagonist_profile),
            Agent::new_protagonist(&world_profile, &protagonist_profile),
        );
        Ok(session_entity)
    }

    fn spawn_session_from_archive(
        &mut self,
        session_id: &str,
        state: SessionArchiveState,
        export_state: ExportState,
    ) -> Result<Entity, String> {
        let session_entity = self
            .world
            .spawn((
                StorySession {
                    id: session_id.to_string(),
                },
                SessionProfiles {
                    world_profile: state.world_profile,
                    protagonist_profile: state.protagonist_profile,
                    key_story_beats: state.key_story_beats,
                },
                TurnFlow {
                    turn_index: state.turn_index,
                    active_turn_id: state.active_turn_id,
                    stage: state.phase,
                },
                state.world_snapshot,
                state.history_log,
                PlayerInputConfig::wait_for_user(),
                ProtagonistDecisionState::from_archive(state.committed_action, state.choices),
                export_state,
            ))
            .id();
        let simulators = if state.simulators.is_empty() {
            vec![Agent::from_context_with_role(
                AgentRole::Simulator,
                AgentOutputType::Json,
                "FateWeaver",
                "",
                state.fate_weaver_context,
            )]
        } else {
            state
                .simulators
                .into_iter()
                .map(|simulator| {
                    Agent::from_context_with_role(
                        AgentRole::Simulator,
                        simulator.output_type,
                        simulator.name,
                        simulator.sys_prompt,
                        simulator.context,
                    )
                })
                .collect()
        };
        self.spawn_agents(
            session_entity,
            simulators,
            Agent::from_context_with_role(
                AgentRole::Narrator,
                AgentOutputType::Text,
                "UpperNarrator",
                "",
                state.upper_narrator_context,
            ),
            Agent::from_context_with_role(
                AgentRole::Protagonist,
                AgentOutputType::Json,
                "Protagonist",
                "",
                state.protagonist_context,
            ),
        );
        Ok(session_entity)
    }

    fn spawn_agents(
        &mut self,
        session_entity: Entity,
        simulators: Vec<Agent>,
        narrator: Agent,
        protagonist: Agent,
    ) {
        for agent in simulators {
            self.world
                .spawn((agent, ChildOf(session_entity), Simulator));
        }
        self.world
            .spawn((narrator, ChildOf(session_entity), Applicator));
        self.world
            .spawn((protagonist, ChildOf(session_entity), Applicator));
    }

    fn session_entity(&self, session_id: &str) -> Option<Entity> {
        self.world
            .resource::<SessionRegistry>()
            .entities
            .get(session_id)
            .copied()
    }

    fn run_one_frame(&mut self) {
        self.schedule.run(&mut self.world);
    }

    fn has_active_work(&mut self) -> bool {
        let mut query = self.world.query::<&TurnFlow>();
        query.iter(&self.world).any(|flow| !flow.stage.is_stable())
    }

    fn export_archive_state(&mut self, session_id: &str) -> Result<SessionArchiveState, String> {
        let entity = self
            .session_entity(session_id)
            .ok_or_else(|| format!("未找到会话 `{session_id}`"))?;
        self.export_archive_state_for_entity(entity)
    }

    fn export_archive_state_for_entity(
        &mut self,
        entity: Entity,
    ) -> Result<SessionArchiveState, String> {
        let flow = *self
            .world
            .get::<TurnFlow>(entity)
            .ok_or_else(|| "会话缺少流程状态".to_string())?;
        let profiles = self
            .world
            .get::<SessionProfiles>(entity)
            .ok_or_else(|| "会话缺少配置资料".to_string())?
            .clone();
        let current_world_snapshot = self
            .world
            .get::<WorldSnapshot>(entity)
            .ok_or_else(|| "会话缺少世界快照".to_string())?
            .clone();
        let current_history_log = self
            .world
            .get::<SessionHistoryLog>(entity)
            .ok_or_else(|| "会话缺少历史记录".to_string())?
            .clone();
        let decision_state = self
            .world
            .get::<ProtagonistDecisionState>(entity)
            .ok_or_else(|| "会话缺少主角决策状态".to_string())?
            .clone();
        let simulator_agents = self.owned_simulator_agents(entity);
        let archive_view = if flow.stage.is_stable() {
            archive_view_from_current(
                flow,
                current_world_snapshot,
                decision_state.committed_action().to_string(),
                decision_state.choices().to_vec(),
                current_history_log,
            )
        } else {
            completed_dialogue_archive_view(&current_history_log)?
        };

        Ok(SessionArchiveState {
            world_profile: profiles.world_profile,
            protagonist_profile: profiles.protagonist_profile,
            key_story_beats: profiles.key_story_beats,
            phase: archive_view.phase,
            turn_index: archive_view.turn_index,
            active_turn_id: archive_view.active_turn_id,
            world_snapshot: archive_view.world_snapshot,
            committed_action: archive_view.committed_action,
            choices: archive_view.choices,
            history_log: archive_view.history_log,
            fate_weaver_context: simulator_agents
                .iter()
                .find(|agent| {
                    agent.role == AgentRole::Simulator && agent.output_type == AgentOutputType::Json
                })
                .map(|agent| agent.context.clone())
                .ok_or_else(|| "缺少 JSON Simulator".to_string())?,
            upper_narrator_context: self.unique_applicator_context(entity, AgentRole::Narrator)?,
            protagonist_context: self.unique_applicator_context(entity, AgentRole::Protagonist)?,
            simulators: simulator_agents
                .iter()
                .map(|agent| {
                    SimulatorArchiveState::new(
                        agent.output_type,
                        agent.name.clone(),
                        agent.sys_prompt.clone(),
                        agent.context.clone(),
                    )
                })
                .collect(),
        })
    }

    fn owned_agent_entities(&mut self, session_entity: Entity) -> Vec<Entity> {
        let mut agents = self.world.query::<(Entity, &ChildOf)>();
        agents
            .iter(&self.world)
            .filter_map(|(entity, owner)| (owner.parent() == session_entity).then_some(entity))
            .collect()
    }

    fn owned_simulator_agents(&mut self, session_entity: Entity) -> Vec<Agent> {
        let mut agents = self.world.query::<(&Agent, &ChildOf, &Simulator)>();
        agents
            .iter(&self.world)
            .filter(move |(_, owner, _)| owner.parent() == session_entity)
            .map(|(agent, _, _)| agent.clone())
            .collect()
    }

    fn unique_applicator_context(
        &mut self,
        session_entity: Entity,
        role: AgentRole,
    ) -> Result<Context, String> {
        let mut agents = self.world.query::<(&Agent, &ChildOf, &Applicator)>();
        let mut matches = agents
            .iter(&self.world)
            .filter(|(agent, owner, _)| owner.parent() == session_entity && agent.role == role);
        let context = matches
            .next()
            .map(|(agent, ..)| agent.context.clone())
            .ok_or_else(|| format!("缺少 Applicator/{role:?} Agent"))?;
        if matches.next().is_some() {
            return Err(format!("存在多个 Applicator/{role:?} Agent"));
        }
        Ok(context)
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

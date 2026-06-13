use std::time::Duration;

use bevy_ecs::{message::Messages, prelude::World, schedule::Schedule};
use tokio::{
    sync::{mpsc, oneshot},
    time::{MissedTickBehavior, interval},
};

use crate::components::{agent::Agent, outcome::PlayerActionInput, turn_flow::TurnFlow};

use super::{
    AkashicSessionEngine,
    session::{NewSessionState, SessionRuntime},
    turn_messages::PlayerCommand,
};

const DEFAULT_RUNTIME_TICK_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Clone)]
pub(crate) struct SessionRuntimeHandle {
    command_tx: mpsc::UnboundedSender<EngineCommand>,
}

pub(crate) enum EngineCommand {
    CreateSession {
        session_id: String,
        state: NewSessionState,
        runtime_handle: SessionRuntimeHandle,
        tx: oneshot::Sender<Result<AkashicSessionEngine, String>>,
    },
    StartNextTurn {
        session_id: String,
    },
    SubmitPlayerAction {
        session_id: String,
        input: PlayerActionInput,
    },
    SubmitPlayerActionForTurn {
        session_id: String,
        expected_turn_id: u64,
        input: PlayerActionInput,
        tx: oneshot::Sender<Result<(), String>>,
    },
    AddSimulator {
        session_id: String,
        simulator: Agent,
        tx: oneshot::Sender<Result<(), String>>,
    },
    CloseSession {
        session_id: String,
        tx: oneshot::Sender<Result<(), String>>,
    },
}

impl SessionRuntimeHandle {
    pub(crate) fn spawn(world: World, schedule: Schedule) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let handle = Self { command_tx };
        SessionRuntime::spawn(world, schedule, command_rx);
        handle
    }

    pub(crate) fn send(&self, command: EngineCommand) -> Result<(), ()> {
        self.command_tx.send(command).map_err(|_| ())
    }
}

impl SessionRuntime {
    pub(crate) fn spawn(
        world: World,
        schedule: Schedule,
        command_rx: mpsc::UnboundedReceiver<EngineCommand>,
    ) {
        tokio::spawn(async move {
            let mut runtime = Self {
                world,
                schedule,
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
                runtime_handle,
                tx,
            } => {
                let result = self.create_session(session_id, state, runtime_handle);
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
                        .map(|flow| flow.active_turn_id())
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
            EngineCommand::SubmitPlayerActionForTurn {
                session_id,
                expected_turn_id,
                input,
                tx,
            } => {
                let result =
                    self.submit_player_action_for_turn(&session_id, expected_turn_id, input);
                let _ = tx.send(result);
            }
            EngineCommand::AddSimulator {
                session_id,
                simulator,
                tx,
            } => {
                let _ = tx.send(self.add_simulator(&session_id, simulator));
            }
            EngineCommand::CloseSession { session_id, tx } => {
                self.remove_session(&session_id);
                let _ = tx.send(Ok(()));
            }
        }
    }

    fn submit_player_action_for_turn(
        &mut self,
        session_id: &str,
        expected_turn_id: u64,
        input: PlayerActionInput,
    ) -> Result<(), String> {
        let Some(entity) = self.session_entity(session_id) else {
            return Err("未找到进行中的记录。".to_string());
        };
        let Some(flow) = self.world.get::<TurnFlow>(entity) else {
            return Err("记录流程状态不可用。".to_string());
        };
        if flow.stage != crate::components::turn_flow::TurnStage::AwaitingPlayer {
            return Err("当前回合还不能提交选择。".to_string());
        }
        let turn_id = flow.active_turn_id();
        if turn_id != expected_turn_id {
            return Err(format!(
                "选择回合已变化，请刷新后重试。expected={expected_turn_id}, actual={turn_id}"
            ));
        }

        self.world.resource_mut::<Messages<PlayerCommand>>().write(
            PlayerCommand::SubmitPlayerAction {
                session_entity: entity,
                turn_id,
                input,
            },
        );
        self.run_one_frame();
        self.run_one_frame();

        let Some(mut flow) = self.world.get_mut::<TurnFlow>(entity) else {
            return Err("记录流程状态不可用。".to_string());
        };
        if flow.stage != crate::components::turn_flow::TurnStage::TurnCompleted {
            return Err("当前选择未被故事引擎接受。".to_string());
        }
        flow.advance();
        self.run_one_frame();
        Ok(())
    }
}

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

    pub(crate) fn send(
        &self,
        command: EngineCommand,
    ) -> Result<(), mpsc::error::SendError<EngineCommand>> {
        self.command_tx.send(command)
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
            EngineCommand::CloseSession { session_id, tx } => {
                self.remove_session(&session_id);
                let _ = tx.send(Ok(()));
            }
        }
    }
}

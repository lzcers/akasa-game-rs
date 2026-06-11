use bevy_ecs::{
    entity::Entity,
    hierarchy::ChildOf,
    query::With,
    system::{Commands, Query},
};

use crate::components::{
    agent::{Applicator, Simulator},
    flow::{ApplicationCompleted, FlowEnd, PlayerInputCompleted, SimulationCompleted},
    session_event_sink::SessionEventSink,
    turn_flow::{TurnFlow, TurnStage},
};

#[allow(clippy::type_complexity)]
pub fn flow_progress_system(
    mut commands: Commands,
    mut sessions: Query<(
        Entity,
        &SessionEventSink,
        &mut TurnFlow,
        Option<&FlowEnd>,
        Option<&PlayerInputCompleted>,
    )>,
    simulators: Query<(&ChildOf, Option<&SimulationCompleted>), With<Simulator>>,
    applicators: Query<(&ChildOf, Option<&ApplicationCompleted>), With<Applicator>>,
) {
    for (session_entity, event_sink, mut flow, flow_end, player_input) in sessions.iter_mut() {
        match flow.stage {
            TurnStage::Simulation => {
                if flow_end.is_some() {
                    commands.entity(session_entity).remove::<FlowEnd>();
                    let round = flow.active_turn_id.max(1);
                    flow.end();
                    event_sink.publish_flow_turn_end(round);
                    continue;
                }

                let total = simulators
                    .iter()
                    .filter(|(owner, _)| owner.parent() == session_entity)
                    .count();
                let completed = simulators
                    .iter()
                    .filter(|(owner, completed)| {
                        owner.parent() == session_entity
                            && completed
                                .is_some_and(|completed| completed.turn_id == flow.active_turn_id)
                    })
                    .count();

                if total == 0 {
                    event_sink.publish_flow_turn_error(
                        flow.active_turn_id.max(1),
                        flow.stage,
                        "flow",
                        "simulation stage has no simulator entities",
                    );
                    flow.stage = TurnStage::Failed;
                } else if completed == total {
                    flow.stage = TurnStage::Application;
                }
            }
            TurnStage::Application => {
                let total = applicators
                    .iter()
                    .filter(|(owner, _)| owner.parent() == session_entity)
                    .count();
                let resolved = applicators
                    .iter()
                    .filter(|(owner, completed)| {
                        owner.parent() == session_entity
                            && completed
                                .is_some_and(|completed| completed.turn_id == flow.active_turn_id)
                    })
                    .count();

                if total == 0 {
                    event_sink.publish_flow_turn_error(
                        flow.active_turn_id.max(1),
                        flow.stage,
                        "flow",
                        "application stage has no applicator entities",
                    );
                    flow.stage = TurnStage::Failed;
                } else if resolved == total && flow_end.is_some() {
                    commands.entity(session_entity).remove::<FlowEnd>();
                    let round = flow.active_turn_id.max(1);
                    flow.end();
                    event_sink.publish_flow_turn_end(round);
                } else if resolved == total {
                    flow.stage = TurnStage::AwaitingPlayer;
                }
            }
            TurnStage::AwaitingPlayer => {
                if player_input.is_some_and(|completed| completed.turn_id == flow.active_turn_id) {
                    commands
                        .entity(session_entity)
                        .remove::<PlayerInputCompleted>();
                    let round = flow.active_turn_id.max(1);
                    flow.finish_turn();
                    event_sink.publish_flow_turn_completed(round);
                }
            }
            TurnStage::Idle | TurnStage::TurnCompleted | TurnStage::Ended | TurnStage::Failed => {}
        }
    }
}

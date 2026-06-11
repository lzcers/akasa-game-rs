use bevy_ecs::{
    entity::Entity,
    message::MessageReader,
    system::{Commands, Query},
};

use crate::{
    components::{
        flow::PlayerInputCompleted,
        outcome::{PlayerActionType, ProtagonistDecisionState},
        session_event_sink::SessionEventSink,
        turn_flow::{TurnFlow, TurnStage},
    },
    engine::turn_messages::PlayerCommand,
};

#[allow(clippy::type_complexity)]
pub fn player_input_consume_system(
    mut commands: Commands,
    mut player_commands: MessageReader<PlayerCommand>,
    mut sessions: Query<(
        Entity,
        &SessionEventSink,
        &TurnFlow,
        &mut ProtagonistDecisionState,
        Option<&PlayerInputCompleted>,
    )>,
) {
    let player_commands = player_commands.read().cloned().collect::<Vec<_>>();

    for (entity, event_sink, flow, mut decision_state, outcome) in sessions.iter_mut() {
        if flow.stage != TurnStage::AwaitingPlayer
            || outcome.is_some_and(|outcome| outcome.turn_id == flow.active_turn_id)
        {
            continue;
        }

        for command in &player_commands {
            match command {
                PlayerCommand::SubmitPlayerAction {
                    session_entity,
                    turn_id,
                    input,
                } => {
                    if *session_entity != entity || *turn_id != flow.active_turn_id {
                        continue;
                    }
                    let action = input.action.trim();
                    if action.is_empty()
                        || (input.r#type == PlayerActionType::SelectedOption
                            && !decision_state.has_action(action))
                    {
                        continue;
                    }

                    let action_type = input.r#type;
                    let committed_action = decision_state.commit_action(action);
                    event_sink.publish_player_input(
                        flow.active_turn_id.max(1),
                        action_type,
                        committed_action,
                    );
                    commands.entity(entity).insert(PlayerInputCompleted {
                        turn_id: flow.active_turn_id,
                    });
                    break;
                }
            }
        }
    }
}

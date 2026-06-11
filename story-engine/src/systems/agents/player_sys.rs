use bevy_ecs::{
    entity::Entity,
    message::MessageReader,
    system::{Commands, Query},
};

use crate::{
    components::{
        flow::PlayerInputCompleted,
        outcome::{CharacterDecisionState, PlayerActionItem, PlayerActionType},
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
        &mut CharacterDecisionState,
        Option<&PlayerInputCompleted>,
    )>,
) {
    let player_commands = player_commands.read().cloned().collect::<Vec<_>>();

    for (entity, event_sink, flow, mut decision_state, outcome) in sessions.iter_mut() {
        if flow.stage != TurnStage::AwaitingPlayer
            || outcome.is_some_and(|outcome| outcome.turn_id == flow.active_turn_id())
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
                    if *session_entity != entity || *turn_id != flow.active_turn_id() {
                        continue;
                    }
                    let Some(committed_actions) = validated_actions(&decision_state, input) else {
                        continue;
                    };

                    let committed_actions = decision_state.commit_actions(committed_actions);
                    event_sink
                        .publish_player_input(flow.active_turn_id().max(1), committed_actions);
                    commands.entity(entity).insert(PlayerInputCompleted {
                        turn_id: flow.active_turn_id(),
                    });
                    break;
                }
            }
        }
    }
}

fn validated_actions(
    decision_state: &CharacterDecisionState,
    input: &crate::components::outcome::PlayerActionInput,
) -> Option<Vec<PlayerActionItem>> {
    let mut actions = Vec::new();
    for item in input
        .actions
        .iter()
        .cloned()
        .map(PlayerActionItem::normalized)
    {
        if item.action.is_empty() {
            return None;
        }
        if let Some(choice) = decision_state.choice_for_action(&item.action) {
            actions.push(PlayerActionItem {
                action_type: PlayerActionType::SelectedOption,
                title: choice.title.clone(),
                motivation_and_risk: choice.motivation_and_risk.clone(),
                ..item
            });
        } else if item.action_type == PlayerActionType::SelectedOption {
            return None;
        } else {
            actions.push(PlayerActionItem {
                action_type: PlayerActionType::FreeText,
                ..item
            });
        }
    }
    (!actions.is_empty()).then_some(actions)
}

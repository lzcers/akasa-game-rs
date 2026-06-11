use bevy_ecs::{entity::Entity, message::Message};

use crate::components::outcome::PlayerActionInput;

#[derive(Message, Debug, Clone)]
pub enum PlayerCommand {
    SubmitPlayerAction {
        session_entity: Entity,
        turn_id: u64,
        input: PlayerActionInput,
    },
}

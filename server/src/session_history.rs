use serde::{Deserialize, Serialize};
use story_engine::components::{
    outcome::{PendingCharacterChoice, PlayerActionItem},
    turn_flow::TurnStage,
    world_snapshot::WorldSnapshot,
};

pub type TurnPhase = TurnStage;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionHistoryLog {
    pub rounds: Vec<RoundHistoryEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoundHistoryEntry {
    pub round: u64,
    pub world_snapshot: Option<WorldSnapshot>,
    pub narration_text: Option<String>,
    pub choices: Vec<PendingCharacterChoice>,
    pub committed_actions: Vec<PlayerActionItem>,
}

use crate::components::{
    outcome::{PendingCharacterChoice, PlayerActionItem},
    turn_flow::TurnStage,
    world_snapshot::WorldSnapshot,
};
use agent::agent::Context;

// 用于从外部恢复引擎状态的 DTO
#[derive(Debug, Clone)]
pub struct SessionArchiveState {
    pub character_name: String,
    pub world_profile: String,
    pub character_profile: String,
    pub key_story_beats: String,
    pub phase: TurnStage,
    pub turn_index: u64,
    pub world_snapshot: WorldSnapshot,
    pub committed_actions: Vec<PlayerActionItem>,
    pub choices: Vec<PendingCharacterChoice>,
    pub fate_weaver_context: Context,
    pub upper_narrator_context: Context,
    pub character_agent_context: Context,
}

pub(crate) fn validate_archive_state(state: &SessionArchiveState) -> Result<(), String> {
    if !state.phase.is_stable() || state.phase == TurnStage::Failed {
        return Err("归档会话不在可恢复的稳定态".to_string());
    }
    Ok(())
}

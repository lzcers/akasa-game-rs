use anyhow::{Context, Result};
use story_engine::components::{agent::AgentOutputType, outcome::PlayerActionType};

use crate::session_history::TurnPhase;

pub(super) fn serialize_phase(phase: TurnPhase) -> Result<String> {
    serde_json::to_string(&phase)
        .map(|value| value.trim_matches('"').to_string())
        .context("failed to serialize turn phase")
}
pub(super) fn deserialize_phase(value: &str) -> std::result::Result<TurnPhase, String> {
    serde_json::from_str(&format!("{value:?}")).map_err(|err| err.to_string())
}
pub(super) fn serialize_agent_output_type(output_type: AgentOutputType) -> Result<String> {
    serde_json::to_string(&output_type)
        .map(|value| value.trim_matches('"').to_string())
        .context("failed to serialize agent output type")
}
pub(super) fn deserialize_agent_output_type(
    value: &str,
) -> std::result::Result<AgentOutputType, String> {
    serde_json::from_str(&format!("{value:?}")).map_err(|err| err.to_string())
}
pub(super) fn serialize_player_action_type(action_type: PlayerActionType) -> Result<String> {
    serde_json::to_string(&action_type)
        .map(|value| value.trim_matches('"').to_string())
        .context("failed to serialize player action type")
}
pub(super) fn deserialize_player_action_type(
    value: &str,
) -> std::result::Result<PlayerActionType, String> {
    serde_json::from_str(&format!("{value:?}")).map_err(|err| err.to_string())
}

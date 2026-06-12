use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use story_engine::components::{
    outcome::{PendingCharacterChoice, PlayerActionInput, PlayerActionItem, PlayerActionType},
    world_snapshot::{ItemState, NpcState, OngoingEvent, WorldSnapshot},
};

use crate::session_history::{RoundHistoryEntry, TurnPhase};

#[derive(Debug, Clone, Deserialize)]
pub struct SessionPath {
    pub session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRoundsQuery {
    pub before_round: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorySummaryData {
    pub summary: String,
    pub narration_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGameSessionRequest {
    #[serde(default)]
    pub character_name: String,
    pub world_profile: String,
    pub character_profile: String,
    #[serde(default)]
    pub key_story_beats: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGameSessionData {
    pub session_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveExportData {
    pub session_id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub compressed_archive: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveExportRequest {
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadArchiveRequest {
    pub compressed_archive: String,
}

pub type SessionActionInput = PlayerActionInput;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GameSessionControlCommand {
    Continue,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlGameSessionRequest {
    pub control: Option<GameSessionControlCommand>,
    pub action: Option<SessionActionInput>,
    pub expected_round: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktrackGameSessionRequest {
    pub source_round: u64,
    pub action: SessionActionInput,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktrackGameSessionData {
    pub session: GameSessionWorldStateData,
    pub source_round: u64,
    pub branch_round: u64,
    pub reused_existing_branch: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneGameSessionQuery {
    pub round: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSessionWorldStateData {
    pub session_id: String,
    pub generated_profiles: GeneratedProfilesData,
    pub status: String,
    pub phase: TurnPhase,
    pub flow_end: bool,
    pub turn_index: u64,
    pub active_turn_id: u64,
    pub world_state: WorldStateData,
    pub latest_narration: String,
    pub current_outcome: String,
    pub choices: Vec<PendingCharacterChoice>,
    pub choice_explorations: ChoiceExplorationsData,
    pub branch_explorations: Vec<BranchExplorationData>,
}

pub type ChoiceExplorationsData = BTreeMap<String, ChoiceExplorationData>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceExplorationData {
    pub visited: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchExplorationData {
    pub action: PlayerActionItem,
    pub visited: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedProfilesData {
    pub world: String,
    pub character: String,
    pub key_story_beats: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundHistoryData {
    pub round: u64,
    pub world_state: Option<WorldStateData>,
    pub narration_text: String,
    pub choices: Vec<PendingCharacterChoice>,
    pub choice_explorations: ChoiceExplorationsData,
    pub branch_explorations: Vec<BranchExplorationData>,
    pub committed_actions: Vec<PlayerActionItem>,
    pub selected_choice_text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRoundsPageData {
    pub session_id: String,
    pub rounds: Vec<RoundHistoryData>,
    pub next_before_round: Option<u64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldStateData {
    pub round: u64,
    pub scene_title: String,
    pub time_absolute: String,
    pub time_relative: Option<String>,
    pub location_name: String,
    pub location_exits: Vec<String>,
    pub location_status: String,
    pub description: String,
    pub current_event: String,
    pub new_info: Vec<String>,
    pub inner_conflict: String,
    pub hard_anchors: Vec<String>,
    pub pace: String,
    pub atmosphere: String,
    pub focal_point: String,
    pub is_ending: bool,
    pub ending_type: Option<String>,
    pub character_condition: String,
    pub character_known_secrets: Vec<String>,
    pub npcs: Vec<NpcStateData>,
    pub items: Vec<ItemStateData>,
    pub events_in_progress: Vec<OngoingEventData>,
    pub unsolved_threads: Vec<String>,
    pub pacing_note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcStateData {
    pub name: String,
    pub location: String,
    pub mood: String,
    pub attitude: String,
    pub goal: String,
    pub secrets: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemStateData {
    pub name: String,
    pub location: String,
    pub status: String,
    pub awareness: String,
    pub relevance: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OngoingEventData {
    pub name: String,
    pub status: String,
    pub escalation_trigger: String,
}

impl From<WorldSnapshot> for WorldStateData {
    fn from(value: WorldSnapshot) -> Self {
        Self {
            round: value.round,
            scene_title: value.scene_title,
            time_absolute: value.time_absolute,
            time_relative: value.time_relative,
            location_name: value.location_name,
            location_exits: value.location_exits,
            location_status: value.location_status,
            description: value.description,
            current_event: value.current_event,
            new_info: value.new_info,
            inner_conflict: value.inner_conflict,
            hard_anchors: value.hard_anchors,
            pace: value.pace,
            atmosphere: value.atmosphere,
            focal_point: value.focal_point,
            is_ending: value.is_ending,
            ending_type: value.ending_type,
            character_condition: value.character_condition,
            character_known_secrets: value.character_known_secrets,
            npcs: value.npcs.into_iter().map(Into::into).collect(),
            items: value.items.into_iter().map(Into::into).collect(),
            events_in_progress: value
                .events_in_progress
                .into_iter()
                .map(Into::into)
                .collect(),
            unsolved_threads: value.unsolved_threads,
            pacing_note: value.pacing_note,
        }
    }
}

impl From<RoundHistoryEntry> for RoundHistoryData {
    fn from(value: RoundHistoryEntry) -> Self {
        let selected_choice_text = selected_choice_text(&value);
        let choice_explorations = value
            .choices
            .iter()
            .map(|choice| {
                (
                    choice.option.action.clone(),
                    ChoiceExplorationData { visited: false },
                )
            })
            .collect();

        Self {
            round: value.round,
            world_state: value.world_snapshot.map(Into::into),
            narration_text: value.narration_text.unwrap_or_default(),
            choices: value.choices,
            choice_explorations,
            branch_explorations: Vec::new(),
            committed_actions: value.committed_actions,
            selected_choice_text,
        }
    }
}

fn selected_choice_text(value: &RoundHistoryEntry) -> Option<String> {
    match value.committed_actions.as_slice() {
        [] => None,
        [single] if single.action_type == PlayerActionType::FreeText => {
            if single.action.trim() == "continue" {
                Some("继续回响".to_string())
            } else {
                Some("[执念]".to_string())
            }
        }
        [single] => value
            .choices
            .iter()
            .find_map(|choice| {
                (choice.option.action == single.action).then(|| choice.option.title.clone())
            })
            .or_else(|| Some(single.title.clone()))
            .filter(|title| !title.trim().is_empty())
            .or_else(|| Some(single.action.clone())),
        many => Some(
            many.iter()
                .map(|action| format!("{}: {}", action.character_name, action.action))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    }
}

impl From<NpcState> for NpcStateData {
    fn from(value: NpcState) -> Self {
        Self {
            name: value.name,
            location: value.location,
            mood: value.mood,
            attitude: value.attitude,
            goal: value.goal,
            secrets: value.secrets,
        }
    }
}

impl From<ItemState> for ItemStateData {
    fn from(value: ItemState) -> Self {
        Self {
            name: value.name,
            location: value.location,
            status: value.status,
            awareness: value.awareness,
            relevance: value.relevance,
        }
    }
}

impl From<OngoingEvent> for OngoingEventData {
    fn from(value: OngoingEvent) -> Self {
        Self {
            name: value.name,
            status: value.status,
            escalation_trigger: value.escalation_trigger,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlGameSessionData {
    pub action: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use story_engine::components::outcome::{CharacterOption, PlayerActionItem};

    #[test]
    fn round_history_choice_explorations_are_keyed_by_action() {
        let entry = RoundHistoryEntry {
            round: 1,
            choices: vec![PendingCharacterChoice {
                id: "choice-1".to_string(),
                option: CharacterOption {
                    title: "绕行".to_string(),
                    action: "绕到钟楼背面".to_string(),
                    motivation_and_risk: "视野更好，但会暴露脚步声".to_string(),
                },
            }],
            ..RoundHistoryEntry::default()
        };

        let data = RoundHistoryData::from(entry);

        assert!(data.choice_explorations.contains_key("绕到钟楼背面"));
        assert!(!data.choice_explorations.contains_key("choice-1"));
    }

    #[test]
    fn round_history_free_text_selected_choice_uses_obsession_label() {
        let entry = RoundHistoryEntry {
            round: 1,
            committed_actions: vec![PlayerActionItem::character_free_text("检查密室暗门")],
            ..RoundHistoryEntry::default()
        };

        let data = RoundHistoryData::from(entry);

        assert_eq!(data.selected_choice_text.as_deref(), Some("[执念]"));
    }
}

use agent::agent::Context;
use serde::{Deserialize, Serialize};

use crate::{
    components::{
        agent::AgentOutputType,
        turn_flow::{TurnFlow, TurnStage},
    },
    resources::{
        history::SessionHistoryLog, protagonist_action::PendingProtagonistChoice,
        world_snapshot::WorldSnapshot,
    },
};

#[derive(Debug, Clone)]
pub struct SessionArchiveState {
    pub world_profile: String,
    pub protagonist_profile: String,
    pub key_story_beats: String,
    pub phase: TurnStage,
    pub turn_index: u64,
    pub active_turn_id: u64,
    pub world_snapshot: WorldSnapshot,
    pub committed_action: String,
    pub choices: Vec<PendingProtagonistChoice>,
    pub history_log: SessionHistoryLog,
    pub fate_weaver_context: Context,
    pub upper_narrator_context: Context,
    pub protagonist_context: Context,
    pub simulators: Vec<SimulatorArchiveState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatorArchiveState {
    #[serde(rename = "kind", default = "default_simulator_kind", skip_serializing)]
    legacy_kind: LegacyAgentArchiveKind,
    #[serde(alias = "output_kind")]
    pub output_type: AgentOutputType,
    pub name: String,
    #[serde(default)]
    pub sys_prompt: String,
    pub context: Context,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyAgentArchiveKind {
    Simulator,
    Applicator,
    Player,
}

impl SimulatorArchiveState {
    pub fn new(
        output_type: AgentOutputType,
        name: impl Into<String>,
        sys_prompt: impl Into<String>,
        context: Context,
    ) -> Self {
        Self {
            legacy_kind: LegacyAgentArchiveKind::Simulator,
            output_type,
            name: name.into(),
            sys_prompt: sys_prompt.into(),
            context,
        }
    }
}

pub(crate) struct ArchiveView {
    pub phase: TurnStage,
    pub turn_index: u64,
    pub active_turn_id: u64,
    pub world_snapshot: WorldSnapshot,
    pub committed_action: String,
    pub choices: Vec<PendingProtagonistChoice>,
    pub history_log: SessionHistoryLog,
}

pub(crate) fn archive_view_from_current(
    flow: TurnFlow,
    world_snapshot: WorldSnapshot,
    committed_action: String,
    choices: Vec<PendingProtagonistChoice>,
    history_log: SessionHistoryLog,
) -> ArchiveView {
    ArchiveView {
        phase: flow.stage,
        turn_index: flow.turn_index,
        active_turn_id: flow.active_turn_id,
        world_snapshot,
        committed_action,
        choices,
        history_log,
    }
}

pub(crate) fn completed_dialogue_archive_view(
    history_log: &SessionHistoryLog,
) -> Result<ArchiveView, String> {
    let completed_round = history_log
        .rounds
        .iter()
        .rev()
        .find(|entry| {
            entry
                .narration_text
                .as_deref()
                .is_some_and(|text| !text.trim().is_empty())
                && entry.world_snapshot.is_some()
        })
        .ok_or_else(|| "当前会话还没有已完成的对话可用于创建存档".to_string())?;
    let completed_round_id = completed_round.round;
    let world_snapshot = completed_round
        .world_snapshot
        .clone()
        .expect("completed dialogue entries are filtered to include world snapshots");
    let choices = completed_round.choices.clone();
    let committed_action = history_log
        .rounds
        .iter()
        .rev()
        .filter(|entry| entry.round < completed_round_id)
        .filter_map(|entry| entry.committed_action.as_deref())
        .find(|action| !action.trim().is_empty())
        .unwrap_or("start")
        .to_string();
    let mut archive_history = SessionHistoryLog {
        rounds: history_log
            .rounds
            .iter()
            .filter(|entry| entry.round <= completed_round_id)
            .cloned()
            .collect(),
    };

    if let Some(entry) = archive_history
        .rounds
        .iter_mut()
        .find(|entry| entry.round == completed_round_id)
    {
        entry.committed_action = None;
    }

    Ok(ArchiveView {
        phase: TurnStage::AwaitingPlayer,
        turn_index: completed_round_id,
        active_turn_id: completed_round_id,
        world_snapshot,
        committed_action,
        choices,
        history_log: archive_history,
    })
}

pub(crate) fn validate_archive_state(state: &SessionArchiveState) -> Result<(), String> {
    if !state.phase.is_stable() || state.phase == TurnStage::Failed {
        return Err("归档会话不在可恢复的稳定态".to_string());
    }
    if state.active_turn_id < state.turn_index {
        return Err("归档会话的 active_turn_id 不能小于 turn_index".to_string());
    }
    if state
        .simulators
        .iter()
        .any(|simulator| simulator.legacy_kind != LegacyAgentArchiveKind::Simulator)
    {
        return Err("归档的 simulators 只能包含 Simulator".to_string());
    }
    if !state.simulators.is_empty()
        && state
            .simulators
            .iter()
            .filter(|simulator| simulator.output_type == AgentOutputType::Json)
            .count()
            != 1
    {
        return Err("归档必须包含且只能包含一个 JSON Simulator".to_string());
    }
    if state
        .simulators
        .iter()
        .any(|simulator| simulator.output_type != AgentOutputType::Json)
    {
        return Err("归档的 simulators 只能包含 JSON Simulator".to_string());
    }
    Ok(())
}

fn default_simulator_kind() -> LegacyAgentArchiveKind {
    LegacyAgentArchiveKind::Simulator
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::{history::RoundHistoryEntry, protagonist_action::ProtagonistOption};

    #[test]
    fn non_stable_archive_view_uses_latest_completed_dialogue() {
        let history_log = SessionHistoryLog {
            rounds: vec![
                RoundHistoryEntry {
                    round: 1,
                    world_snapshot: Some(WorldSnapshot {
                        round: 1,
                        scene_title: "旧厅".to_string(),
                        ..WorldSnapshot::default()
                    }),
                    narration_text: Some("旧厅里的灯醒了。".to_string()),
                    choices: vec![PendingProtagonistChoice {
                        id: "choice-1".to_string(),
                        option: ProtagonistOption {
                            title: "推门".to_string(),
                            action: "推门".to_string(),
                            motivation_and_risk: "门后有风".to_string(),
                        },
                    }],
                    committed_action: Some("推门".to_string()),
                },
                RoundHistoryEntry {
                    round: 2,
                    world_snapshot: Some(WorldSnapshot {
                        round: 2,
                        scene_title: "雾廊".to_string(),
                        ..WorldSnapshot::default()
                    }),
                    narration_text: Some("雾廊把脚步声吞下。".to_string()),
                    choices: vec![PendingProtagonistChoice {
                        id: "choice-1".to_string(),
                        option: ProtagonistOption {
                            title: "点灯".to_string(),
                            action: "点灯".to_string(),
                            motivation_and_risk: "光会暴露位置".to_string(),
                        },
                    }],
                    committed_action: Some("点灯".to_string()),
                },
                RoundHistoryEntry {
                    round: 3,
                    world_snapshot: Some(WorldSnapshot {
                        round: 3,
                        scene_title: "未完成的下一幕".to_string(),
                        ..WorldSnapshot::default()
                    }),
                    narration_text: None,
                    choices: Vec::new(),
                    committed_action: None,
                },
            ],
        };

        let archive_view = completed_dialogue_archive_view(&history_log).unwrap();

        assert_eq!(archive_view.phase, TurnStage::AwaitingPlayer);
        assert_eq!(archive_view.turn_index, 2);
        assert_eq!(archive_view.active_turn_id, 2);
        assert_eq!(archive_view.world_snapshot.scene_title, "雾廊");
        assert_eq!(archive_view.committed_action, "推门");
        assert_eq!(archive_view.choices[0].option.action, "点灯");
        assert_eq!(archive_view.history_log.rounds.len(), 2);
        assert_eq!(archive_view.history_log.rounds[1].committed_action, None);
    }

    #[test]
    fn non_stable_archive_view_rejects_history_without_completed_dialogue() {
        let history_log = SessionHistoryLog {
            rounds: vec![RoundHistoryEntry {
                round: 1,
                world_snapshot: Some(WorldSnapshot {
                    round: 1,
                    ..WorldSnapshot::default()
                }),
                narration_text: None,
                choices: Vec::new(),
                committed_action: None,
            }],
        };

        assert_eq!(
            completed_dialogue_archive_view(&history_log).err().unwrap(),
            "当前会话还没有已完成的对话可用于创建存档"
        );
    }

    #[test]
    fn simulator_archive_omits_legacy_kind_when_serializing() {
        let archive =
            SimulatorArchiveState::new(AgentOutputType::Json, "FateWeaver", "", Context::default());
        let value = serde_json::to_value(archive).expect("archive should serialize");

        assert!(value.get("kind").is_none());
        assert_eq!(value["output_type"], "json");
    }
}

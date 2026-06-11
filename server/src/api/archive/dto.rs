use serde::{Deserialize, Serialize};

use agent::agent::context::Context;
use story_engine::components::{
    outcome::{PendingCharacterChoice, PlayerActionItem},
    world_snapshot::WorldSnapshot,
};

use crate::session_history::{SessionHistoryLog, TurnPhase};

/// 内部恢复用：TurnState 的可序列化快照
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TurnStateArchive {
    pub phase: TurnPhase,
    pub turn_index: u64,
    pub active_turn_id: u64,
}

/// 内部恢复用：角色决策状态快照。单玩家模式是只有 character 一项的特例。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct CharacterDecisionArchive {
    pub committed_actions: Vec<PlayerActionItem>,
    pub choices: Vec<PendingCharacterChoice>,
}

/// 整个 session 的内部归档载荷
/// 这是恢复真源，不是面向前端的展示 DTO。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionArchivePayload {
    /// 复用旧 session_id
    pub session_id: String,

    /// 存档展示标题，用于列表页
    pub title: String,

    /// 原始文本资料，按你的要求只存文本
    #[serde(default = "default_character_name")]
    pub character_name: String,
    pub world_profile: String,
    pub character_profile: String,
    #[serde(default)]
    pub key_story_beats: String,

    /// 当前回合状态
    pub turn_state: TurnStateArchive,

    pub fate_weaver: Context,
    /// 唯一 Narrator 与角色候选行动 Agent 的完整 Context
    pub upper_narrator: Context,
    pub character_agent: Context,
    /// 当前世界状态
    pub world_snapshot: WorldSnapshot,

    /// 当前角色决策状态，保证选项可继续提交
    pub character_decision: CharacterDecisionArchive,

    /// 每轮结构化历史，保证前端可恢复完整时间线
    pub history_log: SessionHistoryLog,
}

fn default_character_name() -> String {
    "玩家角色".to_string()
}

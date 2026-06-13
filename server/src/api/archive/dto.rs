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

    /// 完整 session 数据库快照，用于保留故事线上的全部分支。
    pub database_archive: SessionDatabaseArchive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionDatabaseArchive {
    pub active_node_id: String,
    pub total_node_count: i64,
    pub story_nodes: Vec<StoryNodeArchive>,
    pub story_edges: Vec<StoryEdgeArchive>,
    pub story_edge_actions: Vec<StoryEdgeActionArchive>,
    pub entity_flow_outputs: Vec<EntityFlowOutputArchive>,
    pub entity_context_items: Vec<EntityContextItemArchive>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StoryNodeArchive {
    pub node_id: String,
    pub parent_node_id: Option<String>,
    pub node_depth: i64,
    pub sequence_index: i64,
    pub phase: String,
    pub flow_end: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StoryEdgeArchive {
    pub from_node_id: String,
    pub to_node_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StoryEdgeActionArchive {
    pub from_node_id: String,
    pub to_node_id: String,
    pub character_name: String,
    pub player_id: Option<String>,
    pub action_type: String,
    pub title: String,
    pub action: String,
    pub motivation_and_risk: String,
    pub submitted_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EntityFlowOutputArchive {
    pub node_id: String,
    pub stage: String,
    pub entity_name: String,
    pub output_type: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EntityContextItemArchive {
    pub node_id: String,
    pub entity_name: String,
    pub item_index: i64,
    pub item_kind: String,
    pub message_role: Option<String>,
    pub content: Option<String>,
    pub created_at: String,
}

fn default_character_name() -> String {
    "玩家角色".to_string()
}

use bevy_ecs::component::Component;
use serde::{Deserialize, Serialize};

/// 世界 Agent 单轮完整输出
#[derive(Component, Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "snake_case")]
pub struct WorldSnapshot {
    /// 展示/序列化轮次副本；流程真源是会话实体上的 TurnFlow。
    pub round: u64,
    pub scene_title: String,
    /// 故事内绝对时间，如 "第三日 凌晨两点一刻"
    pub time_absolute: String,
    /// 关键时间压力的描述
    #[serde(default)]
    pub time_relative: Option<String>,
    pub location_name: String,
    pub location_exits: Vec<String>,
    /// 当前地点的特殊状态
    pub location_status: String,
    /// 场景整体氛围与感官细节
    pub description: String,
    /// 刚刚发生或正在发生的核心事件
    pub current_event: String,
    /// 本段情境中玩家角色可感知的新线索或信息
    pub new_info: Vec<String>,
    /// 情境中蕴藏的冲突、压力或两难
    pub inner_conflict: String,
    /// 必须出现在最终叙事中的关键信息（伏笔、情绪真相、逻辑事实）
    pub hard_anchors: Vec<String>,
    pub pace: String,
    pub atmosphere: String,
    pub focal_point: String,
    /// 当前轮次是否已到达故事结局
    #[serde(default)]
    pub is_ending: bool,
    /// 结局的情绪基调或主题；非结局时为空
    #[serde(default)]
    pub ending_type: Option<String>,
    /// 玩家角色当前身心状态的文学化描述
    pub character_condition: String,
    /// 玩家角色已确切知晓的剧情秘密
    pub character_known_secrets: Vec<String>,
    pub npcs: Vec<NpcState>,
    pub items: Vec<ItemState>,
    pub events_in_progress: Vec<OngoingEvent>,
    pub unsolved_threads: Vec<String>,
    /// 当前叙事节奏的简评与下一轮建议
    pub pacing_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "snake_case")]
pub struct NpcState {
    pub name: String,
    pub location: String,
    /// 当前情绪与心理状态的描述
    pub mood: String,
    /// 对玩家角色的态度，使用自然语言，如 "既信赖又隐隐藏着愧疚"
    pub attitude: String,
    /// 此刻最直接的意图或行动倾向
    pub goal: String,
    /// 该 NPC 保守的秘密或关键信息
    pub secrets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "snake_case")]
pub struct ItemState {
    pub name: String,
    /// 物品当前所在地或持有者
    pub location: String,
    /// 状态或可用性，如 "已被激活，光芒渐弱"
    pub status: String,
    /// 被玩家角色察觉的程度，如 "刚瞥见"、"未察觉"
    #[serde(default, alias = "aware")]
    pub awareness: String,
    /// 此物与主线伏笔的关联
    pub relevance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "snake_case")]
pub struct OngoingEvent {
    pub name: String,
    /// 当前态势的叙事描述，如 "闸门已被撞开一半，脚步声近在咫尺"
    pub status: String,
    /// 可能导致事态升级的触发条件
    pub escalation_trigger: String,
}

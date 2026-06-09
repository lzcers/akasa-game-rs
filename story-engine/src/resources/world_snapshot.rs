use bevy_ecs::component::Component;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt::Write;

/// 世界 Agent 单轮完整输出
#[derive(Component, Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "snake_case")]
pub struct WorldSnapshot {
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
    /// 本段情境中主角可感知的新线索或信息
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
    /// 主角当前身心状态的文学化描述
    pub protagonist_condition: String,
    /// 主角已确切知晓的剧情秘密
    pub protagonist_known_secrets: Vec<String>,
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
    /// 对主角的态度，使用自然语言，如 "既信赖又隐隐藏着愧疚"
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
    /// 被主角察觉的程度，如 "刚瞥见"、"未察觉"
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

impl WorldSnapshot {
    /// 将世界状态快照转换为可读的流水账文本。
    pub fn to_ledger(&self) -> String {
        let mut out = String::new();

        writeln!(out, "【世界状态｜第{}轮】", self.round).unwrap();

        write!(out, "时间：{}", self.time_absolute).unwrap();
        if let Some(ref rel) = self.time_relative {
            write!(out, "，{}", rel).unwrap();
        }
        writeln!(out).unwrap();

        writeln!(out, "场景：{}", self.scene_title).unwrap();
        write!(out, "地点：{}", self.location_name).unwrap();
        if !self.location_exits.is_empty() {
            write!(out, "出口有").unwrap();
            write!(out, "{}。", self.location_exits.join("、")).unwrap();
        }
        writeln!(out).unwrap();
        writeln!(out, "地点状态：{}", self.location_status).unwrap();
        writeln!(out, "场景描述：{}", self.description).unwrap();
        writeln!(out, "当前事件：{}", self.current_event).unwrap();

        if !self.new_info.is_empty() {
            writeln!(out, "新信息：").unwrap();
            for info in &self.new_info {
                writeln!(out, "- {}", info).unwrap();
            }
        }

        writeln!(out, "内在冲突：{}", self.inner_conflict).unwrap();

        if !self.hard_anchors.is_empty() {
            writeln!(out, "硬锚点：").unwrap();
            for anchor in &self.hard_anchors {
                writeln!(out, "- {}", anchor).unwrap();
            }
        }

        writeln!(out, "节奏：{}", self.pace).unwrap();
        writeln!(out, "氛围：{}", self.atmosphere).unwrap();
        if !self.focal_point.is_empty() {
            writeln!(out, "镜头焦点：{}", self.focal_point).unwrap();
        }

        writeln!(out, "主角状态：{}", self.protagonist_condition).unwrap();
        if !self.protagonist_known_secrets.is_empty() {
            writeln!(
                out,
                "主角已知秘密：{}",
                self.protagonist_known_secrets.join("；")
            )
            .unwrap();
        }

        if !self.npcs.is_empty() {
            writeln!(out, "NPC：").unwrap();
            for (i, npc) in self.npcs.iter().enumerate() {
                write!(out, "{}. {}（位置：{}）", i + 1, npc.name, npc.location).unwrap();
                write!(out, "——情绪：{}", npc.mood).unwrap();
                write!(out, " 态度：{}", npc.attitude).unwrap();
                write!(out, " 当前目标：{}", npc.goal).unwrap();
                if !npc.secrets.is_empty() {
                    write!(out, " 秘密：{}", npc.secrets.join("；")).unwrap();
                }
                writeln!(out).unwrap();
            }
        }

        if !self.items.is_empty() {
            writeln!(out, "关键物品：").unwrap();
            for item in &self.items {
                writeln!(
                    out,
                    "- {}（{}，状态：{}，主角察觉：{}，剧情关联：{}）",
                    item.name, item.location, item.status, item.awareness, item.relevance
                )
                .unwrap();
            }
        }

        if !self.events_in_progress.is_empty() {
            writeln!(out, "进行中的事件：").unwrap();
            for (i, ev) in self.events_in_progress.iter().enumerate() {
                writeln!(
                    out,
                    "{}. {}：{}，触发升级条件：{}",
                    i + 1,
                    ev.name,
                    ev.status,
                    ev.escalation_trigger
                )
                .unwrap();
            }
        }

        if !self.unsolved_threads.is_empty() {
            writeln!(out, "未解伏笔：").unwrap();
            for thread in &self.unsolved_threads {
                writeln!(out, "- {}", thread).unwrap();
            }
        }

        writeln!(out, "叙事节奏：{}", self.pacing_note).unwrap();

        out
    }

    /// 生成给故事 Agent 的 JSON 创作输入。
    /// `protagonist_action` 是主角上一轮的实际行动，可能为空。
    pub fn to_story_prompt(&self, protagonist_action: Option<&str>) -> String {
        let previous_protagonist_action =
            protagonist_action.filter(|action| *action != "start" && !action.trim().is_empty());
        let npcs: Vec<_> = self
            .npcs
            .iter()
            .map(|npc| {
                json!({
                    "name": &npc.name,
                    "location": &npc.location,
                    "mood": &npc.mood,
                    "attitude": &npc.attitude,
                    "goal": &npc.goal,
                })
            })
            .collect();
        let instruction = if self.is_ending {
            "请根据本 JSON 输入写出本轮结局，保持与前文连贯，并完成情绪与事件的收束。"
        } else {
            "请根据本 JSON 输入编写本轮故事，保持与你输出的文本连贯性。"
        };

        serde_json::to_string_pretty(&json!({
            "task": "write_story",
            "round": self.round,
            "previous_protagonist_action": previous_protagonist_action,
            "scene_title": &self.scene_title,
            "time_absolute": &self.time_absolute,
            "time_relative": &self.time_relative,
            "location_name": &self.location_name,
            "location_exits": &self.location_exits,
            "location_status": &self.location_status,
            "description": &self.description,
            "current_event": &self.current_event,
            "new_info": &self.new_info,
            "inner_conflict": &self.inner_conflict,
            "hard_anchors": &self.hard_anchors,
            "pace": &self.pace,
            "atmosphere": &self.atmosphere,
            "focal_point": &self.focal_point,
            "is_ending": self.is_ending,
            "ending_type": &self.ending_type,
            "protagonist_condition": &self.protagonist_condition,
            "protagonist_known_secrets": &self.protagonist_known_secrets,
            "npcs": npcs,
            "items": &self.items,
            "events_in_progress": &self.events_in_progress,
            "instruction": instruction,
        }))
        .expect("story prompt payload should serialize")
    }

    /// 生成给主角 Agent 的 JSON 决策输入。
    pub fn to_protagonist_prompt(&self, protagonist_action: Option<&str>) -> String {
        let previous_protagonist_action =
            protagonist_action.filter(|action| *action != "start" && !action.trim().is_empty());
        let npcs: Vec<_> = self
            .npcs
            .iter()
            .map(|npc| {
                json!({
                    "name": &npc.name,
                    "location": &npc.location,
                    "mood": &npc.mood,
                    "attitude": &npc.attitude,
                    "goal": &npc.goal,
                })
            })
            .collect();

        serde_json::to_string_pretty(&json!({
            "task": "generate_protagonist_options",
            "round": self.round,
            "previous_protagonist_action": previous_protagonist_action,
            "scene_title": &self.scene_title,
            "time_absolute": &self.time_absolute,
            "time_relative": &self.time_relative,
            "location_name": &self.location_name,
            "location_exits": &self.location_exits,
            "location_status": &self.location_status,
            "description": &self.description,
            "current_event": &self.current_event,
            "new_info": &self.new_info,
            "inner_conflict": &self.inner_conflict,
            "protagonist_condition": &self.protagonist_condition,
            "protagonist_known_secrets": &self.protagonist_known_secrets,
            "npcs": npcs,
            "items": &self.items,
            "events_in_progress": &self.events_in_progress,
            "instruction": "请根据本 JSON 输入生成符合主角认知、性格与身心状态的可行行动选项。",
        }))
        .expect("protagonist prompt payload should serialize")
    }
}

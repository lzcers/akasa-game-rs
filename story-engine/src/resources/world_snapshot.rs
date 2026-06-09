use bevy_ecs::component::Component;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt::Write;

/// 世界 Agent 单轮完整输出
#[derive(Component, Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
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

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn sample_snapshot() -> WorldSnapshot {
        WorldSnapshot {
            round: 3,
            scene_title: "沉星观测台".to_string(),
            time_absolute: "第三日 凌晨两点一刻".to_string(),
            time_relative: Some("距离穹顶自毁只剩约一炷香".to_string()),
            location_name: "观测台主厅".to_string(),
            location_exits: vec!["北侧舷梯".to_string(), "破损升降井".to_string()],
            location_status: "星图开始反转，地面符文发烫".to_string(),
            description: "蓝白色星辉压在铜制穹顶上，空气里有铁锈味。".to_string(),
            current_event: "看守者正撞开内层闸门。".to_string(),
            new_info: vec!["闸门裂缝里露出父亲徽记。".to_string()],
            inner_conflict: "继续解读星图会拖慢撤离。".to_string(),
            hard_anchors: vec!["必须写出匕首柄上的徽记。".to_string()],
            pace: "紧迫".to_string(),
            atmosphere: "冷光与灼热机械声交织".to_string(),
            focal_point: "洛寒额头的汗滴落在符文上".to_string(),
            protagonist_condition: "灵能过度消耗，面色苍白。".to_string(),
            protagonist_known_secrets: vec!["第七封印坐标藏在反转星图中。".to_string()],
            npcs: vec![NpcState {
                name: "伊瑟琳".to_string(),
                location: "控制台旁".to_string(),
                mood: "紧张".to_string(),
                attitude: "信赖却愧疚".to_string(),
                goal: "拖住看守者".to_string(),
                secrets: vec!["她认识看守者的真实身份。".to_string()],
            }],
            items: vec![ItemState {
                name: "旧匕首".to_string(),
                location: "看守者腰间".to_string(),
                status: "半露出鞘".to_string(),
                awareness: "刚瞥见".to_string(),
                relevance: "与主角父亲有关".to_string(),
            }],
            events_in_progress: vec![OngoingEvent {
                name: "穹顶自毁".to_string(),
                status: "倒计时加速".to_string(),
                escalation_trigger: "星图解读失败".to_string(),
            }],
            unsolved_threads: vec!["看守者身份".to_string()],
            pacing_note: "动作强度高，下一轮需要信息揭示。".to_string(),
            ..WorldSnapshot::default()
        }
    }

    #[test]
    fn story_prompt_is_json_payload() {
        let prompt = sample_snapshot().to_story_prompt(Some("洛寒按住符文继续解读星图。"));
        let payload: Value = serde_json::from_str(&prompt).expect("prompt should be JSON");

        assert_eq!(payload["task"], "write_story");
        assert_eq!(payload["round"], 3);
        assert_eq!(
            payload["previous_protagonist_action"],
            "洛寒按住符文继续解读星图。"
        );
        assert_eq!(payload["scene_title"], "沉星观测台");
        assert_eq!(payload["location_exits"][0], "北侧舷梯");
        assert!(payload["story_context"].is_null());
        assert!(payload["npcs"][0]["secrets"].is_null());
        assert!(payload["unsolved_threads"].is_null());
        assert!(payload["pacing_note"].is_null());
    }

    #[test]
    fn story_prompt_uses_ending_instruction() {
        let mut snapshot = sample_snapshot();
        snapshot.is_ending = true;
        snapshot.ending_type = Some("bittersweet".to_string());

        let prompt = snapshot.to_story_prompt(Some("start"));
        let payload: Value = serde_json::from_str(&prompt).expect("prompt should be JSON");

        assert!(payload["previous_protagonist_action"].is_null());
        assert_eq!(payload["is_ending"], true);
        assert_eq!(payload["ending_type"], "bittersweet");
        assert!(
            payload["instruction"]
                .as_str()
                .expect("instruction should be a string")
                .contains("结局")
        );
    }

    #[test]
    fn protagonist_prompt_is_json_payload() {
        let prompt = sample_snapshot().to_protagonist_prompt(Some(""));
        let payload: Value = serde_json::from_str(&prompt).expect("prompt should be JSON");

        assert_eq!(payload["task"], "generate_protagonist_options");
        assert_eq!(payload["round"], 3);
        assert!(payload["previous_protagonist_action"].is_null());
        assert_eq!(payload["protagonist_condition"], "灵能过度消耗，面色苍白。");
        assert!(payload["decision_context"].is_null());
        assert!(payload["npcs"][0]["secrets"].is_null());
        assert!(payload["hard_anchors"].is_null());
        assert!(payload["unsolved_threads"].is_null());
        assert!(payload["pacing_note"].is_null());
        assert!(
            payload["instruction"]
                .as_str()
                .expect("instruction should be a string")
                .contains("行动选项")
        );
    }
}

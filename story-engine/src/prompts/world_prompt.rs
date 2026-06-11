use std::fmt::Write;

use serde_json::json;

use crate::components::world_snapshot::WorldSnapshot;

pub fn world_snapshot_ledger(snapshot: &WorldSnapshot) -> String {
    let mut out = String::new();

    writeln!(out, "【世界状态｜第{}轮】", snapshot.round).unwrap();

    write!(out, "时间：{}", snapshot.time_absolute).unwrap();
    if let Some(ref rel) = snapshot.time_relative {
        write!(out, "，{}", rel).unwrap();
    }
    writeln!(out).unwrap();

    writeln!(out, "场景：{}", snapshot.scene_title).unwrap();
    write!(out, "地点：{}", snapshot.location_name).unwrap();
    if !snapshot.location_exits.is_empty() {
        write!(out, "出口有").unwrap();
        write!(out, "{}。", snapshot.location_exits.join("、")).unwrap();
    }
    writeln!(out).unwrap();
    writeln!(out, "地点状态：{}", snapshot.location_status).unwrap();
    writeln!(out, "场景描述：{}", snapshot.description).unwrap();
    writeln!(out, "当前事件：{}", snapshot.current_event).unwrap();

    if !snapshot.new_info.is_empty() {
        writeln!(out, "新信息：").unwrap();
        for info in &snapshot.new_info {
            writeln!(out, "- {}", info).unwrap();
        }
    }

    writeln!(out, "内在冲突：{}", snapshot.inner_conflict).unwrap();

    if !snapshot.hard_anchors.is_empty() {
        writeln!(out, "硬锚点：").unwrap();
        for anchor in &snapshot.hard_anchors {
            writeln!(out, "- {}", anchor).unwrap();
        }
    }

    writeln!(out, "节奏：{}", snapshot.pace).unwrap();
    writeln!(out, "氛围：{}", snapshot.atmosphere).unwrap();
    if !snapshot.focal_point.is_empty() {
        writeln!(out, "镜头焦点：{}", snapshot.focal_point).unwrap();
    }

    writeln!(out, "玩家角色状态：{}", snapshot.character_condition).unwrap();
    if !snapshot.character_known_secrets.is_empty() {
        writeln!(
            out,
            "玩家角色已知秘密：{}",
            snapshot.character_known_secrets.join("；")
        )
        .unwrap();
    }

    if !snapshot.npcs.is_empty() {
        writeln!(out, "NPC：").unwrap();
        for (i, npc) in snapshot.npcs.iter().enumerate() {
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

    if !snapshot.items.is_empty() {
        writeln!(out, "关键物品：").unwrap();
        for item in &snapshot.items {
            writeln!(
                out,
                "- {}（{}，状态：{}，玩家角色察觉：{}，剧情关联：{}）",
                item.name, item.location, item.status, item.awareness, item.relevance
            )
            .unwrap();
        }
    }

    if !snapshot.events_in_progress.is_empty() {
        writeln!(out, "进行中的事件：").unwrap();
        for (i, ev) in snapshot.events_in_progress.iter().enumerate() {
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

    if !snapshot.unsolved_threads.is_empty() {
        writeln!(out, "未解伏笔：").unwrap();
        for thread in &snapshot.unsolved_threads {
            writeln!(out, "- {}", thread).unwrap();
        }
    }

    writeln!(out, "叙事节奏：{}", snapshot.pacing_note).unwrap();

    out
}

pub fn story_prompt(snapshot: &WorldSnapshot, character_action: Option<&str>) -> String {
    let previous_character_action =
        character_action.filter(|action| *action != "start" && !action.trim().is_empty());
    let npcs: Vec<_> = snapshot
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
    let instruction = if snapshot.is_ending {
        "请根据本 JSON 输入写出本轮结局，保持与前文连贯，并完成情绪与事件的收束。"
    } else {
        "请根据本 JSON 输入编写本轮故事，保持与你输出的文本连贯性。"
    };

    serde_json::to_string_pretty(&json!({
        "task": "write_story",
        "round": snapshot.round,
        "previous_character_action": previous_character_action,
        "scene_title": &snapshot.scene_title,
        "time_absolute": &snapshot.time_absolute,
        "time_relative": &snapshot.time_relative,
        "location_name": &snapshot.location_name,
        "location_exits": &snapshot.location_exits,
        "location_status": &snapshot.location_status,
        "description": &snapshot.description,
        "current_event": &snapshot.current_event,
        "new_info": &snapshot.new_info,
        "inner_conflict": &snapshot.inner_conflict,
        "hard_anchors": &snapshot.hard_anchors,
        "pace": &snapshot.pace,
        "atmosphere": &snapshot.atmosphere,
        "focal_point": &snapshot.focal_point,
        "is_ending": snapshot.is_ending,
        "ending_type": &snapshot.ending_type,
        "character_condition": &snapshot.character_condition,
        "character_known_secrets": &snapshot.character_known_secrets,
        "npcs": npcs,
        "items": &snapshot.items,
        "events_in_progress": &snapshot.events_in_progress,
        "instruction": instruction,
    }))
    .expect("story prompt payload should serialize")
}

pub fn character_prompt(snapshot: &WorldSnapshot, character_action: Option<&str>) -> String {
    let previous_character_action =
        character_action.filter(|action| *action != "start" && !action.trim().is_empty());
    let npcs: Vec<_> = snapshot
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
        "task": "generate_character_options",
        "round": snapshot.round,
        "previous_character_action": previous_character_action,
        "scene_title": &snapshot.scene_title,
        "time_absolute": &snapshot.time_absolute,
        "time_relative": &snapshot.time_relative,
        "location_name": &snapshot.location_name,
        "location_exits": &snapshot.location_exits,
        "location_status": &snapshot.location_status,
        "description": &snapshot.description,
        "current_event": &snapshot.current_event,
        "new_info": &snapshot.new_info,
        "inner_conflict": &snapshot.inner_conflict,
        "character_condition": &snapshot.character_condition,
        "character_known_secrets": &snapshot.character_known_secrets,
        "npcs": npcs,
        "items": &snapshot.items,
        "events_in_progress": &snapshot.events_in_progress,
        "instruction": "请根据本 JSON 输入生成符合玩家角色认知、性格与身心状态的可行行动选项。",
    }))
    .expect("character prompt payload should serialize")
}

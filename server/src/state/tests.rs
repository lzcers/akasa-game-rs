use super::*;
use crate::api::archive::{CharacterDecisionArchive, SessionArchivePayload, TurnStateArchive};
use agent::agent::context::Context;
use rusqlite::{Connection, params};
use serde_json::Value;
use story_engine::components::{
    outcome::{CharacterOption, CharacterOptions, PendingCharacterChoice},
    world_snapshot::WorldSnapshot,
};

use crate::session_history::{RoundHistoryEntry, SessionHistoryLog, TurnPhase};

fn test_state() -> AppState {
    test_state_with_config(SessionLifecycleConfig {
        idle_ttl: Duration::from_secs(30 * 60),
        ended_ttl: Duration::from_secs(5 * 60),
        max_hot_sessions: 200,
        reaper_interval: Duration::from_secs(60),
        live_event_history_capacity: DEFAULT_EVENT_HISTORY_CAPACITY,
    })
}

fn test_state_with_config(lifecycle_config: SessionLifecycleConfig) -> AppState {
    AppState::with_lifecycle_config(
        std::env::temp_dir().join(format!("akasa-state-{}.sqlite3", Uuid::new_v4().simple())),
        lifecycle_config,
        false,
    )
}

fn test_state_at_path(path: PathBuf) -> AppState {
    AppState::with_lifecycle_config(
        path,
        SessionLifecycleConfig {
            idle_ttl: Duration::from_secs(30 * 60),
            ended_ttl: Duration::from_secs(5 * 60),
            max_hot_sessions: 200,
            reaper_interval: Duration::from_secs(60),
            live_event_history_capacity: DEFAULT_EVENT_HISTORY_CAPACITY,
        },
        false,
    )
}

#[tokio::test]
async fn load_game_session_from_archive_restores_runtime_into_registry() {
    let state = test_state();
    let compressed =
        archive::compress_archive_payload(&sample_payload()).expect("archive compresses");
    let restored = state
        .load_game_session_from_archive(compressed)
        .await
        .expect("archive should restore");

    assert_eq!(restored.session_id, "session-from-slot");
    assert_eq!(restored.phase, TurnPhase::AwaitingPlayer);
    assert_eq!(restored.turn_index, 8);
    assert_eq!(restored.active_turn_id, 7);
    assert_eq!(restored.current_outcome, "尚未做出选择");
    assert_eq!(restored.choices.len(), 1);

    let loaded_again = state
        .get_game_session_world("session-from-slot")
        .await
        .expect("restored session should be queryable");
    assert_eq!(loaded_again.world_state.scene_title, "钟楼阴影");
}

#[tokio::test]
async fn load_archive_persists_full_history_but_world_response_omits_history() {
    let state = test_state();
    let total_rounds = 20;
    let compressed = archive::compress_archive_payload(&sample_payload_with_rounds(total_rounds))
        .expect("archive compresses");

    state
        .load_game_session_from_archive(compressed)
        .await
        .expect("archive should restore");

    let full_history = state
        .get_game_session_rounds("session-from-slot", None, 100)
        .await
        .expect("full history page should load");

    assert_eq!(full_history.rounds.len(), total_rounds as usize);
    assert_eq!(
        full_history.rounds.first().map(|entry| entry.round),
        Some(1)
    );
    assert_eq!(
        full_history.rounds.last().map(|entry| entry.round),
        Some(total_rounds)
    );
    assert!(!full_history.has_more);
}

#[tokio::test]
async fn load_archive_restores_selected_choice_for_each_history_round() {
    let state = test_state();
    let total_rounds = 3;
    let compressed = archive::compress_archive_payload(&sample_payload_with_rounds(total_rounds))
        .expect("archive compresses");

    state
        .load_game_session_from_archive(compressed)
        .await
        .expect("archive should restore");

    let history = state
        .get_game_session_rounds("session-from-slot", None, 100)
        .await
        .expect("history page should load");

    assert_eq!(history.rounds.len(), total_rounds as usize);
    for round in 1..total_rounds {
        let entry = history
            .rounds
            .iter()
            .find(|entry| entry.round == round)
            .expect("round should be present");
        assert_eq!(entry.committed_actions.len(), 1);
        assert_eq!(entry.committed_actions[0].action, format!("行动-{round}"));
        assert_eq!(
            entry.selected_choice_text.as_deref(),
            Some(format!("选择-{round}").as_str())
        );
    }
    let active_entry = history
        .rounds
        .iter()
        .find(|entry| entry.round == total_rounds)
        .expect("active round should be present");
    assert!(active_entry.committed_actions.is_empty());
    assert_eq!(active_entry.selected_choice_text, None);
}

#[test]
fn nonstable_archive_payload_falls_back_to_latest_completed_db_round() {
    let mut payload = sample_payload_with_rounds(3);
    payload.title = "第4轮：生成中的风暴".to_string();
    payload.turn_state.phase = TurnPhase::Simulation;
    payload.turn_state.turn_index = 3;
    payload.turn_state.active_turn_id = 4;
    payload.world_snapshot = WorldSnapshot {
        round: 4,
        scene_title: "生成中的风暴".to_string(),
        ..WorldSnapshot::default()
    };

    stabilize_archive_payload_from_history(&mut payload, None, None)
        .expect("nonstable payload should use latest completed round");

    assert_eq!(payload.turn_state.phase, TurnPhase::AwaitingPlayer);
    assert_eq!(payload.turn_state.turn_index, 3);
    assert_eq!(payload.turn_state.active_turn_id, 3);
    assert_eq!(payload.world_snapshot.scene_title, "第3轮");
    assert_eq!(
        summarize_actions(&payload.character_decision.committed_actions),
        "行动-2"
    );
    assert_eq!(
        payload.character_decision.choices[0].option.action,
        "行动-3"
    );
    assert_eq!(payload.history_log.rounds.len(), 3);
    assert!(payload.history_log.rounds[2].committed_actions.is_empty());
    assert_eq!(payload.title, "第3轮：第3轮");
}

#[test]
fn archive_payload_can_stabilize_to_requested_completed_round() {
    let mut payload = sample_payload_with_rounds(4);
    payload.turn_state.phase = TurnPhase::AwaitingPlayer;
    payload.turn_state.turn_index = 4;
    payload.turn_state.active_turn_id = 4;

    stabilize_archive_payload_from_history(&mut payload, None, Some(2))
        .expect("requested completed round should be shareable");

    assert_eq!(payload.turn_state.phase, TurnPhase::AwaitingPlayer);
    assert_eq!(payload.turn_state.turn_index, 2);
    assert_eq!(payload.turn_state.active_turn_id, 2);
    assert_eq!(payload.history_log.rounds.len(), 2);
    assert_eq!(payload.world_snapshot.scene_title, "第2轮");
    assert_eq!(
        summarize_actions(&payload.character_decision.committed_actions),
        "行动-1"
    );
    assert_eq!(
        payload.character_decision.choices[0].option.action,
        "行动-2"
    );
}

#[test]
fn story_edges_overlay_committed_actions() {
    let rounds = vec![RoundHistoryEntry {
        round: 2,
        committed_actions: vec![PlayerActionItem::character_free_text("旧行动")],
        ..RoundHistoryEntry::default()
    }];
    let merged = rounds_with_story_edge_actions(
        rounds,
        vec![
            StoredStoryEdgeAction {
                round: 1,
                action: PlayerActionItem::character_free_text("  自定义检查密室暗门  "),
            },
            StoredStoryEdgeAction {
                round: 2,
                action: PlayerActionItem {
                    action_type: PlayerActionType::SelectedOption,
                    action: "绕到钟楼背面".to_string(),
                    ..PlayerActionItem::default()
                },
            },
        ],
    );

    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].round, 1);
    assert_eq!(
        summarize_actions(&merged[0].committed_actions),
        "自定义检查密室暗门"
    );
    assert_eq!(merged[1].round, 2);
    assert_eq!(
        summarize_actions(&merged[1].committed_actions),
        "绕到钟楼背面"
    );
}

#[tokio::test]
async fn select_storyline_node_uses_selected_branch_path_and_choices() {
    let state = test_state();
    let session_id = "session-select-branch-node";
    let to_node_2 = CharacterOption {
        title: "下楼查看老人情况".to_string(),
        action: "下楼去401室观察保险丝问题".to_string(),
        motivation_and_risk: "能靠近异常现场，但可能被邻居注意".to_string(),
    };
    let to_node_3 = CharacterOption {
        title: "检查《电路基础》中的纸条".to_string(),
        action: "翻到夹纸条的那一页，重新审视姐姐留下的地址".to_string(),
        motivation_and_risk: "能获得姐姐线索，但会暂时错过楼下异响".to_string(),
    };
    let to_node_4 = CharacterOption {
        title: "从窗户观察楼下情况".to_string(),
        action: "从窗帘缝隙观察401室窗口和楼下动静".to_string(),
        motivation_and_risk: "能保持距离，但看到的信息有限".to_string(),
    };
    let node_3_choice = CharacterOption {
        title: "仔细翻查书本，看是否还有其他标记".to_string(),
        action: "一页一页翻查《电路基础》中的旧标记".to_string(),
        motivation_and_risk: "可能发现更多姐姐留下的暗号".to_string(),
    };
    let node_4_to_5 = CharacterOption {
        title: "立刻下楼直接敲门询问老人".to_string(),
        action: "下到401室敲门询问老人保险丝和铜线".to_string(),
        motivation_and_risk: "能立刻接触线索，但会暴露关注".to_string(),
    };

    let mut payload = sample_payload_with_rounds(2);
    payload.session_id = session_id.to_string();
    payload.character_name = "陆沉舟".to_string();
    payload.turn_state.phase = TurnPhase::AwaitingPlayer;
    payload.turn_state.turn_index = 2;
    payload.turn_state.active_turn_id = 2;
    payload.world_snapshot = WorldSnapshot {
        round: 2,
        scene_title: "焦糊味的源头".to_string(),
        description: "楼道里的焦糊味正从401室门缝里渗出。".to_string(),
        ..WorldSnapshot::default()
    };
    payload.character_decision.committed_actions =
        vec![PlayerActionItem::character_selected_option(&to_node_2)];
    payload.character_decision.choices = vec![PendingCharacterChoice {
        id: "choice-node-2".to_string(),
        option: CharacterOption {
            title: "借故进屋查看".to_string(),
            action: "装作热心想帮忙，借故进屋检查保险丝".to_string(),
            motivation_and_risk: "可以接近电路箱，但容易引发怀疑".to_string(),
        },
    }];
    payload.history_log.rounds = vec![
        RoundHistoryEntry {
            round: 1,
            world_snapshot: Some(WorldSnapshot {
                round: 1,
                scene_title: "成年日的第一个早晨".to_string(),
                description: "身份芯片在手腕下微微发热。".to_string(),
                ..WorldSnapshot::default()
            }),
            narration_text: Some("成年日的晨光照进房间。".to_string()),
            choices: vec![
                PendingCharacterChoice {
                    id: "choice-1".to_string(),
                    option: to_node_2.clone(),
                },
                PendingCharacterChoice {
                    id: "choice-2".to_string(),
                    option: to_node_3.clone(),
                },
                PendingCharacterChoice {
                    id: "choice-3".to_string(),
                    option: to_node_4.clone(),
                },
            ],
            committed_actions: vec![PlayerActionItem::character_selected_option(&to_node_2)],
        },
        RoundHistoryEntry {
            round: 2,
            world_snapshot: Some(payload.world_snapshot.clone()),
            narration_text: Some("楼道里的焦糊味越来越明显。".to_string()),
            choices: payload.character_decision.choices.clone(),
            committed_actions: vec![],
        },
    ];
    payload.database_archive =
        database_archive_from_history(&payload).expect("branch test payload should build snapshot");
    state
        .persist_payload_database_state(&payload)
        .await
        .expect("branch test payload should persist");

    let branch_3 = state
        .session_archive_repo
        .prepare_backtrack_branch(
            session_id,
            1,
            &[PlayerActionItem::character_selected_option(&to_node_3)],
        )
        .await
        .expect("node 3 branch should prepare");
    save_generated_node(
        &state,
        session_id,
        &branch_3.branch_node_id,
        2,
        "书页间的旧痕",
        "纸条上的旧地址再次浮现。",
        vec![node_3_choice.clone()],
    )
    .await;

    let branch_3_child = state
        .session_archive_repo
        .prepare_backtrack_branch(
            session_id,
            2,
            &[PlayerActionItem::character_selected_option(&node_3_choice)],
        )
        .await
        .expect("node 3 child branch should prepare");
    save_generated_node(
        &state,
        session_id,
        &branch_3_child.branch_node_id,
        3,
        "书脊里的针孔",
        "书脊背面的针孔排列成新的地址。",
        vec![CharacterOption {
            title: "沿着新地址继续查下去".to_string(),
            action: "把针孔地址记下，准备前往新地点".to_string(),
            motivation_and_risk: "线索更明确，但会离家更远".to_string(),
        }],
    )
    .await;

    let branch_4 = state
        .session_archive_repo
        .prepare_backtrack_branch(
            session_id,
            1,
            &[PlayerActionItem::character_selected_option(&to_node_4)],
        )
        .await
        .expect("node 4 branch should prepare");
    save_generated_node(
        &state,
        session_id,
        &branch_4.branch_node_id,
        2,
        "窥视与信号",
        "窗外的异常蓝光在雨幕里闪了一下。",
        vec![node_4_to_5.clone()],
    )
    .await;

    let branch_5 = state
        .session_archive_repo
        .prepare_backtrack_branch(
            session_id,
            2,
            &[PlayerActionItem::character_selected_option(&node_4_to_5)],
        )
        .await
        .expect("node 5 branch should prepare");
    save_generated_node(
        &state,
        session_id,
        &branch_5.branch_node_id,
        3,
        "冰冷门扉",
        "401室的门后传来电流嗡鸣。",
        vec![CharacterOption {
            title: "以保险丝为借口，假装关心".to_string(),
            action: "以保险丝为借口继续试探老人".to_string(),
            motivation_and_risk: "能缓和气氛，但可能问不出真相".to_string(),
        }],
    )
    .await;

    let selected = state
        .select_game_session_storyline_node(
            session_id,
            SelectStorylineNodeRequest {
                node_id: branch_3.branch_node_id.clone(),
            },
        )
        .await
        .expect("node 3 should be selectable through AppState");
    assert_eq!(selected.world_state.scene_title, "书页间的旧痕");
    assert_eq!(selected.phase, TurnPhase::AwaitingPlayer);
    assert_eq!(
        selected.choices[0].option.title,
        "仔细翻查书本，看是否还有其他标记"
    );

    let materialized_parent = state
        .get_game_session_story_node(session_id, &branch_3.branch_node_id)
        .await
        .expect("selected parent node should materialize with exploration state");
    let parent_data = materialized_parent
        .data
        .expect("completed parent node should include data");
    assert_eq!(
        parent_data
            .choice_explorations
            .get(&node_3_choice.action)
            .map(|exploration| exploration.visited),
        Some(true)
    );

    let page = state
        .get_game_session_rounds(session_id, None, 20)
        .await
        .expect("selected branch rounds should load");
    let titles = page
        .rounds
        .iter()
        .filter_map(|round| {
            round
                .world_state
                .as_ref()
                .map(|world_state| world_state.scene_title.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(titles, vec!["成年日的第一个早晨", "书页间的旧痕"]);
    assert!(!titles.contains(&"冰冷门扉"));
    assert_eq!(
        page.rounds
            .last()
            .and_then(|round| round.choices.first())
            .map(|choice| choice.option.title.as_str()),
        Some("仔细翻查书本，看是否还有其他标记")
    );
}

async fn save_generated_node(
    state: &AppState,
    session_id: &str,
    node_id: &str,
    round: u64,
    title: &str,
    narration: &str,
    choices: Vec<CharacterOption>,
) {
    state
        .session_archive_repo
        .save_flow_turn_update_for_node(
            &FlowTurnUpdate {
                session_id: session_id.to_string(),
                round,
                stage: TurnPhase::Simulation,
                entity_name: "FateWeaver".to_string(),
                output_type: AgentOutputType::Json,
                content: serde_json::to_string(&WorldSnapshot {
                    round,
                    scene_title: title.to_string(),
                    description: narration.to_string(),
                    ..WorldSnapshot::default()
                })
                .expect("world snapshot should serialize"),
            },
            node_id,
        )
        .await
        .expect("world output should save");
    state
        .session_archive_repo
        .save_flow_turn_update_for_node(
            &FlowTurnUpdate {
                session_id: session_id.to_string(),
                round,
                stage: TurnPhase::Application,
                entity_name: "UpperNarrator".to_string(),
                output_type: AgentOutputType::Text,
                content: narration.to_string(),
            },
            node_id,
        )
        .await
        .expect("narration output should save");
    state
        .session_archive_repo
        .save_flow_turn_update_for_node(
            &FlowTurnUpdate {
                session_id: session_id.to_string(),
                round,
                stage: TurnPhase::Application,
                entity_name: "陆沉舟".to_string(),
                output_type: AgentOutputType::Json,
                content: serde_json::to_string(&CharacterOptions { options: choices })
                    .expect("character options should serialize"),
            },
            node_id,
        )
        .await
        .expect("choice output should save");
    state
        .session_archive_repo
        .update_session_turn_state_for_node(session_id, node_id, TurnPhase::AwaitingPlayer)
        .await
        .expect("generated node should become awaiting player");
}

#[test]
fn character_options_mark_persisted_node_as_awaiting_player() {
    let character_update = FlowTurnUpdate {
        session_id: "session-phase".to_string(),
        round: 3,
        stage: TurnPhase::Application,
        entity_name: "洛寒".to_string(),
        output_type: AgentOutputType::Json,
        content: "{}".to_string(),
    };
    let narrator_update = FlowTurnUpdate {
        session_id: "session-phase".to_string(),
        round: 3,
        stage: TurnPhase::Application,
        entity_name: "UpperNarrator".to_string(),
        output_type: AgentOutputType::Text,
        content: "narration".to_string(),
    };

    assert_eq!(
        persisted_phase_for_flow_turn_update(&character_update),
        TurnPhase::AwaitingPlayer
    );
    assert_eq!(
        persisted_phase_for_flow_turn_update(&narrator_update),
        TurnPhase::Application
    );
}

#[tokio::test]
async fn idle_reaper_cools_hot_session_and_world_request_restores_it() {
    let state = test_state_with_config(SessionLifecycleConfig {
        idle_ttl: Duration::ZERO,
        ended_ttl: Duration::ZERO,
        max_hot_sessions: 200,
        reaper_interval: Duration::from_secs(60),
        live_event_history_capacity: DEFAULT_EVENT_HISTORY_CAPACITY,
    });
    let compressed =
        archive::compress_archive_payload(&sample_payload()).expect("archive compresses");
    state
        .load_game_session_from_archive(compressed)
        .await
        .expect("archive should restore");

    {
        let sessions = state.sessions.lock().await;
        let record = sessions
            .get("session-from-slot")
            .expect("session should remain registered");
        assert!(matches!(record.slot, SessionSlot::Cold { .. }));
    }

    let restored = state
        .get_game_session_world("session-from-slot")
        .await
        .expect("cold session should be readable from database");
    assert_eq!(restored.world_state.scene_title, "钟楼阴影");

    let sessions = state.sessions.lock().await;
    let record = sessions
        .get("session-from-slot")
        .expect("session should remain registered");
    assert!(matches!(record.slot, SessionSlot::Cold { .. }));
}

#[tokio::test]
async fn ensure_hot_session_lazily_registers_database_session_after_restart() {
    let db_path = std::env::temp_dir().join(format!(
        "akasa-state-restart-{}.sqlite3",
        Uuid::new_v4().simple()
    ));
    let state = test_state_at_path(db_path.clone());
    let compressed =
        archive::compress_archive_payload(&sample_payload()).expect("archive compresses");
    state
        .load_game_session_from_archive(compressed)
        .await
        .expect("archive should restore into the original process");

    let restarted = test_state_at_path(db_path);
    assert!(restarted.sessions.lock().await.is_empty());

    let access = restarted
        .ensure_hot_session("session-from-slot", false)
        .await
        .expect("database session should lazy-register as cold and restore hot");
    assert_eq!(access.session_id, "session-from-slot");

    let sessions = restarted.sessions.lock().await;
    let record = sessions
        .get("session-from-slot")
        .expect("session should be registered after lazy restore");
    assert!(matches!(record.slot, SessionSlot::Hot(_)));
}

#[tokio::test]
async fn export_save_archive_returns_local_archive_payload() {
    let state = test_state();

    let created = state
        .create_game_session(crate::api::game_sessions::CreateGameSessionRequest {
            character_name: "归档角色".to_string(),
            world_profile: "archive world".to_string(),
            character_profile: "archive character".to_string(),
            key_story_beats: "archive beats".to_string(),
        })
        .await
        .expect("session should create");

    assert!(!created.session_id.starts_with("session-"));
    Uuid::parse_str(&created.session_id).expect("created session id should be a UUID");

    let exported = state
        .export_save_archive(&created.session_id, Some("测试存档"))
        .await
        .expect("save archive should export");

    let payload = archive::decompress_archive_payload(&exported.compressed_archive)
        .expect("exported archive should decode");

    assert_eq!(payload.session_id, created.session_id);
    assert_eq!(payload.title, "测试存档");
    assert!(!payload.database_archive.story_nodes.is_empty());
    assert_eq!(exported.title, "测试存档");
    assert!(!exported.compressed_archive.is_empty());
}

#[tokio::test]
async fn export_save_archive_preserves_full_storyline_database_snapshot() {
    let db_path = std::env::temp_dir().join(format!(
        "akasa-full-save-archive-{}.sqlite3",
        Uuid::new_v4().simple()
    ));
    let state = AppState::with_lifecycle_config(
        db_path,
        SessionLifecycleConfig {
            idle_ttl: Duration::from_secs(30 * 60),
            ended_ttl: Duration::from_secs(5 * 60),
            max_hot_sessions: 200,
            reaper_interval: Duration::from_secs(60),
            live_event_history_capacity: DEFAULT_EVENT_HISTORY_CAPACITY,
        },
        false,
    );

    let compressed = archive::compress_archive_payload(&sample_payload_with_rounds(2))
        .expect("archive compresses");
    state
        .load_game_session_from_archive(compressed)
        .await
        .expect("linear archive should restore");

    let branch_choice = CharacterOption {
        title: "翻窗".to_string(),
        action: "翻窗进入侧厅".to_string(),
        motivation_and_risk: "绕开正门，但玻璃声会暴露位置".to_string(),
    };
    let branch = state
        .session_archive_repo
        .prepare_backtrack_branch(
            "session-from-slot",
            1,
            &[PlayerActionItem::character_selected_option(&branch_choice)],
        )
        .await
        .expect("branch should prepare");
    state
        .session_archive_repo
        .save_flow_turn_update_for_node(
            &FlowTurnUpdate {
                session_id: "session-from-slot".to_string(),
                round: 2,
                stage: TurnPhase::Simulation,
                entity_name: "FateWeaver".to_string(),
                output_type: AgentOutputType::Json,
                content: serde_json::to_string(&WorldSnapshot {
                    round: 2,
                    scene_title: "侧厅窗影".to_string(),
                    ..WorldSnapshot::default()
                })
                .expect("world snapshot should serialize"),
            },
            &branch.branch_node_id,
        )
        .await
        .expect("branch world snapshot should save");
    state
        .session_archive_repo
        .save_flow_turn_update_for_node(
            &FlowTurnUpdate {
                session_id: "session-from-slot".to_string(),
                round: 2,
                stage: TurnPhase::Application,
                entity_name: "UpperNarrator".to_string(),
                output_type: AgentOutputType::Text,
                content: "你翻进侧厅，旧窗框在身后轻轻回弹。".to_string(),
            },
            &branch.branch_node_id,
        )
        .await
        .expect("branch narration should save");
    state
        .session_archive_repo
        .update_session_turn_state_for_node(
            "session-from-slot",
            &branch.branch_node_id,
            TurnPhase::AwaitingPlayer,
        )
        .await
        .expect("branch node should be stable");

    let original_choice = CharacterOption {
        title: "选择-1".to_string(),
        action: "行动-1".to_string(),
        motivation_and_risk: "保持测试稳定".to_string(),
    };
    state
        .session_archive_repo
        .prepare_backtrack_branch(
            "session-from-slot",
            1,
            &[PlayerActionItem::character_selected_option(
                &original_choice,
            )],
        )
        .await
        .expect("linear branch should reactivate");

    let exported = state
        .export_save_archive("session-from-slot", Some("完整存档"))
        .await
        .expect("save archive should export");
    let payload = archive::decompress_archive_payload(&exported.compressed_archive)
        .expect("exported archive should decode");
    assert!(
        payload
            .database_archive
            .story_nodes
            .iter()
            .any(|node| node.node_id == branch.branch_node_id)
    );

    state
        .load_game_session_from_archive(exported.compressed_archive)
        .await
        .expect("full archive should restore");

    let storyline = state
        .get_game_session_storyline("session-from-slot")
        .await
        .expect("storyline should load after archive restore");
    assert!(
        storyline
            .nodes
            .iter()
            .any(|node| { node.node_id == branch.branch_node_id && node.title == "侧厅窗影" })
    );
    assert!(storyline.edges.iter().any(|edge| {
        edge.to_node_id == branch.branch_node_id
            && edge
                .actions
                .iter()
                .any(|action| action.action == "翻窗进入侧厅")
    }));
}

#[tokio::test]
async fn load_game_session_from_archive_overwrites_existing_session() {
    let state = test_state();

    state
        .create_game_session(crate::api::game_sessions::CreateGameSessionRequest {
            character_name: "旧角色".to_string(),
            world_profile: "old world".to_string(),
            character_profile: "old character".to_string(),
            key_story_beats: "old beats".to_string(),
        })
        .await
        .expect("session should create");

    let mut payload = sample_payload();
    payload.session_id = "session-b".to_string();
    let compressed = archive::compress_archive_payload(&payload).expect("archive compresses");
    let restored = state
        .load_game_session_from_archive(compressed)
        .await
        .expect("archive should restore");

    assert_eq!(restored.session_id, "session-b");
    assert_eq!(restored.world_state.scene_title, "钟楼阴影");

    let loaded_again = state
        .get_game_session_world("session-b")
        .await
        .expect("restored session should be queryable");
    assert_eq!(loaded_again.current_outcome, "尚未做出选择");
}

#[tokio::test]
async fn load_game_session_from_archive_clears_stale_future_rows() {
    let db_path = std::env::temp_dir().join(format!(
        "akasa-archive-overwrite-{}.sqlite3",
        Uuid::new_v4().simple()
    ));
    let state = AppState::with_lifecycle_config(
        db_path.clone(),
        SessionLifecycleConfig {
            idle_ttl: Duration::from_secs(30 * 60),
            ended_ttl: Duration::from_secs(5 * 60),
            max_hot_sessions: 200,
            reaper_interval: Duration::from_secs(60),
            live_event_history_capacity: DEFAULT_EVENT_HISTORY_CAPACITY,
        },
        false,
    );

    let first_archive = archive::compress_archive_payload(&sample_payload_with_rounds(5))
        .expect("larger archive should compress");
    state
        .load_game_session_from_archive(first_archive)
        .await
        .expect("larger archive should restore");

    let mut smaller_payload = sample_payload_with_rounds(3);
    smaller_payload.history_log.rounds[2]
        .committed_actions
        .clear();
    smaller_payload.character_decision.committed_actions =
        vec![PlayerActionItem::character_free_text("行动-2")];
    let second_archive = archive::compress_archive_payload(&smaller_payload)
        .expect("smaller archive should compress");
    state
        .load_game_session_from_archive(second_archive)
        .await
        .expect("smaller archive should replace existing session");

    let conn = Connection::open(db_path).expect("sqlite db should open");
    let max_node_depth: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(node_depth), 0) FROM story_nodes WHERE session_id = ?1",
            params!["session-from-slot"],
            |row| row.get(0),
        )
        .expect("story nodes should be queryable");
    let future_output_count: i64 = conn
        .query_row(
            r#"
            SELECT COUNT(*)
            FROM entity_flow_outputs
            WHERE session_id = ?1
                AND node_id IN ('node-4', 'node-5')
            "#,
            params!["session-from-slot"],
            |row| row.get(0),
        )
        .expect("entity flow outputs should be queryable");
    let future_node_count: i64 = conn
        .query_row(
            r#"
            SELECT COUNT(*)
            FROM story_nodes
            WHERE session_id = ?1
                AND node_depth > 3
            "#,
            params!["session-from-slot"],
            |row| row.get(0),
        )
        .expect("story nodes should be queryable");
    let total_node_count: i64 = conn
        .query_row(
            "SELECT total_node_count FROM sessions WHERE session_id = ?1",
            params!["session-from-slot"],
            |row| row.get(0),
        )
        .expect("session metadata should be queryable");

    assert_eq!(max_node_depth, 3);
    assert_eq!(future_output_count, 0);
    assert_eq!(future_node_count, 0);
    assert_eq!(total_node_count, 3);
}

#[tokio::test]
async fn clone_game_session_creates_independent_runtime_session() {
    let state = test_state();
    let compressed =
        archive::compress_archive_payload(&sample_payload()).expect("archive compresses");
    state
        .load_game_session_from_archive(compressed)
        .await
        .expect("source session should restore");

    let cloned = state
        .clone_game_session("session-from-slot", None)
        .await
        .expect("stable source session should clone");

    assert_ne!(cloned.session_id, "session-from-slot");
    assert!(!cloned.session_id.starts_with("session-"));
    Uuid::parse_str(&cloned.session_id).expect("cloned session id should be a UUID");
    assert_eq!(cloned.world_state.scene_title, "钟楼阴影");
    assert_eq!(cloned.current_outcome, "尚未做出选择");

    let source = state
        .get_game_session_world("session-from-slot")
        .await
        .expect("source session should remain queryable");
    let cloned_again = state
        .get_game_session_world(&cloned.session_id)
        .await
        .expect("cloned session should be queryable");

    assert_eq!(
        source.world_state.scene_title,
        cloned_again.world_state.scene_title
    );
}

#[test]
fn game_session_world_state_serializes_world_state_as_camel_case() {
    let dto = GameSessionWorldStateData {
        session_id: "session-test".to_string(),
        active_node_id: "node-2".to_string(),
        generated_profiles: GeneratedProfilesData {
            world: "world".to_string(),
            character: "character".to_string(),
            key_story_beats: "beats".to_string(),
        },
        status: "awaiting_player".to_string(),
        phase: TurnPhase::AwaitingPlayer,
        flow_end: false,
        turn_index: 2,
        active_turn_id: 2,
        world_state: WorldStateData::from(WorldSnapshot {
            round: 2,
            scene_title: "螺旋楼梯的暗影".to_string(),
            time_absolute: "第一日 深夜十一点四十二分".to_string(),
            location_name: "齿轮教堂地下二层".to_string(),
            new_info: vec!["图纸碎片已安全到手".to_string()],
            is_ending: true,
            ending_type: Some("牺牲".to_string()),
            ..WorldSnapshot::default()
        }),
        latest_narration: "narration".to_string(),
        current_outcome: "action".to_string(),
        choices: vec![],
        choice_explorations: ChoiceExplorationsData::new(),
        branch_explorations: vec![],
    };

    let value = serde_json::to_value(dto).expect("dto should serialize");
    let world_state = value
        .get("worldState")
        .and_then(Value::as_object)
        .expect("worldState should be serialized as object");

    assert_eq!(
        world_state.get("sceneTitle").and_then(Value::as_str),
        Some("螺旋楼梯的暗影")
    );
    assert_eq!(
        world_state.get("timeAbsolute").and_then(Value::as_str),
        Some("第一日 深夜十一点四十二分")
    );
    assert_eq!(
        world_state.get("isEnding").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        world_state.get("endingType").and_then(Value::as_str),
        Some("牺牲")
    );
    assert_eq!(
        world_state.get("locationName").and_then(Value::as_str),
        Some("齿轮教堂地下二层")
    );
    assert_eq!(
        world_state
            .get("newInfo")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(Value::as_str),
        Some("图纸碎片已安全到手")
    );
    assert!(world_state.get("scene_title").is_none());
    assert!(world_state.get("new_info").is_none());
    assert!(value.get("history").is_none());
}

#[test]
fn story_node_materialization_requires_stable_complete_outputs() {
    let complete = StoredStoryNodeRound {
        node_id: "node-1".to_string(),
        round: 1,
        phase: TurnPhase::AwaitingPlayer,
        flow_end: false,
        entry: RoundHistoryEntry {
            round: 1,
            world_snapshot: Some(WorldSnapshot::default()),
            narration_text: Some("雾中钟声渐近。".to_string()),
            choices: vec![PendingCharacterChoice {
                id: "choice-1".to_string(),
                option: CharacterOption {
                    title: "靠近钟楼".to_string(),
                    action: "靠近钟楼".to_string(),
                    motivation_and_risk: "线索更近，风险也更近".to_string(),
                },
            }],
            ..RoundHistoryEntry::default()
        },
    };
    let partial = StoredStoryNodeRound {
        phase: TurnPhase::Application,
        entry: RoundHistoryEntry {
            choices: Vec::new(),
            ..complete.entry.clone()
        },
        ..complete.clone()
    };

    assert_eq!(story_node_materialization_status(&complete), "complete");
    assert_eq!(story_node_materialization_status(&partial), "running");
}

fn sample_payload() -> SessionArchivePayload {
    let mut payload = SessionArchivePayload {
        session_id: "session-from-slot".to_string(),
        title: "第7轮：钟楼阴影".to_string(),
        character_name: "洛寒".to_string(),
        world_profile: "world".to_string(),
        character_profile: "character".to_string(),
        key_story_beats: "beats".to_string(),
        turn_state: TurnStateArchive {
            phase: TurnPhase::AwaitingPlayer,
            turn_index: 7,
            active_turn_id: 7,
        },
        fate_weaver: Context::default(),
        upper_narrator: Context::default(),
        character_agent: Context::default(),
        world_snapshot: WorldSnapshot {
            round: 7,
            scene_title: "钟楼阴影".to_string(),
            description: "雾气正在台阶间倒灌。".to_string(),
            ..WorldSnapshot::default()
        },
        character_decision: CharacterDecisionArchive {
            committed_actions: vec![PlayerActionItem::character_free_text("绕到钟楼背面")],
            choices: vec![PendingCharacterChoice {
                id: "choice-1".to_string(),
                option: CharacterOption {
                    title: "绕行".to_string(),
                    action: "绕到钟楼背面".to_string(),
                    motivation_and_risk: "视野更好，但会暴露脚步声".to_string(),
                },
            }],
        },
        history_log: SessionHistoryLog {
            rounds: vec![RoundHistoryEntry {
                round: 7,
                world_snapshot: Some(WorldSnapshot {
                    round: 7,
                    scene_title: "钟楼阴影".to_string(),
                    description: "雾气正在台阶间倒灌。".to_string(),
                    ..WorldSnapshot::default()
                }),
                narration_text: Some("钟声掠过雾墙。".to_string()),
                choices: vec![PendingCharacterChoice {
                    id: "choice-1".to_string(),
                    option: CharacterOption {
                        title: "绕行".to_string(),
                        action: "绕到钟楼背面".to_string(),
                        motivation_and_risk: "视野更好，但会暴露脚步声".to_string(),
                    },
                }],
                committed_actions: vec![PlayerActionItem::character_free_text("绕到钟楼背面")],
            }],
        },
        database_archive: archive::SessionDatabaseArchive {
            active_node_id: "start".to_string(),
            total_node_count: 0,
            story_nodes: Vec::new(),
            story_edges: Vec::new(),
            story_edge_actions: Vec::new(),
            entity_flow_outputs: Vec::new(),
            entity_context_items: Vec::new(),
        },
    };
    payload.database_archive =
        database_archive_from_history(&payload).expect("sample payload should build snapshot");
    payload
}

fn sample_payload_with_rounds(total_rounds: u64) -> SessionArchivePayload {
    let mut payload = sample_payload();
    payload.turn_state.turn_index = total_rounds;
    payload.turn_state.active_turn_id = total_rounds;
    payload.world_snapshot = WorldSnapshot {
        round: total_rounds,
        scene_title: format!("第{total_rounds}轮"),
        description: format!("第{total_rounds}轮描述"),
        ..WorldSnapshot::default()
    };
    payload.character_decision.committed_actions = vec![PlayerActionItem::character_free_text(
        format!("行动-{total_rounds}"),
    )];
    payload.history_log = SessionHistoryLog {
        rounds: (1..=total_rounds)
            .map(|round| {
                let option = CharacterOption {
                    title: format!("选择-{round}"),
                    action: format!("行动-{round}"),
                    motivation_and_risk: "保持测试稳定".to_string(),
                };
                RoundHistoryEntry {
                    round,
                    world_snapshot: Some(WorldSnapshot {
                        round,
                        scene_title: format!("第{round}轮"),
                        description: format!("第{round}轮描述"),
                        ..WorldSnapshot::default()
                    }),
                    narration_text: Some(format!("叙事-{round}")),
                    choices: vec![PendingCharacterChoice {
                        id: format!("choice-{round}"),
                        option: option.clone(),
                    }],
                    committed_actions: vec![PlayerActionItem::character_selected_option(&option)],
                }
            })
            .collect(),
    };
    payload.database_archive =
        database_archive_from_history(&payload).expect("sample payload should build snapshot");
    payload
}

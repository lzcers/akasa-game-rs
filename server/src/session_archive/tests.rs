use super::*;
use crate::session_history::RoundHistoryEntry;
use crate::{analytics::AnalyticsRepository, api::site::AnalyticsEventInput};
use agent::core::Message;
use serde_json::json;
use story_engine::{
    components::{
        agent::AgentOutputType,
        outcome::{CharacterOption, PendingCharacterChoice, PlayerActionType},
        world_snapshot::WorldSnapshot,
    },
    resources::session_events::{
        EntityContextItemAppended, EntityContextRollback, EntityContextRollbackPolicy,
        FlowTurnError, FlowTurnUpdate, PlayerInput, SessionCreated,
    },
};
use uuid::Uuid;

#[tokio::test]
async fn shared_database_stores_analytics_and_session_metadata() {
    let db_path = std::env::temp_dir().join(format!(
        "akasa-shared-db-{}.sqlite3",
        Uuid::new_v4().simple()
    ));
    let db = AppDatabase::new(db_path.clone());
    let analytics = AnalyticsRepository::new(db.clone());
    let sessions = SessionArchiveRepository::new(db);

    analytics
        .append_events(
            &[AnalyticsEventInput {
                id: "evt-shared".to_string(),
                event_name: "session_created".to_string(),
                anonymous_user_id: "anon-shared".to_string(),
                client_session_id: "visit-shared".to_string(),
                game_session_id: Some("session-shared".to_string()),
                source_session_id: None,
                occurred_at: "2026-06-10T00:00:00Z".to_string(),
                app: "game-web".to_string(),
                app_version: None,
                path: Some("/play".to_string()),
                referrer_domain: None,
                utm_source: None,
                utm_medium: None,
                utm_campaign: None,
                device_type: Some("desktop".to_string()),
                platform: Some("MacIntel".to_string()),
                properties: json!({}),
            }],
            None,
        )
        .await
        .expect("analytics event should save");
    sessions
        .save_session_created(&SessionCreated {
            session_id: "session-shared".to_string(),
            character_name: "hero".to_string(),
            world_profile: "world".to_string(),
            character_profile: "hero".to_string(),
            key_story_beats: "beats".to_string(),
        })
        .await
        .expect("session metadata should save");

    let summary = analytics
        .summary(24 * 30)
        .await
        .expect("summary should read");
    let metadata = sessions
        .load_session_metadata("session-shared")
        .await
        .expect("session metadata should load")
        .expect("session metadata should exist");
    let conn = Connection::open(db_path).expect("sqlite db should open");
    let removed_table_count: i64 = conn
        .query_row(
            r#"
                SELECT COUNT(*)
                FROM sqlite_master
                WHERE type = 'table'
                    AND name IN (
                        'session_rounds',
                        'game_session_archives',
                        'flow_turns',
                        'player_inputs',
                        'flow_outputs',
                        'agent_contexts',
                        'agent_context_items'
                    )
                "#,
            [],
            |row| row.get(0),
        )
        .expect("schema should be queryable");
    let story_graph_table_count: i64 = conn
        .query_row(
            r#"
                SELECT COUNT(*)
                FROM sqlite_master
                WHERE type = 'table'
                    AND name IN (
                        'sessions',
                        'session_worlds',
                        'session_characters',
                        'story_nodes',
                        'story_edges',
                        'story_edge_actions',
                        'entity_flow_outputs',
                        'entity_context_items'
                    )
                "#,
            [],
            |row| row.get(0),
        )
        .expect("story graph schema should be queryable");
    let story_edge_columns = conn
        .prepare("PRAGMA table_info(story_edges)")
        .expect("story_edges columns should be inspectable")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("story_edges columns should query")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("story_edges columns should read");
    let session_columns = conn
        .prepare("PRAGMA table_info(sessions)")
        .expect("sessions columns should be inspectable")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("sessions columns should query")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("sessions columns should read");
    let session_world_columns = conn
        .prepare("PRAGMA table_info(session_worlds)")
        .expect("session_worlds columns should be inspectable")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("session_worlds columns should query")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("session_worlds columns should read");
    let session_character_columns = conn
        .prepare("PRAGMA table_info(session_characters)")
        .expect("session_characters columns should be inspectable")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("session_characters columns should query")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("session_characters columns should read");
    let story_edge_action_columns = conn
        .prepare("PRAGMA table_info(story_edge_actions)")
        .expect("story_edge_actions columns should be inspectable")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("story_edge_actions columns should query")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("story_edge_actions columns should read");
    let entity_context_item_columns = conn
        .prepare("PRAGMA table_info(entity_context_items)")
        .expect("entity_context_items columns should be inspectable")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("entity_context_items columns should query")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("entity_context_items columns should read");

    assert_eq!(summary.totals.events, 1);
    assert_eq!(metadata.world_profile, "world");
    assert_eq!(removed_table_count, 0);
    assert_eq!(story_graph_table_count, 8);
    assert!(!session_columns.contains(&"world_profile".to_string()));
    assert!(!session_columns.contains(&"character_profile".to_string()));
    assert!(!session_columns.contains(&"key_story_beats".to_string()));
    assert!(session_world_columns.contains(&"world_profile".to_string()));
    assert!(session_world_columns.contains(&"global_key_story_beats".to_string()));
    assert!(session_character_columns.contains(&"character_name".to_string()));
    assert!(session_character_columns.contains(&"character_profile".to_string()));
    assert!(session_character_columns.contains(&"key_story_beats".to_string()));
    assert!(session_character_columns.contains(&"player_id".to_string()));
    assert!(session_character_columns.contains(&"is_playable".to_string()));
    assert!(!story_edge_columns.contains(&"action".to_string()));
    assert!(!story_edge_columns.contains(&"action_type".to_string()));
    assert!(story_edge_action_columns.contains(&"character_name".to_string()));
    assert!(story_edge_action_columns.contains(&"player_id".to_string()));
    assert!(entity_context_item_columns.contains(&"entity_name".to_string()));
    assert!(entity_context_item_columns.contains(&"message_role".to_string()));
    assert!(entity_context_item_columns.contains(&"content".to_string()));
    assert!(!entity_context_item_columns.contains(&"agent_name".to_string()));
    assert!(!entity_context_item_columns.contains(&"message_json".to_string()));
    assert!(story_edge_action_columns.contains(&"action_type".to_string()));
    assert!(story_edge_action_columns.contains(&"title".to_string()));
    assert!(story_edge_action_columns.contains(&"action".to_string()));
    assert!(story_edge_action_columns.contains(&"motivation_and_risk".to_string()));
}

#[tokio::test]
async fn entity_flow_outputs_upsert_and_page_with_before_cursor() {
    let repo = test_repo();
    let rounds = (1..=5)
        .map(|round| round_entry(round, &format!("round-{round}")))
        .collect::<Vec<_>>();

    repo.save_rounds("session-rounds", &rounds)
        .await
        .expect("rounds should save");
    repo.save_rounds("session-rounds", &[round_entry(3, "round-3-updated")])
        .await
        .expect("round should upsert");

    let latest = repo
        .load_round_page("session-rounds", None, 2)
        .await
        .expect("latest page should load");
    assert_eq!(
        latest
            .rounds
            .iter()
            .map(|entry| entry.round)
            .collect::<Vec<_>>(),
        vec![4, 5]
    );
    assert_eq!(latest.next_before_round, Some(4));
    assert!(latest.has_more);

    let older = repo
        .load_round_page("session-rounds", latest.next_before_round, 2)
        .await
        .expect("older page should load");
    assert_eq!(
        older
            .rounds
            .iter()
            .map(|entry| entry.round)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(
        older.rounds[1].narration_text.as_deref(),
        Some("round-3-updated")
    );
    assert_eq!(older.next_before_round, Some(2));
    assert!(older.has_more);
}

#[tokio::test]
async fn flow_turn_updates_store_entity_outputs() {
    let repo = test_repo();
    let snapshot = WorldSnapshot {
        round: 7,
        scene_title: "钟楼阴影".to_string(),
        ..WorldSnapshot::default()
    };

    repo.save_flow_turn_update(&FlowTurnUpdate {
        session_id: "session-flow-rows".to_string(),
        round: 7,
        stage: TurnPhase::Simulation,
        entity_name: "FateWeaver".to_string(),
        output_type: AgentOutputType::Json,
        content: serde_json::to_string(&snapshot).expect("snapshot should serialize"),
    })
    .await
    .expect("world output should save");
    repo.save_flow_turn_update(&FlowTurnUpdate {
        session_id: "session-flow-rows".to_string(),
        round: 7,
        stage: TurnPhase::Application,
        entity_name: "UpperNarrator".to_string(),
        output_type: AgentOutputType::Text,
        content: "钟声掠过雾墙。".to_string(),
    })
    .await
    .expect("narration output should save");

    let rounds = repo
        .load_rounds("session-flow-rows")
        .await
        .expect("entity flow outputs should load");

    assert_eq!(rounds.len(), 1);
    assert_eq!(rounds[0].round, 7);
    assert_eq!(
        rounds[0]
            .world_snapshot
            .as_ref()
            .map(|snapshot| snapshot.scene_title.as_str()),
        Some("钟楼阴影")
    );
    assert_eq!(rounds[0].narration_text.as_deref(), Some("钟声掠过雾墙。"));
}

#[tokio::test]
async fn story_edge_actions_store_choice_option_fields() {
    let db_path = std::env::temp_dir().join(format!(
        "akasa-story-edge-choice-{}.sqlite3",
        Uuid::new_v4().simple()
    ));
    let repo = SessionArchiveRepository::new(AppDatabase::new(db_path.clone()));
    let choice = PendingCharacterChoice {
        id: "choice-1".to_string(),
        option: CharacterOption {
            title: "绕行".to_string(),
            action: "绕到钟楼背面".to_string(),
            motivation_and_risk: "视野更好，但会暴露脚步声".to_string(),
        },
    };

    repo.save_session_created(&SessionCreated {
        session_id: "session-choice-edge".to_string(),
        character_name: "hero".to_string(),
        world_profile: "world".to_string(),
        character_profile: "hero".to_string(),
        key_story_beats: "beats".to_string(),
    })
    .await
    .expect("session metadata should save");
    repo.save_rounds(
        "session-choice-edge",
        &[RoundHistoryEntry {
            round: 1,
            choices: vec![choice.clone()],
            ..RoundHistoryEntry::default()
        }],
    )
    .await
    .expect("round choices should save");
    repo.update_session_turn_state("session-choice-edge", TurnPhase::AwaitingPlayer, 1, 1)
        .await
        .expect("session active node should update");
    repo.save_player_input(&PlayerInput {
        session_id: "session-choice-edge".to_string(),
        round: 1,
        actions: vec![PlayerActionItem::character_selected_option(&choice.option)],
    })
    .await
    .expect("story edge should save");

    let conn = Connection::open(db_path).expect("sqlite db should open");
    let edge_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM story_edges WHERE session_id = ?1",
            params!["session-choice-edge"],
            |row| row.get(0),
        )
        .expect("story edge should be stored");
    let stored_action: (String, Option<String>, String, String, String, String) = conn
        .query_row(
            r#"
                SELECT character_name, player_id, action_type, title, action, motivation_and_risk
                FROM story_edge_actions
                WHERE session_id = ?1
                "#,
            params!["session-choice-edge"],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .expect("story edge action should be stored");
    let loaded_actions_before_target = repo
        .load_story_edge_actions("session-choice-edge")
        .await
        .expect("story edge actions should load");
    let empty_choice_explorations = repo
        .load_choice_explorations("session-choice-edge")
        .await
        .expect("choice explorations should load");
    let has_round_one_action_before_target = repo
        .has_story_edge_action_for_round("session-choice-edge", 1)
        .await
        .expect("round duplicate check should run");
    let has_round_two_action = repo
        .has_story_edge_action_for_round("session-choice-edge", 2)
        .await
        .expect("round duplicate check should run");

    assert_eq!(edge_count, 1);
    assert!(!has_round_one_action_before_target);
    assert!(!has_round_two_action);
    assert_eq!(stored_action.0, "hero");
    assert_eq!(stored_action.1, None);
    assert_eq!(stored_action.2, "selected_option");
    assert_eq!(stored_action.3, choice.option.title);
    assert_eq!(stored_action.4, choice.option.action);
    assert_eq!(stored_action.5, choice.option.motivation_and_risk);
    assert!(loaded_actions_before_target.is_empty());
    assert!(empty_choice_explorations.is_empty());
    repo.update_session_turn_state("session-choice-edge", TurnPhase::Application, 1, 2)
        .await
        .expect("target node should become active");
    let has_round_one_action = repo
        .has_story_edge_action_for_round("session-choice-edge", 1)
        .await
        .expect("round duplicate check should run");
    let loaded_actions = repo
        .load_story_edge_actions("session-choice-edge")
        .await
        .expect("story edge actions should load");
    repo.save_flow_turn_update(&FlowTurnUpdate {
        session_id: "session-choice-edge".to_string(),
        round: 2,
        stage: TurnPhase::Application,
        entity_name: "UpperNarrator".to_string(),
        output_type: AgentOutputType::Text,
        content: "你绕到钟楼背面，潮湿砖缝里透出微光。".to_string(),
    })
    .await
    .expect("target narration should save");
    let choice_explorations = repo
        .load_choice_explorations("session-choice-edge")
        .await
        .expect("choice explorations should load");
    assert!(has_round_one_action);
    assert_eq!(loaded_actions.len(), 1);
    assert_eq!(loaded_actions[0].action.action, "绕到钟楼背面");
    assert_eq!(choice_explorations.len(), 1);
    assert_eq!(choice_explorations[0].round, 1);
    assert_eq!(choice_explorations[0].action, "绕到钟楼背面");
}

#[tokio::test]
async fn story_edge_actions_store_empty_title_for_free_text() {
    let db_path = std::env::temp_dir().join(format!(
        "akasa-free-text-edge-{}.sqlite",
        Uuid::new_v4().simple()
    ));
    let repo = SessionArchiveRepository::new(AppDatabase::new(db_path.clone()));

    repo.save_session_created(&SessionCreated {
        session_id: "session-free-text-edge".to_string(),
        character_name: "hero".to_string(),
        world_profile: "world".to_string(),
        character_profile: "hero".to_string(),
        key_story_beats: "beats".to_string(),
    })
    .await
    .expect("session metadata should save");
    repo.update_session_turn_state("session-free-text-edge", TurnPhase::AwaitingPlayer, 1, 1)
        .await
        .expect("session active node should update");
    repo.save_player_input(&PlayerInput {
        session_id: "session-free-text-edge".to_string(),
        round: 1,
        actions: vec![PlayerActionItem::character_free_text("检查密室暗门")],
    })
    .await
    .expect("story edge should save");

    let conn = Connection::open(db_path).expect("sqlite db should open");
    let stored_action: (String, String, String, String) = conn
        .query_row(
            r#"
                SELECT character_name, action_type, title, action
                FROM story_edge_actions
                WHERE session_id = ?1
                "#,
            params!["session-free-text-edge"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("story edge action should be stored");

    assert_eq!(stored_action.0, "hero");
    assert_eq!(stored_action.1, "free_text");
    assert_eq!(stored_action.2, "");
    assert_eq!(stored_action.3, "检查密室暗门");
}

#[tokio::test]
async fn session_flow_turn_and_entity_context_tables_round_trip() {
    let repo = test_repo();
    repo.save_session_created(&SessionCreated {
        session_id: "session-db".to_string(),
        character_name: "hero".to_string(),
        world_profile: "world".to_string(),
        character_profile: "hero".to_string(),
        key_story_beats: "beats".to_string(),
    })
    .await
    .expect("session metadata should save");

    let metadata = repo
        .load_session_metadata("session-db")
        .await
        .expect("metadata should load")
        .expect("metadata should exist");
    assert_eq!(metadata.world_profile, "world");
    assert_eq!(metadata.phase, TurnPhase::Start);
    assert!(!metadata.flow_end);

    repo.save_rounds("session-db", &[round_entry(1, "round-1")])
        .await
        .expect("flow turn should save");
    repo.record_flow_turn_completed("session-db", 1)
        .await
        .expect("completion should save");
    let metadata = repo
        .load_session_metadata("session-db")
        .await
        .expect("metadata should load")
        .expect("metadata should exist");
    assert_eq!(metadata.phase, TurnPhase::TurnCompleted);
    assert_eq!(metadata.turn_index, 1);
    assert_eq!(metadata.active_turn_id, 1);
    assert!(!metadata.flow_end);

    repo.record_flow_turn_end("session-db", 1)
        .await
        .expect("flow end should save");
    let metadata = repo
        .load_session_metadata("session-db")
        .await
        .expect("metadata should load")
        .expect("metadata should exist");
    assert_eq!(metadata.phase, TurnPhase::Ended);
    assert!(metadata.flow_end);

    repo.save_entity_context_item(&EntityContextItemAppended {
        session_id: "session-db".to_string(),
        round: 1,
        entity_name: "UpperNarrator".to_string(),
        message: Message::user("latest context"),
    })
    .await
    .expect("entity context item should save");
    repo.save_entity_context_item(&EntityContextItemAppended {
        session_id: "session-db".to_string(),
        round: 1,
        entity_name: "UpperNarrator".to_string(),
        message: Message::assistant("discarded response"),
    })
    .await
    .expect("assistant entity context item should save");
    repo.save_entity_context_rollback(&EntityContextRollback {
        session_id: "session-db".to_string(),
        round: 1,
        entity_name: "UpperNarrator".to_string(),
        policy: EntityContextRollbackPolicy::LatestInput,
    })
    .await
    .expect("entity context rollback should save");
    let contexts = repo
        .load_entity_contexts_through_round("session-db", None)
        .await
        .expect("entity contexts should load");
    assert_eq!(contexts.len(), 1);
    assert_eq!(contexts[0].entity_name, "UpperNarrator");
    assert!(contexts[0].context.conversation().is_empty());

    repo.save_player_input(&PlayerInput {
        session_id: "session-db".to_string(),
        round: 1,
        actions: vec![PlayerActionItem {
            action_type: PlayerActionType::SelectedOption,
            action: "绕到钟楼背面".to_string(),
            ..PlayerActionItem::default()
        }],
    })
    .await
    .expect("story edge action should save");
    repo.update_session_turn_state("session-db", TurnPhase::AwaitingPlayer, 2, 2)
        .await
        .expect("target node should become active");
    let inputs = repo
        .load_story_edge_actions("session-db")
        .await
        .expect("story edge actions should load");
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].round, 1);
    assert_eq!(
        inputs[0].action.action_type,
        PlayerActionType::SelectedOption
    );
    assert_eq!(inputs[0].action.action, "绕到钟楼背面");

    repo.record_flow_turn_error(&FlowTurnError {
        session_id: "session-db".to_string(),
        round: 2,
        stage: TurnPhase::Simulation,
        entity_name: "FateWeaver".to_string(),
        msg: "boom".to_string(),
    })
    .await
    .expect("flow error should save");
    let metadata = repo
        .load_session_metadata("session-db")
        .await
        .expect("metadata should load")
        .expect("metadata should exist");
    assert_eq!(metadata.phase, TurnPhase::Failed);
}

#[tokio::test]
async fn backtrack_branch_preserves_old_future_and_switches_active_path() {
    let db_path = std::env::temp_dir().join(format!(
        "akasa-backtrack-branch-{}.sqlite3",
        Uuid::new_v4().simple()
    ));
    let repo = SessionArchiveRepository::new(AppDatabase::new(db_path.clone()));
    let old_choice = CharacterOption {
        title: "走正门".to_string(),
        action: "走正门进入钟楼".to_string(),
        motivation_and_risk: "更快，但更容易被看见".to_string(),
    };
    let branch_choice = CharacterOption {
        title: "绕行".to_string(),
        action: "绕到钟楼背面".to_string(),
        motivation_and_risk: "视野更好，但会暴露脚步声".to_string(),
    };

    repo.save_session_created(&SessionCreated {
        session_id: "session-backtrack".to_string(),
        character_name: "hero".to_string(),
        world_profile: "world".to_string(),
        character_profile: "hero".to_string(),
        key_story_beats: "beats".to_string(),
    })
    .await
    .expect("session metadata should save");
    repo.save_rounds(
        "session-backtrack",
        &[
            RoundHistoryEntry {
                round: 1,
                narration_text: Some("钟楼门前雾气翻涌。".to_string()),
                choices: vec![
                    PendingCharacterChoice {
                        id: "choice-1".to_string(),
                        option: old_choice.clone(),
                    },
                    PendingCharacterChoice {
                        id: "choice-2".to_string(),
                        option: branch_choice.clone(),
                    },
                ],
                ..RoundHistoryEntry::default()
            },
            RoundHistoryEntry {
                round: 2,
                narration_text: Some("你推开正门，木轴发出长声。".to_string()),
                ..RoundHistoryEntry::default()
            },
        ],
    )
    .await
    .expect("rounds should save");
    repo.update_session_turn_state("session-backtrack", TurnPhase::AwaitingPlayer, 1, 1)
        .await
        .expect("source node should become active");
    repo.save_player_input(&PlayerInput {
        session_id: "session-backtrack".to_string(),
        round: 1,
        actions: vec![PlayerActionItem::character_selected_option(&old_choice)],
    })
    .await
    .expect("old story edge should save");
    repo.update_session_turn_state("session-backtrack", TurnPhase::AwaitingPlayer, 2, 2)
        .await
        .expect("old future should become active");

    let branch = repo
        .prepare_backtrack_branch(
            "session-backtrack",
            1,
            &[PlayerActionItem::character_selected_option(&branch_choice)],
        )
        .await
        .expect("backtrack branch should prepare");
    assert!(!branch.reused_existing_branch);
    assert_eq!(branch.branch_round, 2);

    repo.save_flow_turn_update(&FlowTurnUpdate {
        session_id: "session-backtrack".to_string(),
        round: 2,
        stage: TurnPhase::Application,
        entity_name: "UpperNarrator".to_string(),
        output_type: AgentOutputType::Text,
        content: "你绕到钟楼背面，潮湿砖缝里透出微光。".to_string(),
    })
    .await
    .expect("branch narration should save");

    let active_rounds = repo
        .load_rounds("session-backtrack")
        .await
        .expect("active path rounds should load");
    assert_eq!(active_rounds.len(), 2);
    assert_eq!(
        active_rounds[1].narration_text.as_deref(),
        Some("你绕到钟楼背面，潮湿砖缝里透出微光。")
    );

    let conn = Connection::open(db_path).expect("sqlite db should open");
    let old_linear_narration: String = conn
        .query_row(
            r#"
            SELECT content
            FROM entity_flow_outputs
            WHERE session_id = ?1
                AND node_id = 'node-2'
                AND output_type = 'text'
            "#,
            params!["session-backtrack"],
            |row| row.get(0),
        )
        .expect("old linear future output should remain");
    let depth_two_node_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM story_nodes WHERE session_id = ?1 AND node_depth = 2",
            params!["session-backtrack"],
            |row| row.get(0),
        )
        .expect("story nodes should be countable");
    assert_eq!(old_linear_narration, "你推开正门，木轴发出长声。");
    assert_eq!(depth_two_node_count, 2);

    let reused = repo
        .prepare_backtrack_branch(
            "session-backtrack",
            1,
            &[PlayerActionItem::character_selected_option(&branch_choice)],
        )
        .await
        .expect("existing branch should activate");
    assert!(reused.reused_existing_branch);
    let depth_two_node_count_after_reuse: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM story_nodes WHERE session_id = ?1 AND node_depth = 2",
            params!["session-backtrack"],
            |row| row.get(0),
        )
        .expect("story nodes should be countable");
    assert_eq!(depth_two_node_count_after_reuse, 2);
    let reused_metadata = repo
        .load_session_metadata("session-backtrack")
        .await
        .expect("metadata should load")
        .expect("metadata should exist");
    assert_eq!(reused_metadata.phase, TurnPhase::AwaitingPlayer);
    assert_eq!(reused_metadata.turn_index, 2);
    assert_eq!(reused_metadata.active_turn_id, 2);

    let downstream_choice = CharacterOption {
        title: "推开后门".to_string(),
        action: "从钟楼背面的窄门潜入".to_string(),
        motivation_and_risk: "可以避开正门视线，但里面可能有机关".to_string(),
    };
    let downstream = repo
        .prepare_backtrack_branch(
            "session-backtrack",
            2,
            &[PlayerActionItem::character_selected_option(
                &downstream_choice,
            )],
        )
        .await
        .expect("downstream branch should prepare");
    assert_eq!(downstream.branch_round, 3);
    repo.save_flow_turn_update(&FlowTurnUpdate {
        session_id: "session-backtrack".to_string(),
        round: 3,
        stage: TurnPhase::Application,
        entity_name: "UpperNarrator".to_string(),
        output_type: AgentOutputType::Text,
        content: "窄门后传来齿轮咬合的细声。".to_string(),
    })
    .await
    .expect("downstream narration should save");

    repo.prepare_backtrack_branch(
        "session-backtrack",
        1,
        &[PlayerActionItem::character_selected_option(&branch_choice)],
    )
    .await
    .expect("parent branch should reactivate");
    let reactivated_actions = repo
        .load_story_edge_actions("session-backtrack")
        .await
        .expect("reactivated branch actions should load");
    assert_eq!(reactivated_actions.len(), 1);
    assert_eq!(reactivated_actions[0].round, 1);
    assert_eq!(reactivated_actions[0].action.action, branch_choice.action);
    let parent_rounds = repo
        .load_rounds("session-backtrack")
        .await
        .expect("parent branch rounds should load");
    assert_eq!(parent_rounds.len(), 2);
    assert_eq!(
        parent_rounds[1].narration_text.as_deref(),
        Some("你绕到钟楼背面，潮湿砖缝里透出微光。")
    );
}

#[tokio::test]
async fn backtrack_generation_persists_outputs_to_prepared_branch_node() {
    let db_path = std::env::temp_dir().join(format!(
        "akasa-backtrack-target-node-{}.sqlite3",
        Uuid::new_v4().simple()
    ));
    let repo = SessionArchiveRepository::new(AppDatabase::new(db_path.clone()));
    let old_choice = CharacterOption {
        title: "翻窗上屋顶，暂时躲避".to_string(),
        action: "翻窗上屋顶，暂时躲避搜查队".to_string(),
        motivation_and_risk: "可以避开搜查，但屋顶容易暴露身形".to_string(),
    };
    let branch_choice = CharacterOption {
        title: "继续研读古卷，试图找到更多线索".to_string(),
        action: "继续研读古卷，寻找父亲下落和猎杀标记的线索".to_string(),
        motivation_and_risk: "能获得更多情报，但会耽误逃离时机".to_string(),
    };

    repo.save_session_created(&SessionCreated {
        session_id: "session-backtrack-target".to_string(),
        character_name: "hero".to_string(),
        world_profile: "world".to_string(),
        character_profile: "hero".to_string(),
        key_story_beats: "beats".to_string(),
    })
    .await
    .expect("session metadata should save");
    repo.save_rounds(
        "session-backtrack-target",
        &[RoundHistoryEntry {
            round: 1,
            narration_text: Some("阁楼外传来搜查队的脚步声。".to_string()),
            choices: vec![
                PendingCharacterChoice {
                    id: "choice-1".to_string(),
                    option: old_choice.clone(),
                },
                PendingCharacterChoice {
                    id: "choice-2".to_string(),
                    option: branch_choice.clone(),
                },
            ],
            ..RoundHistoryEntry::default()
        }],
    )
    .await
    .expect("round choices should save");
    repo.update_session_turn_state("session-backtrack-target", TurnPhase::AwaitingPlayer, 1, 1)
        .await
        .expect("source node should become active");
    repo.save_player_input(&PlayerInput {
        session_id: "session-backtrack-target".to_string(),
        round: 1,
        actions: vec![PlayerActionItem::character_selected_option(&old_choice)],
    })
    .await
    .expect("old story edge should save");
    repo.update_session_turn_state("session-backtrack-target", TurnPhase::Application, 1, 2)
        .await
        .expect("old future should become active");
    repo.save_flow_turn_update(&FlowTurnUpdate {
        session_id: "session-backtrack-target".to_string(),
        round: 2,
        stage: TurnPhase::Application,
        entity_name: "UpperNarrator".to_string(),
        output_type: AgentOutputType::Text,
        content: "你翻上屋顶，夜风压低了瓦片间的声响。".to_string(),
    })
    .await
    .expect("old branch narration should save");

    let branch = repo
        .prepare_backtrack_branch(
            "session-backtrack-target",
            1,
            &[PlayerActionItem::character_selected_option(&branch_choice)],
        )
        .await
        .expect("backtrack branch should prepare");
    assert!(!branch.reused_existing_branch);
    assert_eq!(branch.branch_round, 2);
    assert_ne!(branch.branch_node_id, "node-2");

    repo.prepare_backtrack_branch(
        "session-backtrack-target",
        1,
        &[PlayerActionItem::character_selected_option(&old_choice)],
    )
    .await
    .expect("sibling branch should reactivate");
    repo.save_player_input(&PlayerInput {
        session_id: "session-backtrack-target".to_string(),
        round: 1,
        actions: vec![PlayerActionItem::character_selected_option(&branch_choice)],
    })
    .await
    .expect("prepared branch edge action should save to prepared target");
    repo.update_session_turn_state_for_node(
        "session-backtrack-target",
        &branch.branch_node_id,
        TurnPhase::Application,
    )
    .await
    .expect("target branch node state should update explicitly");
    repo.save_flow_turn_update_for_node(
        &FlowTurnUpdate {
            session_id: "session-backtrack-target".to_string(),
            round: 2,
            stage: TurnPhase::Application,
            entity_name: "UpperNarrator".to_string(),
            output_type: AgentOutputType::Text,
            content: "你继续研读古卷，符文在煤油灯下逐渐连成一条暗线。".to_string(),
        },
        &branch.branch_node_id,
    )
    .await
    .expect("target branch narration should save");

    let conn = Connection::open(db_path).expect("sqlite db should open");
    let linear_narration: String = conn
        .query_row(
            r#"
            SELECT content
            FROM entity_flow_outputs
            WHERE session_id = ?1
                AND node_id = 'node-2'
                AND stage = 'application'
                AND entity_name = 'UpperNarrator'
                AND output_type = 'text'
            "#,
            params!["session-backtrack-target"],
            |row| row.get(0),
        )
        .expect("linear narration should remain");
    let linear_action: String = conn
        .query_row(
            r#"
            SELECT action
            FROM story_edge_actions
            WHERE session_id = ?1
                AND from_node_id = 'node-1'
                AND to_node_id = 'node-2'
            "#,
            params!["session-backtrack-target"],
            |row| row.get(0),
        )
        .expect("linear action should remain");
    let branch_action: String = conn
        .query_row(
            r#"
            SELECT action
            FROM story_edge_actions
            WHERE session_id = ?1
                AND from_node_id = 'node-1'
                AND to_node_id = ?2
            "#,
            params!["session-backtrack-target", &branch.branch_node_id],
            |row| row.get(0),
        )
        .expect("branch action should remain");
    let branch_narration: String = conn
        .query_row(
            r#"
            SELECT content
            FROM entity_flow_outputs
            WHERE session_id = ?1
                AND node_id = ?2
                AND stage = 'application'
                AND entity_name = 'UpperNarrator'
                AND output_type = 'text'
            "#,
            params!["session-backtrack-target", &branch.branch_node_id],
            |row| row.get(0),
        )
        .expect("branch narration should exist");
    assert_eq!(linear_narration, "你翻上屋顶，夜风压低了瓦片间的声响。");
    assert_eq!(linear_action, old_choice.action);
    assert_eq!(branch_action, branch_choice.action);
    assert_eq!(
        branch_narration,
        "你继续研读古卷，符文在煤油灯下逐渐连成一条暗线。"
    );

    let reused = repo
        .prepare_backtrack_branch(
            "session-backtrack-target",
            1,
            &[PlayerActionItem::character_selected_option(&branch_choice)],
        )
        .await
        .expect("stored branch should reactivate");
    assert!(reused.reused_existing_branch);
    assert_eq!(reused.branch_node_id, branch.branch_node_id);
}

#[tokio::test]
async fn active_leaf_does_not_overlay_its_existing_child_edge() {
    let db_path = std::env::temp_dir().join(format!(
        "akasa-active-leaf-child-edge-{}.sqlite3",
        Uuid::new_v4().simple()
    ));
    let repo = SessionArchiveRepository::new(AppDatabase::new(db_path.clone()));
    let choice_1 = CharacterOption {
        title: "进入二层".to_string(),
        action: "沿楼梯进入二层".to_string(),
        motivation_and_risk: "更接近线索，但可能遇到守卫".to_string(),
    };
    let choice_2 = CharacterOption {
        title: "进入三层".to_string(),
        action: "继续上到三层".to_string(),
        motivation_and_risk: "视野更好，但退路变窄".to_string(),
    };
    let choice_3 = CharacterOption {
        title: "推开档案室门".to_string(),
        action: "推开三层档案室的门".to_string(),
        motivation_and_risk: "能确认档案线索，但门后可能有埋伏".to_string(),
    };

    repo.save_session_created(&SessionCreated {
        session_id: "session-active-leaf".to_string(),
        character_name: "hero".to_string(),
        world_profile: "world".to_string(),
        character_profile: "hero".to_string(),
        key_story_beats: "beats".to_string(),
    })
    .await
    .expect("session metadata should save");
    repo.save_rounds(
        "session-active-leaf",
        &[
            RoundHistoryEntry {
                round: 1,
                choices: vec![PendingCharacterChoice {
                    id: "choice-1".to_string(),
                    option: choice_1.clone(),
                }],
                ..RoundHistoryEntry::default()
            },
            RoundHistoryEntry {
                round: 2,
                choices: vec![PendingCharacterChoice {
                    id: "choice-1".to_string(),
                    option: choice_2.clone(),
                }],
                ..RoundHistoryEntry::default()
            },
            RoundHistoryEntry {
                round: 3,
                choices: vec![PendingCharacterChoice {
                    id: "choice-1".to_string(),
                    option: choice_3.clone(),
                }],
                ..RoundHistoryEntry::default()
            },
            RoundHistoryEntry {
                round: 4,
                narration_text: Some("档案室门后尘埃浮起。".to_string()),
                ..RoundHistoryEntry::default()
            },
        ],
    )
    .await
    .expect("rounds should save");

    for (round, choice) in [(1, &choice_1), (2, &choice_2), (3, &choice_3)] {
        repo.update_session_turn_state(
            "session-active-leaf",
            TurnPhase::AwaitingPlayer,
            round,
            round,
        )
        .await
        .expect("source node should become active");
        repo.save_player_input(&PlayerInput {
            session_id: "session-active-leaf".to_string(),
            round,
            actions: vec![PlayerActionItem::character_selected_option(choice)],
        })
        .await
        .expect("story edge should save");
    }

    repo.update_session_turn_state("session-active-leaf", TurnPhase::AwaitingPlayer, 3, 3)
        .await
        .expect("round 3 should become active");
    let leaf_actions = repo
        .load_story_edge_actions("session-active-leaf")
        .await
        .expect("leaf story edge actions should load");
    assert_eq!(leaf_actions.len(), 2);
    assert_eq!(leaf_actions[0].round, 1);
    assert_eq!(leaf_actions[1].round, 2);
    assert!(
        !repo
            .has_story_edge_action_for_round("session-active-leaf", 3)
            .await
            .expect("duplicate check should run")
    );

    repo.update_session_turn_state("session-active-leaf", TurnPhase::AwaitingPlayer, 4, 4)
        .await
        .expect("round 4 should become active");
    let active_path_actions = repo
        .load_story_edge_actions("session-active-leaf")
        .await
        .expect("active path story edge actions should load");
    assert_eq!(active_path_actions.len(), 3);
    assert_eq!(active_path_actions[2].round, 3);
    assert!(
        repo.has_story_edge_action_for_round("session-active-leaf", 3)
            .await
            .expect("duplicate check should run")
    );
}

#[tokio::test]
async fn backtrack_existing_empty_branch_requires_generation() {
    let db_path = std::env::temp_dir().join(format!(
        "akasa-backtrack-empty-branch-{}.sqlite3",
        Uuid::new_v4().simple()
    ));
    let repo = SessionArchiveRepository::new(AppDatabase::new(db_path.clone()));
    let branch_choice = CharacterOption {
        title: "绕行".to_string(),
        action: "绕到钟楼背面".to_string(),
        motivation_and_risk: "视野更好，但会暴露脚步声".to_string(),
    };

    repo.save_session_created(&SessionCreated {
        session_id: "session-empty-branch".to_string(),
        character_name: "hero".to_string(),
        world_profile: "world".to_string(),
        character_profile: "hero".to_string(),
        key_story_beats: "beats".to_string(),
    })
    .await
    .expect("session metadata should save");
    repo.save_rounds(
        "session-empty-branch",
        &[RoundHistoryEntry {
            round: 1,
            narration_text: Some("钟楼门前雾气翻涌。".to_string()),
            choices: vec![PendingCharacterChoice {
                id: "choice-1".to_string(),
                option: branch_choice.clone(),
            }],
            ..RoundHistoryEntry::default()
        }],
    )
    .await
    .expect("rounds should save");
    repo.update_session_turn_state("session-empty-branch", TurnPhase::AwaitingPlayer, 1, 1)
        .await
        .expect("source node should become active");
    repo.save_player_input(&PlayerInput {
        session_id: "session-empty-branch".to_string(),
        round: 1,
        actions: vec![PlayerActionItem::character_selected_option(&branch_choice)],
    })
    .await
    .expect("empty branch edge should save");

    let branch = repo
        .prepare_backtrack_branch(
            "session-empty-branch",
            1,
            &[PlayerActionItem::character_selected_option(&branch_choice)],
        )
        .await
        .expect("existing empty branch should activate");

    assert_eq!(branch.branch_round, 2);
    assert!(!branch.reused_existing_branch);
    assert!(branch.requires_generation);

    let conn = Connection::open(db_path).expect("sqlite db should open");
    let depth_two_node_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM story_nodes WHERE session_id = ?1 AND node_depth = 2",
            params!["session-empty-branch"],
            |row| row.get(0),
        )
        .expect("story nodes should be countable");
    assert_eq!(depth_two_node_count, 1);
}

fn test_repo() -> SessionArchiveRepository {
    SessionArchiveRepository::new(AppDatabase::new(std::env::temp_dir().join(format!(
        "akasa-session-archives-{}.sqlite3",
        Uuid::new_v4().simple()
    ))))
}

fn round_entry(round: u64, narration: &str) -> RoundHistoryEntry {
    RoundHistoryEntry {
        round,
        narration_text: Some(narration.to_string()),
        ..RoundHistoryEntry::default()
    }
}

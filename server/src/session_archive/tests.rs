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
    let loaded_actions = repo
        .load_story_edge_actions("session-choice-edge")
        .await
        .expect("story edge actions should load");
    let has_round_one_action = repo
        .has_story_edge_action_for_round("session-choice-edge", 1)
        .await
        .expect("round duplicate check should run");
    let has_round_two_action = repo
        .has_story_edge_action_for_round("session-choice-edge", 2)
        .await
        .expect("round duplicate check should run");

    assert_eq!(edge_count, 1);
    assert!(has_round_one_action);
    assert!(!has_round_two_action);
    assert_eq!(stored_action.0, "hero");
    assert_eq!(stored_action.1, None);
    assert_eq!(stored_action.2, "selected_option");
    assert_eq!(stored_action.3, choice.option.title);
    assert_eq!(stored_action.4, choice.option.action);
    assert_eq!(stored_action.5, choice.option.motivation_and_risk);
    assert_eq!(loaded_actions.len(), 1);
    assert_eq!(loaded_actions[0].action.action, "绕到钟楼背面");
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
        .load_entity_contexts("session-db")
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

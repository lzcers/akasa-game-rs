use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use agent::agent::Context;
use chrono::{DateTime, Utc};
use story_engine::{
    components::{
        agent::{Agent as StoryAgent, AgentOutputType},
        outcome::{PendingProtagonistChoice, PlayerActionType},
        world_snapshot::WorldSnapshot,
    },
    engine::{AkashicEngine, AkashicSessionEngine},
    profile::DEFAULT_KEY_STORY_BEATS,
    resources::session_events::{EngineEvent, FlowTurnUpdate, SessionCreated},
};
use tokio::sync::{Mutex, broadcast};
use uuid::Uuid;

use crate::{
    analytics::{AnalyticsRepository, AnalyticsSummary},
    api::archive,
    api::game_sessions::{
        ControlGameSessionData, ControlGameSessionRequest, CreateGameSessionData,
        CreateGameSessionRequest, GameSessionControlCommand, GameSessionWorldStateData,
        GeneratedProfilesData, RoundHistoryData, SaveExportData, SessionActionInput,
        SessionRoundsPageData, WorldStateData,
    },
    api::site::{AnalyticsBatchRequest, SubmitFeedbackData, ValidatedFeedbackRequest},
    database::AppDatabase,
    email::{FeedbackEmail, FeedbackMailer},
    error::AppError,
    session_archive::{SessionArchiveRepository, StoredSessionMetadata, StoredStoryEdgeAction},
    session_history::{RoundHistoryEntry, SessionHistoryLog, TurnPhase},
};

#[derive(Clone)]
pub struct AppState {
    sessions: Arc<Mutex<HashMap<String, SessionRecord>>>,
    restore_lock: Arc<Mutex<()>>,
    analytics_repo: AnalyticsRepository,
    session_archive_repo: SessionArchiveRepository,
    feedback_mailer: FeedbackMailer,
    engine: AkashicEngine,
    lifecycle_config: SessionLifecycleConfig,
}

struct SessionRecord {
    session_id: String,
    _created_at: DateTime<Utc>,
    last_accessed_at: DateTime<Utc>,
    active_streams: usize,
    last_phase: TurnPhase,
    slot: SessionSlot,
}

enum SessionSlot {
    Hot(HotSession),
    Cooling {
        _started_at: DateTime<Utc>,
    },
    Cold {
        _saved_at: DateTime<Utc>,
        _summary: Option<String>,
    },
}

struct HotSession {
    engine: AkashicSessionEngine,
    events_tx: broadcast::Sender<LiveEngineEvent>,
    live_events: Arc<Mutex<LiveEventLog>>,
}

struct HotSessionAccess {
    session_id: String,
    engine: AkashicSessionEngine,
    events_tx: broadcast::Sender<LiveEngineEvent>,
    live_events: Arc<Mutex<LiveEventLog>>,
}

pub struct LiveSessionStream {
    pub session_id: String,
    pub replayed_events: Vec<LiveEngineEvent>,
    pub event_rx: broadcast::Receiver<LiveEngineEvent>,
    pub lease: LiveSessionLease,
}

pub struct LiveSessionLease {
    session_id: String,
    sessions: Arc<Mutex<HashMap<String, SessionRecord>>>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveEngineEvent {
    pub event_id: u64,
    pub event: EngineEvent,
}

#[derive(Default)]
struct LiveEventLog {
    next_event_id: u64,
    history: VecDeque<LiveEngineEvent>,
    capacity: usize,
}

const EVENT_CHANNEL_CAPACITY: usize = 256;
const DEFAULT_EVENT_HISTORY_CAPACITY: usize = 256;
const DEFAULT_SESSION_IDLE_TTL_SECS: u64 = 30 * 60;
const DEFAULT_SESSION_ENDED_TTL_SECS: u64 = 5 * 60;
const DEFAULT_MAX_HOT_SESSIONS: usize = 200;
const DEFAULT_SESSION_REAPER_INTERVAL_SECS: u64 = 60;
const MAX_SESSION_ROUNDS_PAGE_LIMIT: usize = 100;

#[derive(Clone, Debug)]
struct SessionLifecycleConfig {
    idle_ttl: Duration,
    ended_ttl: Duration,
    max_hot_sessions: usize,
    reaper_interval: Duration,
    live_event_history_capacity: usize,
}

impl Default for SessionLifecycleConfig {
    fn default() -> Self {
        Self {
            idle_ttl: Duration::from_secs(env_u64(
                "AKASA_SESSION_IDLE_TTL_SECS",
                DEFAULT_SESSION_IDLE_TTL_SECS,
            )),
            ended_ttl: Duration::from_secs(env_u64(
                "AKASA_SESSION_ENDED_TTL_SECS",
                DEFAULT_SESSION_ENDED_TTL_SECS,
            )),
            max_hot_sessions: env_usize("AKASA_MAX_HOT_SESSIONS", DEFAULT_MAX_HOT_SESSIONS).max(1),
            reaper_interval: Duration::from_secs(env_u64(
                "AKASA_SESSION_REAPER_INTERVAL_SECS",
                DEFAULT_SESSION_REAPER_INTERVAL_SECS,
            )),
            live_event_history_capacity: env_usize(
                "AKASA_LIVE_EVENT_HISTORY_CAPACITY",
                DEFAULT_EVENT_HISTORY_CAPACITY,
            )
            .max(1),
        }
    }
}

impl SessionLifecycleConfig {
    fn ttl_for_phase(&self, phase: TurnPhase) -> Duration {
        if matches!(phase, TurnPhase::Ended | TurnPhase::Failed) {
            self.ended_ttl
        } else {
            self.idle_ttl
        }
    }
}

impl AppState {
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self::with_lifecycle_config(database_path, SessionLifecycleConfig::default(), true)
    }

    fn with_lifecycle_config(
        database_path: impl Into<PathBuf>,
        lifecycle_config: SessionLifecycleConfig,
        spawn_reaper: bool,
    ) -> Self {
        let db = AppDatabase::new(database_path);
        let state = Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            restore_lock: Arc::new(Mutex::new(())),
            analytics_repo: AnalyticsRepository::new(db.clone()),
            session_archive_repo: SessionArchiveRepository::new(db),
            feedback_mailer: FeedbackMailer::from_env(),
            engine: AkashicEngine::new(),
            lifecycle_config,
        };
        if spawn_reaper {
            state.spawn_session_reaper();
        }
        state
    }

    pub async fn record_analytics_events(
        &self,
        request: AnalyticsBatchRequest,
        ip_address: Option<String>,
    ) -> Result<usize, AppError> {
        if request.events.len() > 100 {
            return Err(AppError::bad_request(
                "单次最多提交 100 条 analytics events。",
            ));
        }

        for event in &request.events {
            event.validate()?;
        }

        self.analytics_repo
            .append_events(&request.events, ip_address.as_deref())
            .await
            .map_err(|err| AppError::internal(format!("写入埋点事件失败：{err:#}")))
    }

    pub async fn analytics_summary(&self, range_hours: u32) -> Result<AnalyticsSummary, AppError> {
        self.analytics_repo
            .summary(range_hours)
            .await
            .map_err(|err| AppError::internal(format!("读取埋点指标失败：{err:#}")))
    }

    pub async fn send_feedback(
        &self,
        request: ValidatedFeedbackRequest,
    ) -> Result<SubmitFeedbackData, AppError> {
        let feedback_id = format!("feedback-{}", Uuid::new_v4().simple());
        let submitted_at = now_string();

        self.feedback_mailer
            .send_feedback(FeedbackEmail {
                feedback_id: &feedback_id,
                submitted_at: &submitted_at,
                request: &request,
            })
            .await
            .map_err(|err| AppError::internal(format!("发送反馈邮件失败：{err:#}")))?;

        Ok(SubmitFeedbackData {
            feedback_id,
            accepted: true,
        })
    }

    // 创建会话
    pub async fn create_game_session(
        &self,
        request: CreateGameSessionRequest,
    ) -> Result<CreateGameSessionData, AppError> {
        let session_id = format!("session-{}", Uuid::new_v4().simple());
        let created_at = now_string();
        let world_profile = request.world_profile.trim();
        let protagonist_profile = request.protagonist_profile.trim();
        let key_story_beats = request.key_story_beats.trim();
        if world_profile.is_empty() || protagonist_profile.is_empty() {
            return Err(AppError::bad_request(
                "`worldProfile` 与 `protagonistProfile` 不能为空。",
            ));
        }
        let engine = self
            .engine
            .create_session(
                &session_id,
                world_profile,
                protagonist_profile,
                if key_story_beats.is_empty() {
                    DEFAULT_KEY_STORY_BEATS
                } else {
                    key_story_beats
                },
            )
            .await
            .map_err(AppError::internal)?;
        let key_story_beats = if key_story_beats.is_empty() {
            DEFAULT_KEY_STORY_BEATS
        } else {
            key_story_beats
        };
        self.session_archive_repo
            .save_session_created(&SessionCreated {
                session_id: session_id.clone(),
                world_profile: world_profile.to_string(),
                protagonist_profile: protagonist_profile.to_string(),
                key_story_beats: key_story_beats.to_string(),
            })
            .await
            .map_err(|err| AppError::internal(format!("写入会话元数据失败：{err:#}")))?;
        let fate_weaver =
            StoryAgent::new_fate_weaver(world_profile, protagonist_profile, key_story_beats);
        let upper_narrator = StoryAgent::new_upper_narrator(world_profile, protagonist_profile);
        let protagonist = StoryAgent::new_protagonist(world_profile, protagonist_profile);
        self.session_archive_repo
            .replace_agent_contexts_from_contexts(
                &session_id,
                0,
                &[
                    ("FateWeaver", &fate_weaver.context),
                    ("UpperNarrator", &upper_narrator.context),
                    ("Protagonist", &protagonist.context),
                ],
            )
            .await
            .map_err(|err| AppError::internal(format!("写入初始 Agent context 失败：{err:#}")))?;
        let session = build_session_record(
            session_id.clone(),
            engine,
            self.lifecycle_config.live_event_history_capacity,
            self.session_archive_repo.clone(),
            TurnPhase::Start,
        );

        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), session);
        self.reap_idle_sessions_once().await?;
        Ok(CreateGameSessionData {
            session_id,
            created_at,
        })
    }

    pub async fn export_save_archive(
        &self,
        session_id: &str,
        title: Option<&str>,
    ) -> Result<SaveExportData, AppError> {
        let archive = self.archive_payload_for_session(session_id, title).await?;
        let exported_at = now_string();
        let compressed_archive =
            archive::compress_archive_payload(&archive).map_err(AppError::internal)?;

        Ok(SaveExportData {
            session_id: archive.session_id.clone(),
            title: archive.title.clone(),
            created_at: exported_at.clone(),
            updated_at: exported_at,
            compressed_archive,
        })
    }

    pub async fn load_game_session_from_archive(
        &self,
        compressed_archive: String,
    ) -> Result<GameSessionWorldStateData, AppError> {
        let payload = archive::decompress_archive_payload(&compressed_archive)
            .map_err(AppError::bad_request)?;
        archive::validate_archive_payload(&payload).map_err(AppError::bad_request)?;
        let session_id = payload.session_id.clone();
        let payload_for_storage = payload.clone();
        self.persist_payload_database_state(&payload_for_storage)
            .await?;
        let engine = archive::load_archive_payload(&self.engine, payload)
            .await
            .map_err(AppError::bad_request)?;
        let session = build_session_record(
            session_id.clone(),
            engine,
            self.lifecycle_config.live_event_history_capacity,
            self.session_archive_repo.clone(),
            payload_for_storage.turn_state.phase,
        );

        self.sessions.lock().await.insert(session_id, session);
        self.reap_idle_sessions_once().await?;

        self.get_game_session_world(&payload_for_storage.session_id)
            .await
    }

    pub async fn get_game_session_world(
        &self,
        session_id: &str,
    ) -> Result<GameSessionWorldStateData, AppError> {
        self.touch_session(session_id).await?;
        self.game_session_world_from_database(session_id).await
    }

    pub async fn get_game_session_rounds(
        &self,
        session_id: &str,
        before_round: Option<u64>,
        limit: usize,
    ) -> Result<SessionRoundsPageData, AppError> {
        let limit = limit.clamp(1, MAX_SESSION_ROUNDS_PAGE_LIMIT);
        self.touch_session(session_id).await?;
        let page = self
            .session_archive_repo
            .load_round_page(session_id, before_round, limit)
            .await
            .map_err(|err| AppError::internal(format!("读取会话历史分页失败：{err:#}")))?;

        Ok(SessionRoundsPageData {
            session_id: session_id.to_string(),
            rounds: page
                .rounds
                .into_iter()
                .map(RoundHistoryData::from)
                .collect(),
            next_before_round: page.next_before_round,
            has_more: page.has_more,
        })
    }

    pub async fn clone_game_session(
        &self,
        source_session_id: &str,
    ) -> Result<GameSessionWorldStateData, AppError> {
        let source_session_id = source_session_id.trim();
        if source_session_id.is_empty() {
            return Err(AppError::bad_request("`sessionId` 不能为空。"));
        }

        let clone_session_id = format!("session-{}", Uuid::new_v4().simple());
        let mut payload = self
            .archive_payload_for_session(source_session_id, None)
            .await?;
        payload.session_id = clone_session_id.clone();
        payload.title = format!("{}（分支）", payload.title.trim());
        let payload_for_storage = payload.clone();
        self.persist_payload_database_state(&payload_for_storage)
            .await?;

        let engine = archive::load_archive_payload(&self.engine, payload)
            .await
            .map_err(AppError::bad_request)?;
        let session = build_session_record(
            clone_session_id.clone(),
            engine,
            self.lifecycle_config.live_event_history_capacity,
            self.session_archive_repo.clone(),
            payload_for_storage.turn_state.phase,
        );

        self.sessions.lock().await.insert(clone_session_id, session);
        self.reap_idle_sessions_once().await?;

        self.get_game_session_world(&payload_for_storage.session_id)
            .await
    }

    pub async fn get_game_session_narrations(
        &self,
        session_id: &str,
    ) -> Result<Vec<String>, AppError> {
        let payload = self.archive_payload_for_session(session_id, None).await?;
        Ok(collect_history_narrations(&payload.history_log.rounds))
    }

    pub async fn control_game_session(
        &self,
        session_id: &str,
        request: ControlGameSessionRequest,
    ) -> Result<ControlGameSessionData, AppError> {
        let hot = self.ensure_hot_session(session_id, false).await?;
        let result = match (request.control, request.action) {
            (Some(control), None) => apply_control(&hot.engine, control),
            (None, Some(action)) => {
                self.apply_action_from_database(&hot.engine, session_id, action)
                    .await
            }
            (None, None) => Err(AppError::bad_request(
                "请求体至少需要提供 `control` 或 `action` 之一。",
            )),
            (Some(_), Some(_)) => Err(AppError::bad_request(
                "同一次请求只能执行一种操作：控制命令或玩家行动。",
            )),
        }?;
        Ok(result)
    }

    pub async fn open_game_session_stream(
        &self,
        session_id: &str,
        since: Option<u64>,
    ) -> Result<LiveSessionStream, AppError> {
        let hot = self.ensure_hot_session(session_id, true).await?;
        let live_events = hot.live_events.lock().await;
        let event_rx = hot.events_tx.subscribe();
        let replayed_events = live_events
            .history
            .iter()
            .filter(|event| since.is_none_or(|event_id| event.event_id > event_id))
            .cloned()
            .collect();

        Ok(LiveSessionStream {
            session_id: hot.session_id.clone(),
            replayed_events,
            event_rx,
            lease: LiveSessionLease {
                session_id: hot.session_id,
                sessions: Arc::clone(&self.sessions),
            },
        })
    }

    async fn apply_action_from_database(
        &self,
        engine: &AkashicSessionEngine,
        session_id: &str,
        action: SessionActionInput,
    ) -> Result<ControlGameSessionData, AppError> {
        let selected_action = action.action.trim();
        if selected_action.is_empty() {
            return Err(AppError::bad_request("提交行动不能为空。"));
        }

        if action.r#type == PlayerActionType::SelectedOption {
            let choices = self.latest_choices_for_session(session_id).await?;
            let is_valid_option = choices
                .iter()
                .any(|choice| choice.option.action == selected_action);
            if !is_valid_option {
                return Err(AppError::bad_request("当前所选行动不在候选列表中。"));
            }
        }

        engine
            .submit_player_action(SessionActionInput {
                r#type: action.r#type,
                action: selected_action.to_string(),
            })
            .map_err(AppError::bad_request)?;
        engine.start_next_turn().map_err(AppError::bad_request)?;

        Ok(ControlGameSessionData {
            action: "submit_action".to_string(),
        })
    }

    async fn latest_choices_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<PendingProtagonistChoice>, AppError> {
        let rounds = self
            .session_archive_repo
            .load_rounds(session_id)
            .await
            .map_err(|err| AppError::internal(format!("读取会话选项失败：{err:#}")))?;
        Ok(rounds
            .iter()
            .rev()
            .find(|round| !round.choices.is_empty())
            .map(|round| round.choices.clone())
            .unwrap_or_default())
    }

    async fn ensure_hot_session(
        &self,
        session_id: &str,
        register_stream: bool,
    ) -> Result<HotSessionAccess, AppError> {
        loop {
            let cold_session_id = {
                let mut sessions = self.sessions.lock().await;
                let record = sessions
                    .get_mut(session_id)
                    .ok_or_else(|| AppError::not_found(format!("未找到会话 `{session_id}`")))?;
                record.touch(Utc::now());
                match &record.slot {
                    SessionSlot::Hot(hot) => {
                        let engine = hot.engine.clone();
                        let events_tx = hot.events_tx.clone();
                        let live_events = Arc::clone(&hot.live_events);
                        if register_stream {
                            record.active_streams += 1;
                        }
                        return Ok(HotSessionAccess {
                            session_id: record.session_id.clone(),
                            engine,
                            events_tx,
                            live_events,
                        });
                    }
                    SessionSlot::Cooling { .. } => None,
                    SessionSlot::Cold { .. } => Some(record.session_id.clone()),
                }
            };

            let Some(cold_session_id) = cold_session_id else {
                tokio::time::sleep(Duration::from_millis(25)).await;
                continue;
            };

            let _restore_guard = self.restore_lock.lock().await;
            if let Some(access) = self
                .hot_session_access_if_available(session_id, register_stream)
                .await?
            {
                return Ok(access);
            }

            let payload = self
                .archive_payload_for_session(&cold_session_id, None)
                .await?;
            archive::validate_archive_payload(&payload).map_err(AppError::internal)?;
            let restored_phase = payload.turn_state.phase;
            let engine = archive::load_archive_payload(&self.engine, payload)
                .await
                .map_err(AppError::internal)?;
            let hot = build_hot_session(
                session_id,
                engine,
                self.lifecycle_config.live_event_history_capacity,
                self.session_archive_repo.clone(),
                restored_phase,
            );

            let mut sessions = self.sessions.lock().await;
            let record = sessions
                .get_mut(session_id)
                .ok_or_else(|| AppError::not_found(format!("未找到会话 `{session_id}`")))?;
            record.touch(Utc::now());
            record.last_phase = restored_phase;
            record.slot = SessionSlot::Hot(hot);
            if register_stream {
                record.active_streams += 1;
            }

            if let SessionSlot::Hot(hot) = &record.slot {
                return Ok(HotSessionAccess {
                    session_id: record.session_id.clone(),
                    engine: hot.engine.clone(),
                    events_tx: hot.events_tx.clone(),
                    live_events: Arc::clone(&hot.live_events),
                });
            }
        }
    }

    async fn hot_session_access_if_available(
        &self,
        session_id: &str,
        register_stream: bool,
    ) -> Result<Option<HotSessionAccess>, AppError> {
        let mut sessions = self.sessions.lock().await;
        let record = sessions
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found(format!("未找到会话 `{session_id}`")))?;
        record.touch(Utc::now());
        let SessionSlot::Hot(hot) = &record.slot else {
            return Ok(None);
        };
        let engine = hot.engine.clone();
        let events_tx = hot.events_tx.clone();
        let live_events = Arc::clone(&hot.live_events);
        if register_stream {
            record.active_streams += 1;
        }
        Ok(Some(HotSessionAccess {
            session_id: record.session_id.clone(),
            engine,
            events_tx,
            live_events,
        }))
    }

    async fn archive_payload_for_session(
        &self,
        session_id: &str,
        title: Option<&str>,
    ) -> Result<archive::SessionArchivePayload, AppError> {
        self.touch_session(session_id).await?;
        let mut payload = self
            .archive_payload_from_database(session_id, title)
            .await?;
        stabilize_archive_payload_from_history(&mut payload, title)
            .map_err(AppError::bad_request)?;
        Ok(payload)
    }

    async fn persist_payload_database_state(
        &self,
        payload: &archive::SessionArchivePayload,
    ) -> Result<(), AppError> {
        self.session_archive_repo
            .save_session_metadata(&StoredSessionMetadata {
                session_id: payload.session_id.clone(),
                world_profile: payload.world_profile.clone(),
                protagonist_profile: payload.protagonist_profile.clone(),
                key_story_beats: payload.key_story_beats.clone(),
                phase: payload.turn_state.phase,
                turn_index: payload.turn_state.turn_index,
                active_turn_id: payload.turn_state.active_turn_id,
                flow_end: payload.turn_state.phase == TurnPhase::Ended,
            })
            .await
            .map_err(|err| AppError::internal(format!("写入会话元数据失败：{err:#}")))?;
        self.persist_rounds(&payload.session_id, &payload.history_log.rounds)
            .await?;
        self.session_archive_repo
            .replace_story_edges_from_rounds(&payload.session_id, &payload.history_log.rounds)
            .await
            .map_err(|err| AppError::internal(format!("写入故事边行动失败：{err:#}")))?;
        self.session_archive_repo
            .replace_agent_contexts_from_contexts(
                &payload.session_id,
                payload
                    .turn_state
                    .active_turn_id
                    .max(payload.turn_state.turn_index),
                &[
                    ("FateWeaver", &payload.fate_weaver),
                    ("UpperNarrator", &payload.upper_narrator),
                    ("Protagonist", &payload.protagonist),
                ],
            )
            .await
            .map_err(|err| AppError::internal(format!("写入 Agent context 失败：{err:#}")))?;
        Ok(())
    }

    async fn game_session_world_from_database(
        &self,
        session_id: &str,
    ) -> Result<GameSessionWorldStateData, AppError> {
        let metadata = self.load_session_metadata(session_id).await?;
        let rounds = self.load_session_rounds(session_id).await?;
        Ok(world_state_from_database(metadata, &rounds))
    }

    async fn archive_payload_from_database(
        &self,
        session_id: &str,
        title: Option<&str>,
    ) -> Result<archive::SessionArchivePayload, AppError> {
        let metadata = self.load_session_metadata(session_id).await?;
        let rounds = self.load_session_rounds(session_id).await?;
        let contexts = self.load_agent_context_map(session_id).await?;
        Ok(archive_payload_from_database(
            metadata, rounds, contexts, title,
        ))
    }

    async fn load_session_metadata(
        &self,
        session_id: &str,
    ) -> Result<StoredSessionMetadata, AppError> {
        self.session_archive_repo
            .load_session_metadata(session_id)
            .await
            .map_err(|err| AppError::internal(format!("读取会话元数据失败：{err:#}")))?
            .ok_or_else(|| AppError::not_found(format!("未找到会话 `{session_id}`")))
    }

    async fn load_session_rounds(
        &self,
        session_id: &str,
    ) -> Result<Vec<RoundHistoryEntry>, AppError> {
        let rounds = self
            .session_archive_repo
            .load_rounds(session_id)
            .await
            .map_err(|err| AppError::internal(format!("读取完整会话历史失败：{err:#}")))?;
        let story_edge_actions = self
            .session_archive_repo
            .load_story_edge_actions(session_id)
            .await
            .map_err(|err| AppError::internal(format!("读取故事边行动失败：{err:#}")))?;
        Ok(rounds_with_story_edge_actions(rounds, story_edge_actions))
    }

    async fn load_agent_context_map(
        &self,
        session_id: &str,
    ) -> Result<HashMap<String, Context>, AppError> {
        let contexts = self
            .session_archive_repo
            .load_agent_contexts(session_id)
            .await
            .map_err(|err| AppError::internal(format!("读取 Agent Context 失败：{err:#}")))?;
        Ok(contexts
            .into_iter()
            .map(|context| (context.agent_name, context.context))
            .collect())
    }

    async fn touch_session(&self, session_id: &str) -> Result<(), AppError> {
        {
            let mut sessions = self.sessions.lock().await;
            if let Some(record) = sessions.get_mut(session_id) {
                record.touch(Utc::now());
                return Ok(());
            }
        }
        self.load_session_metadata(session_id).await?;
        Ok(())
    }

    async fn persist_rounds(
        &self,
        session_id: &str,
        rounds: &[RoundHistoryEntry],
    ) -> Result<(), AppError> {
        self.session_archive_repo
            .save_rounds(session_id, rounds)
            .await
            .map_err(|err| AppError::internal(format!("写入会话轮次历史失败：{err:#}")))
    }

    pub async fn reap_idle_sessions_once(&self) -> Result<usize, AppError> {
        let candidate_ids = self.collect_eviction_candidates().await;
        let mut evicted = 0;
        for session_id in candidate_ids {
            if self.cool_hot_session(&session_id).await? {
                evicted += 1;
            }
        }
        Ok(evicted)
    }

    async fn collect_eviction_candidates(&self) -> Vec<String> {
        let now = Utc::now();
        let sessions = self.sessions.lock().await;
        let hot_count = sessions
            .values()
            .filter(|record| matches!(record.slot, SessionSlot::Hot(_)))
            .count();
        let mut candidates = Vec::new();
        let mut lru_candidates = Vec::new();

        for record in sessions.values() {
            let SessionSlot::Hot(_) = record.slot else {
                continue;
            };
            if record.active_streams > 0 || !is_evictable_phase(record.last_phase) {
                continue;
            }
            let idle_for = elapsed_since(now, record.last_accessed_at);
            if idle_for >= record_ttl(record, &self.lifecycle_config) {
                candidates.push(record.session_id.clone());
            }
            lru_candidates.push((record.last_accessed_at, record.session_id.clone()));
        }

        if hot_count > self.lifecycle_config.max_hot_sessions {
            lru_candidates.sort_by_key(|(last_accessed_at, _)| *last_accessed_at);
            let overflow = hot_count - self.lifecycle_config.max_hot_sessions;
            candidates.extend(
                lru_candidates
                    .into_iter()
                    .take(overflow)
                    .map(|(_, session_id)| session_id),
            );
        }

        let mut seen = HashSet::new();
        candidates
            .into_iter()
            .filter(|session_id| seen.insert(session_id.clone()))
            .collect()
    }

    async fn cool_hot_session(&self, session_id: &str) -> Result<bool, AppError> {
        let hot = {
            let mut sessions = self.sessions.lock().await;
            let Some(record) = sessions.get_mut(session_id) else {
                return Ok(false);
            };
            if record.active_streams > 0 || !is_evictable_phase(record.last_phase) {
                return Ok(false);
            }
            match std::mem::replace(
                &mut record.slot,
                SessionSlot::Cooling {
                    _started_at: Utc::now(),
                },
            ) {
                SessionSlot::Hot(hot) => hot,
                other => {
                    record.slot = other;
                    return Ok(false);
                }
            }
        };

        let payload = match self.archive_payload_for_session(session_id, None).await {
            Ok(payload) => payload,
            Err(error) => {
                self.restore_failed_cooling_session(session_id, hot).await;
                return Err(error);
            }
        };
        if archive::validate_archive_payload(&payload).is_err() {
            self.restore_failed_cooling_session(session_id, hot).await;
            return Ok(false);
        }
        if let Err(error) = hot.engine.close().await {
            self.restore_failed_cooling_session(session_id, hot).await;
            return Err(AppError::internal(format!(
                "关闭会话 `{session_id}` 失败：{error}"
            )));
        }

        let mut sessions = self.sessions.lock().await;
        let Some(record) = sessions.get_mut(session_id) else {
            return Ok(false);
        };
        record.last_phase = payload.turn_state.phase;
        record.slot = SessionSlot::Cold {
            _saved_at: Utc::now(),
            _summary: Some(payload.title),
        };
        Ok(true)
    }

    async fn restore_failed_cooling_session(&self, session_id: &str, hot: HotSession) {
        let mut sessions = self.sessions.lock().await;
        if let Some(record) = sessions.get_mut(session_id) {
            record.slot = SessionSlot::Hot(hot);
        }
    }

    fn spawn_session_reaper(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(state.lifecycle_config.reaper_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if let Err(error) = state.reap_idle_sessions_once().await {
                    tracing::warn!("session reaper failed: {:?}", error);
                }
            }
        });
    }
}

impl LiveEventLog {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            next_event_id: 0,
            history: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    fn push(&mut self, event: EngineEvent) -> LiveEngineEvent {
        self.next_event_id += 1;
        let event = LiveEngineEvent {
            event_id: self.next_event_id,
            event,
        };
        self.history.push_back(event.clone());
        while self.history.len() > self.capacity {
            self.history.pop_front();
        }
        event
    }
}

impl SessionRecord {
    fn touch(&mut self, now: DateTime<Utc>) {
        self.last_accessed_at = now;
    }
}

impl Drop for LiveSessionLease {
    fn drop(&mut self) {
        let session_id = self.session_id.clone();
        let sessions = Arc::clone(&self.sessions);
        tokio::spawn(async move {
            let mut sessions = sessions.lock().await;
            if let Some(record) = sessions.get_mut(&session_id) {
                record.active_streams = record.active_streams.saturating_sub(1);
                record.touch(Utc::now());
            }
        });
    }
}

fn build_session_record(
    session_id: String,
    engine: AkashicSessionEngine,
    live_event_history_capacity: usize,
    session_archive_repo: SessionArchiveRepository,
    initial_phase: TurnPhase,
) -> SessionRecord {
    let now = Utc::now();
    let hot = build_hot_session(
        &session_id,
        engine,
        live_event_history_capacity,
        session_archive_repo,
        initial_phase,
    );

    SessionRecord {
        session_id,
        _created_at: now,
        last_accessed_at: now,
        active_streams: 0,
        last_phase: initial_phase,
        slot: SessionSlot::Hot(hot),
    }
}

fn build_hot_session(
    session_id: &str,
    engine: AkashicSessionEngine,
    live_event_history_capacity: usize,
    session_archive_repo: SessionArchiveRepository,
    _initial_phase: TurnPhase,
) -> HotSession {
    let mut event_rx = engine.subscribe_session_events();
    let (events_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let live_events = Arc::new(Mutex::new(LiveEventLog::with_capacity(
        live_event_history_capacity,
    )));
    let events_tx_for_task = events_tx.clone();
    let live_events_for_task = Arc::clone(&live_events);
    let session_id_for_event_task = session_id.to_string();
    tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    let live_event = {
                        let mut live_events = live_events_for_task.lock().await;
                        live_events.push(event.clone())
                    };
                    let _ = events_tx_for_task.send(live_event);

                    match event {
                        EngineEvent::SessionCreated(created) => {
                            if let Err(error) =
                                session_archive_repo.save_session_created(&created).await
                            {
                                tracing::warn!(
                                    "failed to persist created session {}: {:?}",
                                    created.session_id,
                                    error
                                );
                            }
                        }
                        EngineEvent::TaskUpdate(_) | EngineEvent::TaskCompleted(_) => {}
                        EngineEvent::PlayerInput(input) => {
                            if let Err(error) = session_archive_repo.save_player_input(&input).await
                            {
                                tracing::warn!(
                                    "failed to persist story edge action for {} / {}: {:?}",
                                    input.session_id,
                                    input.round,
                                    error
                                );
                            }
                        }
                        EngineEvent::AgentContextItemAppended(update) => {
                            if let Err(error) =
                                session_archive_repo.save_agent_context_item(&update).await
                            {
                                tracing::warn!(
                                    "failed to persist agent context item for {} / {}: {:?}",
                                    update.session_id,
                                    update.agent_name,
                                    error
                                );
                            }
                        }
                        EngineEvent::AgentContextRollback(rollback) => {
                            if let Err(error) = session_archive_repo
                                .save_agent_context_rollback(&rollback)
                                .await
                            {
                                tracing::warn!(
                                    "failed to persist agent context rollback for {} / {}: {:?}",
                                    rollback.session_id,
                                    rollback.agent_name,
                                    error
                                );
                            }
                        }
                        EngineEvent::FlowTurnUpdate(update) => {
                            let persisted_phase = persisted_phase_for_flow_turn_update(&update);
                            if let Err(error) = session_archive_repo
                                .update_session_turn_state(
                                    &update.session_id,
                                    persisted_phase,
                                    if matches!(
                                        persisted_phase,
                                        TurnPhase::AwaitingPlayer | TurnPhase::TurnCompleted
                                    ) {
                                        update.round
                                    } else {
                                        update.round.saturating_sub(1)
                                    },
                                    update.round,
                                )
                                .await
                            {
                                tracing::warn!(
                                    "failed to update session turn state for {}: {:?}",
                                    update.session_id,
                                    error
                                );
                            }
                            if let Err(error) =
                                session_archive_repo.save_flow_turn_update(&update).await
                            {
                                tracing::warn!(
                                    "failed to persist flow turn output for {}: {:?}",
                                    session_id_for_event_task,
                                    error
                                );
                            }
                        }
                        EngineEvent::FlowTurnCompleted(completed) => {
                            if let Err(error) = session_archive_repo
                                .record_flow_turn_completed(&completed.session_id, completed.round)
                                .await
                            {
                                tracing::warn!(
                                    "failed to persist completed flow turn for {} / {}: {:?}",
                                    completed.session_id,
                                    completed.round,
                                    error
                                );
                            }
                            tracing::debug!(
                                session_id = %completed.session_id,
                                round = completed.round,
                                "flow turn completed"
                            );
                        }
                        EngineEvent::FlowTurnEnd(end) => {
                            if let Err(error) = session_archive_repo
                                .record_flow_turn_end(&end.session_id, end.round)
                                .await
                            {
                                tracing::warn!(
                                    "failed to persist ended flow turn for {} / {}: {:?}",
                                    end.session_id,
                                    end.round,
                                    error
                                );
                            }
                            tracing::debug!(
                                session_id = %end.session_id,
                                round = end.round,
                                "flow turn ended"
                            );
                        }
                        EngineEvent::FlowTurnError(error) => {
                            if let Err(persist_error) =
                                session_archive_repo.record_flow_turn_error(&error).await
                            {
                                tracing::warn!(
                                    "failed to persist flow turn error for {} / {}: {:?}",
                                    error.session_id,
                                    error.round,
                                    persist_error
                                );
                            }
                            tracing::warn!(
                                session_id = %error.session_id,
                                round = error.round,
                                stage = ?error.stage,
                                entity_name = %error.entity_name,
                                msg = %error.msg,
                                "flow turn error"
                            );
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        "session event pipeline for {} lagged by {} events",
                        session_id_for_event_task,
                        skipped
                    );
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    HotSession {
        engine,
        events_tx,
        live_events,
    }
}

fn apply_control(
    engine: &AkashicSessionEngine,
    control: GameSessionControlCommand,
) -> Result<ControlGameSessionData, AppError> {
    match control {
        GameSessionControlCommand::Continue => {
            engine.start_next_turn().map_err(AppError::bad_request)?;
            Ok(ControlGameSessionData {
                action: "continue".to_string(),
            })
        }
    }
}

fn persisted_phase_for_flow_turn_update(update: &FlowTurnUpdate) -> TurnPhase {
    if update.stage == TurnPhase::Application
        && update.output_type == AgentOutputType::Json
        && update.entity_name == "Protagonist"
    {
        TurnPhase::AwaitingPlayer
    } else {
        update.stage
    }
}

fn world_state_from_database(
    metadata: StoredSessionMetadata,
    rounds: &[RoundHistoryEntry],
) -> GameSessionWorldStateData {
    let world_snapshot = latest_world_snapshot(rounds).unwrap_or_default();
    let latest_narration = latest_narration_from_rounds(rounds, &world_snapshot);
    let current_outcome = latest_committed_action(rounds).unwrap_or_else(|| {
        if metadata.phase == TurnPhase::Start {
            "start".to_string()
        } else {
            "尚未做出选择".to_string()
        }
    });
    let choices = latest_choices_from_rounds(rounds);

    GameSessionWorldStateData {
        session_id: metadata.session_id,
        generated_profiles: GeneratedProfilesData {
            world: metadata.world_profile,
            protagonist: metadata.protagonist_profile,
            key_story_beats: metadata.key_story_beats,
        },
        status: status_from_phase(metadata.phase).to_string(),
        phase: metadata.phase,
        flow_end: metadata.flow_end,
        turn_index: visible_turn_index_from_parts(
            metadata.phase,
            metadata.turn_index,
            metadata.active_turn_id,
        ),
        active_turn_id: metadata.active_turn_id,
        world_state: WorldStateData::from(world_snapshot),
        latest_narration,
        current_outcome,
        choices,
    }
}

fn archive_payload_from_database(
    metadata: StoredSessionMetadata,
    rounds: Vec<RoundHistoryEntry>,
    contexts: HashMap<String, Context>,
    requested_title: Option<&str>,
) -> archive::SessionArchivePayload {
    let world_snapshot = latest_world_snapshot(&rounds).unwrap_or_default();
    let committed_action = latest_committed_action(&rounds).unwrap_or_else(|| "start".to_string());
    let choices = latest_choices_from_rounds(&rounds);
    let round_for_title = metadata.active_turn_id.max(metadata.turn_index).max(1);
    let title = archive_title_for_round(
        requested_title,
        "",
        &world_snapshot.scene_title,
        round_for_title,
    );

    archive::SessionArchivePayload {
        session_id: metadata.session_id,
        title,
        world_profile: metadata.world_profile,
        protagonist_profile: metadata.protagonist_profile,
        key_story_beats: metadata.key_story_beats,
        turn_state: archive::TurnStateArchive {
            phase: if metadata.flow_end {
                TurnPhase::Ended
            } else {
                metadata.phase
            },
            turn_index: metadata.turn_index,
            active_turn_id: metadata.active_turn_id,
        },
        fate_weaver: contexts.get("FateWeaver").cloned().unwrap_or_default(),
        upper_narrator: contexts.get("UpperNarrator").cloned().unwrap_or_default(),
        protagonist: contexts.get("Protagonist").cloned().unwrap_or_default(),
        world_snapshot,
        protagonist_decision: archive::ProtagonistDecisionArchive {
            committed_action,
            choices,
        },
        history_log: SessionHistoryLog { rounds },
    }
}

fn latest_world_snapshot(rounds: &[RoundHistoryEntry]) -> Option<WorldSnapshot> {
    rounds
        .iter()
        .rev()
        .find_map(|round| round.world_snapshot.clone())
}

fn latest_narration_from_rounds(
    rounds: &[RoundHistoryEntry],
    world_snapshot: &WorldSnapshot,
) -> String {
    rounds
        .iter()
        .rev()
        .filter_map(|round| round.narration_text.as_deref())
        .map(str::trim)
        .find(|text| !text.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| world_snapshot.description.clone())
}

fn latest_choices_from_rounds(rounds: &[RoundHistoryEntry]) -> Vec<PendingProtagonistChoice> {
    rounds
        .iter()
        .rev()
        .find(|round| !round.choices.is_empty())
        .map(|round| round.choices.clone())
        .unwrap_or_default()
}

fn latest_committed_action(rounds: &[RoundHistoryEntry]) -> Option<String> {
    rounds
        .iter()
        .rev()
        .filter_map(|round| round.committed_action.as_deref())
        .map(str::trim)
        .find(|action| !action.is_empty())
        .map(str::to_string)
}

fn rounds_with_story_edge_actions(
    mut rounds: Vec<RoundHistoryEntry>,
    story_edge_actions: Vec<StoredStoryEdgeAction>,
) -> Vec<RoundHistoryEntry> {
    for input in story_edge_actions {
        let action = input.action.trim();
        if action.is_empty() {
            continue;
        }

        let action = match input.action_type {
            PlayerActionType::SelectedOption | PlayerActionType::FreeText => action.to_string(),
        };
        if let Some(round) = rounds.iter_mut().find(|round| round.round == input.round) {
            round.committed_action = Some(action);
        } else {
            rounds.push(RoundHistoryEntry {
                round: input.round,
                committed_action: Some(action),
                ..RoundHistoryEntry::default()
            });
        }
    }
    rounds.sort_by_key(|round| round.round);
    rounds
}

fn stabilize_archive_payload_from_history(
    payload: &mut archive::SessionArchivePayload,
    requested_title: Option<&str>,
) -> Result<(), String> {
    if is_archive_stable_phase(payload.turn_state.phase) {
        return Ok(());
    }

    let completed_round = payload
        .history_log
        .rounds
        .iter()
        .rev()
        .find(|entry| is_completed_dialogue_round(entry))
        .cloned()
        .ok_or_else(|| "当前会话还没有已完成的对话可用于创建存档".to_string())?;
    let completed_round_id = completed_round.round;
    let world_snapshot = completed_round
        .world_snapshot
        .clone()
        .expect("completed dialogue rounds require a world snapshot");
    let committed_action = payload
        .history_log
        .rounds
        .iter()
        .rev()
        .filter(|entry| entry.round < completed_round_id)
        .filter_map(|entry| entry.committed_action.as_deref())
        .find(|action| !action.trim().is_empty())
        .unwrap_or("start")
        .to_string();
    let mut archive_rounds = payload
        .history_log
        .rounds
        .iter()
        .filter(|entry| entry.round <= completed_round_id)
        .cloned()
        .collect::<Vec<_>>();
    if let Some(entry) = archive_rounds
        .iter_mut()
        .find(|entry| entry.round == completed_round_id)
    {
        entry.committed_action = None;
    }

    payload.title = archive_title_for_round(
        requested_title,
        payload.title.as_str(),
        &world_snapshot.scene_title,
        completed_round_id,
    );
    payload.turn_state.phase = TurnPhase::AwaitingPlayer;
    payload.turn_state.turn_index = completed_round_id;
    payload.turn_state.active_turn_id = completed_round_id;
    payload.world_snapshot = world_snapshot;
    payload.protagonist_decision.committed_action = committed_action;
    payload.protagonist_decision.choices = completed_round.choices;
    payload.history_log = SessionHistoryLog {
        rounds: archive_rounds,
    };
    Ok(())
}

fn is_archive_stable_phase(phase: TurnPhase) -> bool {
    matches!(
        phase,
        TurnPhase::Start | TurnPhase::AwaitingPlayer | TurnPhase::TurnCompleted | TurnPhase::Ended
    )
}

fn is_completed_dialogue_round(entry: &RoundHistoryEntry) -> bool {
    entry.world_snapshot.is_some()
        && entry
            .narration_text
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty())
}

fn archive_title_for_round(
    requested_title: Option<&str>,
    existing_title: &str,
    scene_title: &str,
    round: u64,
) -> String {
    let requested = requested_title.unwrap_or_default().trim();
    if !requested.is_empty() {
        return requested.to_string();
    }

    let scene = scene_title.trim();
    if !scene.is_empty() {
        return format!("第{round}轮：{scene}");
    }

    let existing = existing_title.trim();
    if !existing.is_empty() {
        return existing.to_string();
    }

    format!("第{round}轮存档")
}

fn visible_turn_index_from_parts(phase: TurnPhase, turn_index: u64, active_turn_id: u64) -> u64 {
    if phase == TurnPhase::TurnCompleted {
        turn_index.max(1)
    } else {
        active_turn_id.max(turn_index + 1)
    }
}

fn collect_history_narrations(history: &[RoundHistoryEntry]) -> Vec<String> {
    history
        .iter()
        .filter_map(|entry| {
            entry
                .narration_text
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        })
        .collect()
}

fn is_evictable_phase(phase: TurnPhase) -> bool {
    matches!(
        phase,
        TurnPhase::AwaitingPlayer | TurnPhase::TurnCompleted | TurnPhase::Ended
    )
}

fn status_from_phase(phase: TurnPhase) -> &'static str {
    match phase {
        TurnPhase::Start => "pending",
        TurnPhase::AwaitingPlayer => "awaiting_player",
        TurnPhase::TurnCompleted => "waiting_control",
        TurnPhase::Ended => "ended",
        TurnPhase::Failed => "failed",
        _ => "running",
    }
}

fn now_string() -> String {
    Utc::now().to_rfc3339()
}

fn elapsed_since(now: DateTime<Utc>, then: DateTime<Utc>) -> Duration {
    (now - then).to_std().unwrap_or_default()
}

fn record_ttl(record: &SessionRecord, config: &SessionLifecycleConfig) -> Duration {
    config.ttl_for_phase(record.last_phase)
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::archive::{
        ProtagonistDecisionArchive, SessionArchivePayload, TurnStateArchive,
    };
    use agent::agent::context::Context;
    use serde_json::Value;
    use story_engine::components::{
        outcome::{PendingProtagonistChoice, ProtagonistOption},
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
        assert_eq!(restored.current_outcome, "绕到钟楼背面");
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
        let compressed =
            archive::compress_archive_payload(&sample_payload_with_rounds(total_rounds))
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

        stabilize_archive_payload_from_history(&mut payload, None)
            .expect("nonstable payload should use latest completed round");

        assert_eq!(payload.turn_state.phase, TurnPhase::AwaitingPlayer);
        assert_eq!(payload.turn_state.turn_index, 3);
        assert_eq!(payload.turn_state.active_turn_id, 3);
        assert_eq!(payload.world_snapshot.scene_title, "第3轮");
        assert_eq!(payload.protagonist_decision.committed_action, "行动-2");
        assert_eq!(
            payload.protagonist_decision.choices[0].option.action,
            "行动-3"
        );
        assert_eq!(payload.history_log.rounds.len(), 3);
        assert_eq!(payload.history_log.rounds[2].committed_action, None);
        assert_eq!(payload.title, "第3轮：第3轮");
    }

    #[test]
    fn story_edges_overlay_committed_actions() {
        let rounds = vec![RoundHistoryEntry {
            round: 2,
            committed_action: Some("旧行动".to_string()),
            ..RoundHistoryEntry::default()
        }];
        let merged = rounds_with_story_edge_actions(
            rounds,
            vec![
                StoredStoryEdgeAction {
                    round: 1,
                    action_type: PlayerActionType::FreeText,
                    action: "  自定义检查密室暗门  ".to_string(),
                },
                StoredStoryEdgeAction {
                    round: 2,
                    action_type: PlayerActionType::SelectedOption,
                    action: "绕到钟楼背面".to_string(),
                },
            ],
        );

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].round, 1);
        assert_eq!(
            merged[0].committed_action.as_deref(),
            Some("自定义检查密室暗门")
        );
        assert_eq!(merged[1].round, 2);
        assert_eq!(merged[1].committed_action.as_deref(), Some("绕到钟楼背面"));
    }

    #[test]
    fn protagonist_options_mark_persisted_node_as_awaiting_player() {
        let protagonist_update = FlowTurnUpdate {
            session_id: "session-phase".to_string(),
            round: 3,
            stage: TurnPhase::Application,
            entity_name: "Protagonist".to_string(),
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
            persisted_phase_for_flow_turn_update(&protagonist_update),
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
    async fn export_save_archive_returns_local_archive_payload() {
        let state = test_state();

        let created = state
            .create_game_session(crate::api::game_sessions::CreateGameSessionRequest {
                world_profile: "archive world".to_string(),
                protagonist_profile: "archive protagonist".to_string(),
                key_story_beats: "archive beats".to_string(),
            })
            .await
            .expect("session should create");

        let exported = state
            .export_save_archive(&created.session_id, Some("测试存档"))
            .await
            .expect("save archive should export");

        let payload = archive::decompress_archive_payload(&exported.compressed_archive)
            .expect("exported archive should decode");

        assert_eq!(payload.session_id, created.session_id);
        assert_eq!(payload.title, "测试存档");
        assert_eq!(exported.title, "测试存档");
        assert!(!exported.compressed_archive.is_empty());
    }

    #[tokio::test]
    async fn load_game_session_from_archive_overwrites_existing_session() {
        let state = test_state();

        state
            .create_game_session(crate::api::game_sessions::CreateGameSessionRequest {
                world_profile: "old world".to_string(),
                protagonist_profile: "old protagonist".to_string(),
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
        assert_eq!(loaded_again.current_outcome, "绕到钟楼背面");
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
            .clone_game_session("session-from-slot")
            .await
            .expect("stable source session should clone");

        assert_ne!(cloned.session_id, "session-from-slot");
        assert!(cloned.session_id.starts_with("session-"));
        assert_eq!(cloned.world_state.scene_title, "钟楼阴影");
        assert_eq!(cloned.current_outcome, "绕到钟楼背面");

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
            generated_profiles: GeneratedProfilesData {
                world: "world".to_string(),
                protagonist: "protagonist".to_string(),
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

    fn sample_payload() -> SessionArchivePayload {
        SessionArchivePayload {
            session_id: "session-from-slot".to_string(),
            title: "第7轮：钟楼阴影".to_string(),
            world_profile: "world".to_string(),
            protagonist_profile: "protagonist".to_string(),
            key_story_beats: "beats".to_string(),
            turn_state: TurnStateArchive {
                phase: TurnPhase::AwaitingPlayer,
                turn_index: 7,
                active_turn_id: 7,
            },
            fate_weaver: Context::default(),
            upper_narrator: Context::default(),
            protagonist: Context::default(),
            world_snapshot: WorldSnapshot {
                round: 7,
                scene_title: "钟楼阴影".to_string(),
                description: "雾气正在台阶间倒灌。".to_string(),
                ..WorldSnapshot::default()
            },
            protagonist_decision: ProtagonistDecisionArchive {
                committed_action: "绕到钟楼背面".to_string(),
                choices: vec![PendingProtagonistChoice {
                    id: "choice-1".to_string(),
                    option: ProtagonistOption {
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
                    choices: vec![PendingProtagonistChoice {
                        id: "choice-1".to_string(),
                        option: ProtagonistOption {
                            title: "绕行".to_string(),
                            action: "绕到钟楼背面".to_string(),
                            motivation_and_risk: "视野更好，但会暴露脚步声".to_string(),
                        },
                    }],
                    committed_action: Some("绕到钟楼背面".to_string()),
                }],
            },
        }
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
        payload.protagonist_decision.committed_action = format!("行动-{total_rounds}");
        payload.history_log = SessionHistoryLog {
            rounds: (1..=total_rounds)
                .map(|round| RoundHistoryEntry {
                    round,
                    world_snapshot: Some(WorldSnapshot {
                        round,
                        scene_title: format!("第{round}轮"),
                        description: format!("第{round}轮描述"),
                        ..WorldSnapshot::default()
                    }),
                    narration_text: Some(format!("叙事-{round}")),
                    choices: vec![PendingProtagonistChoice {
                        id: format!("choice-{round}"),
                        option: ProtagonistOption {
                            title: format!("选择-{round}"),
                            action: format!("行动-{round}"),
                            motivation_and_risk: "保持测试稳定".to_string(),
                        },
                    }],
                    committed_action: Some(format!("行动-{round}")),
                })
                .collect(),
        };
        payload
    }
}

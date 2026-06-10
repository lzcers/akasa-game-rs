use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Utc};
use story_engine::{
    debug::LocalDebugObserver,
    engine::{AkashicEngine, AkashicSessionEngine, RuntimeDebugObserver, Session},
    profile::DEFAULT_KEY_STORY_BEATS,
    resources::{
        agent_task::TaskUpdate, export::TaskEvent, protagonist_action::PlayerActionType,
        turn_state::TurnPhase,
    },
};
use tokio::sync::{Mutex, broadcast};
use uuid::Uuid;

use crate::{
    analytics::{AnalyticsRepository, AnalyticsSummary},
    api::archive,
    api::game_sessions::{
        ControlGameSessionData, ControlGameSessionRequest, CreateGameSessionData,
        CreateGameSessionRequest, GameSessionControlCommand, GameSessionWorldStateData,
        RoundHistoryData, SaveExportData, SessionActionInput, WorldStateData,
    },
    api::site::{AnalyticsBatchRequest, SubmitFeedbackData, ValidatedFeedbackRequest},
    email::{FeedbackEmail, FeedbackMailer},
    error::AppError,
    session_archive::{SessionArchiveRepository, StoredSessionArchive},
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
    Cooling { _started_at: DateTime<Utc> },
    Cold(ColdSession),
}

struct HotSession {
    engine: AkashicSessionEngine,
    events_tx: broadcast::Sender<LiveTaskUpdate>,
    live_events: Arc<Mutex<LiveEventLog>>,
}

struct ColdSession {
    archive_ref: String,
    _saved_at: DateTime<Utc>,
    _summary: Option<String>,
}

struct HotSessionAccess {
    session_id: String,
    engine: AkashicSessionEngine,
    events_tx: broadcast::Sender<LiveTaskUpdate>,
    live_events: Arc<Mutex<LiveEventLog>>,
}

pub struct LiveSessionStream {
    pub session_id: String,
    pub replayed_events: Vec<LiveTaskUpdate>,
    pub event_rx: broadcast::Receiver<LiveTaskUpdate>,
    pub lease: LiveSessionLease,
}

pub struct LiveSessionLease {
    session_id: String,
    sessions: Arc<Mutex<HashMap<String, SessionRecord>>>,
}

#[derive(Clone, Debug)]
pub struct LiveTaskUpdate {
    pub event_id: u64,
    pub round: u64,
    pub update: TaskUpdate,
}

#[derive(Default)]
struct LiveEventLog {
    next_event_id: u64,
    history: VecDeque<LiveTaskUpdate>,
    capacity: usize,
}

const EVENT_CHANNEL_CAPACITY: usize = 256;
const DEFAULT_EVENT_HISTORY_CAPACITY: usize = 256;
const DEFAULT_SESSION_IDLE_TTL_SECS: u64 = 30 * 60;
const DEFAULT_SESSION_ENDED_TTL_SECS: u64 = 5 * 60;
const DEFAULT_MAX_HOT_SESSIONS: usize = 200;
const DEFAULT_SESSION_REAPER_INTERVAL_SECS: u64 = 60;

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
    pub fn new(
        analytics_events_path: impl Into<PathBuf>,
        session_archives_path: impl Into<PathBuf>,
        local_debug: bool,
    ) -> Self {
        let state = Self::with_lifecycle_config(
            analytics_events_path,
            session_archives_path,
            local_debug,
            SessionLifecycleConfig::default(),
            true,
        );
        state
    }

    fn with_lifecycle_config(
        analytics_events_path: impl Into<PathBuf>,
        session_archives_path: impl Into<PathBuf>,
        local_debug: bool,
        lifecycle_config: SessionLifecycleConfig,
        spawn_reaper: bool,
    ) -> Self {
        let state = Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            restore_lock: Arc::new(Mutex::new(())),
            analytics_repo: AnalyticsRepository::new(analytics_events_path),
            session_archive_repo: SessionArchiveRepository::new(session_archives_path),
            feedback_mailer: FeedbackMailer::from_env(),
            engine: AkashicEngine::new_with_debug_observer(local_debug.then(|| {
                Arc::new(LocalDebugObserver::for_workspace_root()) as Arc<dyn RuntimeDebugObserver>
            })),
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
            .append_events(&request.events)
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
        let session = build_session_record(
            session_id.clone(),
            engine,
            self.lifecycle_config.live_event_history_capacity,
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
        let engine = archive::load_archive_payload(&self.engine, payload)
            .await
            .map_err(AppError::bad_request)?;
        engine
            .wait_until_ready()
            .await
            .map_err(AppError::internal)?;
        let session = build_session_record(
            session_id.clone(),
            engine,
            self.lifecycle_config.live_event_history_capacity,
        );
        let snapshot = hot_session(&session)
            .expect("newly built session must be hot")
            .engine
            .get_game_session();
        let state_view = world_state_from_session(&session.session_id, &snapshot);

        self.sessions.lock().await.insert(session_id, session);
        self.persist_archive_payload(&payload_for_storage).await?;
        self.reap_idle_sessions_once().await?;

        Ok(state_view)
    }

    pub async fn get_game_session_world(
        &self,
        session_id: &str,
    ) -> Result<GameSessionWorldStateData, AppError> {
        let hot = self.ensure_hot_session(session_id, false).await?;
        let snapshot = hot.engine.get_game_session();
        self.update_session_phase(session_id, snapshot.phase).await;

        Ok(world_state_from_session(&hot.session_id, &snapshot))
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

        let engine = archive::load_archive_payload(&self.engine, payload)
            .await
            .map_err(AppError::bad_request)?;
        engine
            .wait_until_ready()
            .await
            .map_err(AppError::internal)?;
        let session = build_session_record(
            clone_session_id.clone(),
            engine,
            self.lifecycle_config.live_event_history_capacity,
        );
        let snapshot = hot_session(&session)
            .expect("newly built session must be hot")
            .engine
            .get_game_session();
        let state_view = world_state_from_session(&session.session_id, &snapshot);

        self.sessions.lock().await.insert(clone_session_id, session);
        self.persist_archive_payload(&payload_for_storage).await?;
        self.reap_idle_sessions_once().await?;

        Ok(state_view)
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
            (None, Some(action)) => apply_action(&hot.engine, action),
            (None, None) => Err(AppError::bad_request(
                "请求体至少需要提供 `control` 或 `action` 之一。",
            )),
            (Some(_), Some(_)) => Err(AppError::bad_request(
                "同一次请求只能执行一种操作：控制命令或玩家行动。",
            )),
        }?;
        let snapshot = hot.engine.get_game_session();
        self.update_session_phase(session_id, snapshot.phase).await;
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

    async fn ensure_hot_session(
        &self,
        session_id: &str,
        register_stream: bool,
    ) -> Result<HotSessionAccess, AppError> {
        loop {
            let archive_ref = {
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
                    SessionSlot::Cold(cold) => Some(cold.archive_ref.clone()),
                }
            };

            let Some(archive_ref) = archive_ref else {
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

            let stored = self
                .session_archive_repo
                .load_archive(&archive_ref)
                .await
                .map_err(|err| {
                    AppError::internal(format!("读取冷会话 `{session_id}` 存档失败：{err:#}"))
                })?
                .ok_or_else(|| {
                    AppError::not_found(format!("未找到会话 `{session_id}` 的冷存档"))
                })?;
            let payload = archive::decompress_archive_payload(&stored.compressed_archive)
                .map_err(AppError::internal)?;
            archive::validate_archive_payload(&payload).map_err(AppError::internal)?;
            let engine = archive::load_archive_payload(&self.engine, payload)
                .await
                .map_err(AppError::internal)?;
            engine
                .wait_until_ready()
                .await
                .map_err(AppError::internal)?;
            let snapshot = engine.get_game_session();
            let hot = build_hot_session(engine, self.lifecycle_config.live_event_history_capacity);

            let mut sessions = self.sessions.lock().await;
            let record = sessions
                .get_mut(session_id)
                .ok_or_else(|| AppError::not_found(format!("未找到会话 `{session_id}`")))?;
            record.touch(Utc::now());
            record.last_phase = snapshot.phase;
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
        loop {
            let hot_engine_or_archive_ref = {
                let mut sessions = self.sessions.lock().await;
                let record = sessions
                    .get_mut(session_id)
                    .ok_or_else(|| AppError::not_found(format!("未找到会话 `{session_id}`")))?;
                record.touch(Utc::now());
                match &record.slot {
                    SessionSlot::Hot(hot) => Ok(hot.engine.clone()),
                    SessionSlot::Cooling { .. } => Err(None),
                    SessionSlot::Cold(cold) => Err(Some(cold.archive_ref.clone())),
                }
            };

            match hot_engine_or_archive_ref {
                Ok(engine) => {
                    return archive::gen_archive_payload(session_id, title, &engine)
                        .await
                        .map_err(AppError::bad_request);
                }
                Err(None) => {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(Some(archive_ref)) => {
                    let stored = self
                        .session_archive_repo
                        .load_archive(&archive_ref)
                        .await
                        .map_err(|err| {
                            AppError::internal(format!(
                                "读取冷会话 `{session_id}` 存档失败：{err:#}"
                            ))
                        })?
                        .ok_or_else(|| {
                            AppError::not_found(format!("未找到会话 `{session_id}` 的冷存档"))
                        })?;
                    let mut payload =
                        archive::decompress_archive_payload(&stored.compressed_archive)
                            .map_err(AppError::internal)?;
                    if let Some(title) = title.map(str::trim).filter(|title| !title.is_empty()) {
                        payload.title = title.to_string();
                    }
                    return Ok(payload);
                }
            }
        }
    }

    async fn persist_archive_payload(
        &self,
        payload: &archive::SessionArchivePayload,
    ) -> Result<(), AppError> {
        let compressed_archive =
            archive::compress_archive_payload(payload).map_err(AppError::internal)?;
        let now = now_string();
        self.session_archive_repo
            .save_archive(StoredSessionArchive {
                session_id: payload.session_id.clone(),
                compressed_archive,
                title: Some(payload.title.clone()),
                phase: status_from_phase(payload.turn_state.phase).to_string(),
                created_at: now.clone(),
                updated_at: now.clone(),
                last_accessed_at: now,
            })
            .await
            .map_err(|err| AppError::internal(format!("写入会话存档失败：{err:#}")))
    }

    async fn update_session_phase(&self, session_id: &str, phase: TurnPhase) {
        let mut sessions = self.sessions.lock().await;
        if let Some(record) = sessions.get_mut(session_id) {
            record.last_phase = phase;
        }
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

        let payload = match archive::gen_archive_payload(session_id, None, &hot.engine).await {
            Ok(payload) => payload,
            Err(error) => {
                self.restore_failed_cooling_session(session_id, hot).await;
                return Err(AppError::bad_request(error));
            }
        };
        if archive::validate_archive_payload(&payload).is_err() {
            self.restore_failed_cooling_session(session_id, hot).await;
            return Ok(false);
        }
        if let Err(error) = self.persist_archive_payload(&payload).await {
            self.restore_failed_cooling_session(session_id, hot).await;
            return Err(error);
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
        record.slot = SessionSlot::Cold(ColdSession {
            archive_ref: session_id.to_string(),
            _saved_at: Utc::now(),
            _summary: Some(payload.title),
        });
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

    fn push(&mut self, round: u64, update: TaskUpdate) -> LiveTaskUpdate {
        self.next_event_id += 1;
        let event = LiveTaskUpdate {
            event_id: self.next_event_id,
            round,
            update,
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
) -> SessionRecord {
    let snapshot = engine.get_game_session();
    let now = Utc::now();
    let hot = build_hot_session(engine, live_event_history_capacity);

    SessionRecord {
        session_id,
        _created_at: now,
        last_accessed_at: now,
        active_streams: 0,
        last_phase: snapshot.phase,
        slot: SessionSlot::Hot(hot),
    }
}

fn build_hot_session(
    engine: AkashicSessionEngine,
    live_event_history_capacity: usize,
) -> HotSession {
    let mut event_rx = engine.subscribe_events();
    let (events_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let live_events = Arc::new(Mutex::new(LiveEventLog::with_capacity(
        live_event_history_capacity,
    )));
    let events_tx_for_task = events_tx.clone();
    let live_events_for_task = Arc::clone(&live_events);
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            let TaskEvent::TaskUpdated { round, update } = event;
            let event = {
                let mut live_events = live_events_for_task.lock().await;
                live_events.push(round, update)
            };
            let _ = events_tx_for_task.send(event);
        }
    });

    HotSession {
        engine,
        events_tx,
        live_events,
    }
}

fn hot_session(record: &SessionRecord) -> Option<&HotSession> {
    match &record.slot {
        SessionSlot::Hot(hot) => Some(hot),
        SessionSlot::Cooling { .. } | SessionSlot::Cold(_) => None,
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

fn apply_action(
    engine: &AkashicSessionEngine,
    action: SessionActionInput,
) -> Result<ControlGameSessionData, AppError> {
    let selected_action = action.action.trim();
    if selected_action.is_empty() {
        return Err(AppError::bad_request("提交行动不能为空。"));
    }

    if action.r#type == PlayerActionType::SelectedOption {
        let snapshot = engine.get_game_session();
        let is_valid_option = snapshot
            .choices
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

fn world_state_from_session(session_id: &str, snapshot: &Session) -> GameSessionWorldStateData {
    GameSessionWorldStateData {
        session_id: session_id.to_string(),
        status: status_from_phase(snapshot.phase).to_string(),
        phase: snapshot.phase,
        turn_index: visible_turn_index(snapshot),
        active_turn_id: snapshot.active_turn_id,
        world_state: WorldStateData::from(snapshot.world_snapshot.clone()),
        history: snapshot
            .history
            .clone()
            .into_iter()
            .map(RoundHistoryData::from)
            .collect(),
        current_task: snapshot.current_task.clone(),
        tasks: snapshot.tasks.clone(),
        latest_narration: latest_narration(snapshot),
        current_protagonist_action: current_protagonist_action(snapshot),
        choices: snapshot.choices.clone(),
    }
}

fn latest_narration(snapshot: &Session) -> String {
    if snapshot.latest_narration.trim().is_empty() {
        snapshot.world_snapshot.description.clone()
    } else {
        snapshot.latest_narration.clone()
    }
}

fn current_protagonist_action(snapshot: &Session) -> String {
    if snapshot.current_protagonist_action.trim().is_empty() {
        "尚未做出选择".to_string()
    } else {
        snapshot.current_protagonist_action.clone()
    }
}

fn visible_turn_index(snapshot: &Session) -> u64 {
    if snapshot.phase == TurnPhase::TurnCompleted {
        snapshot.turn_index.max(1)
    } else {
        snapshot.active_turn_id.max(snapshot.turn_index + 1)
    }
}

#[cfg(test)]
fn collect_story_narrations(snapshot: &Session) -> Vec<String> {
    let mut narrations: Vec<String> = snapshot
        .history
        .iter()
        .filter_map(|entry| {
            entry
                .narration_text
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        })
        .collect();

    let latest = snapshot.latest_narration.trim();
    if is_stable_phase(snapshot.phase)
        && !latest.is_empty()
        && narrations.last().is_none_or(|item| item != latest)
    {
        narrations.push(latest.to_string());
    }

    narrations
}

fn collect_history_narrations(
    history: &[story_engine::resources::history::RoundHistoryEntry],
) -> Vec<String> {
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

fn is_stable_phase(phase: TurnPhase) -> bool {
    matches!(
        phase,
        TurnPhase::Idle
            | TurnPhase::AwaitingPlayer
            | TurnPhase::TurnCompleted
            | TurnPhase::Ended
            | TurnPhase::Failed
    )
}

fn is_evictable_phase(phase: TurnPhase) -> bool {
    matches!(
        phase,
        TurnPhase::AwaitingPlayer | TurnPhase::TurnCompleted | TurnPhase::Ended
    )
}

fn status_from_phase(phase: TurnPhase) -> &'static str {
    match phase {
        TurnPhase::Idle => "pending",
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
    use story_engine::resources::{
        history::{RoundHistoryEntry, SessionHistoryLog},
        protagonist_action::{PendingProtagonistChoice, ProtagonistOption},
        turn_state::TurnPhase,
        world_snapshot::WorldSnapshot,
    };

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
            std::env::temp_dir().join(format!(
                "akasa-analytics-{}.sqlite3",
                Uuid::new_v4().simple()
            )),
            std::env::temp_dir().join(format!(
                "akasa-session-archives-{}.sqlite3",
                Uuid::new_v4().simple()
            )),
            false,
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
        assert_eq!(restored.current_protagonist_action, "绕到钟楼背面");
        assert_eq!(restored.choices.len(), 1);
        assert_eq!(restored.history.len(), 1);
        assert_eq!(restored.history[0].narration_text, "钟声掠过雾墙。");
        assert_eq!(
            restored.history[0].selected_choice_text.as_deref(),
            Some("绕行")
        );

        let loaded_again = state
            .get_game_session_world("session-from-slot")
            .await
            .expect("restored session should be queryable");
        assert_eq!(loaded_again.world_state.scene_title, "钟楼阴影");
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
            assert!(matches!(record.slot, SessionSlot::Cold(_)));
        }

        let restored = state
            .get_game_session_world("session-from-slot")
            .await
            .expect("cold session should restore on demand");
        assert_eq!(restored.world_state.scene_title, "钟楼阴影");

        let sessions = state.sessions.lock().await;
        let record = sessions
            .get("session-from-slot")
            .expect("session should remain registered");
        assert!(matches!(record.slot, SessionSlot::Hot(_)));
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
        assert_eq!(loaded_again.current_protagonist_action, "绕到钟楼背面");
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
        assert_eq!(cloned.current_protagonist_action, "绕到钟楼背面");

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
        assert_eq!(source.history.len(), cloned_again.history.len());
    }

    #[test]
    fn game_session_world_state_serializes_world_state_as_camel_case() {
        let dto = GameSessionWorldStateData {
            session_id: "session-test".to_string(),
            status: "awaiting_player".to_string(),
            phase: TurnPhase::AwaitingPlayer,
            turn_index: 2,
            active_turn_id: 2,
            world_state: WorldStateData::from(WorldSnapshot {
                round: 2,
                scene_title: "螺旋楼梯的暗影".to_string(),
                time_absolute: "第一日 深夜十一点四十二分".to_string(),
                location_name: "齿轮教堂地下二层".to_string(),
                new_info: vec!["图纸碎片已安全到手".to_string()],
                ..WorldSnapshot::default()
            }),
            history: vec![],
            current_task: None,
            tasks: vec![],
            latest_narration: "narration".to_string(),
            current_protagonist_action: "action".to_string(),
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
    }

    #[test]
    fn collect_story_narrations_uses_all_history_and_current_latest_output() {
        let snapshot = sample_session_from_archive();

        let narrations = collect_story_narrations(&snapshot);

        assert_eq!(
            narrations,
            vec!["钟声掠过雾墙。", "回廊尽头亮起一盏迟到的灯。"]
        );
    }

    #[test]
    fn collect_story_narrations_skips_empty_and_duplicate_latest_output() {
        let mut snapshot = sample_session_from_archive();
        snapshot.history.push(RoundHistoryEntry {
            round: 8,
            world_snapshot: None,
            narration_text: Some("回廊尽头亮起一盏迟到的灯。".to_string()),
            choices: Vec::new(),
            committed_action: None,
        });
        snapshot.latest_narration = "回廊尽头亮起一盏迟到的灯。".to_string();
        snapshot.history.push(RoundHistoryEntry {
            round: 9,
            world_snapshot: None,
            narration_text: Some("   ".to_string()),
            choices: Vec::new(),
            committed_action: None,
        });

        let narrations = collect_story_narrations(&snapshot);

        assert_eq!(
            narrations,
            vec!["钟声掠过雾墙。", "回廊尽头亮起一盏迟到的灯。"]
        );
    }

    #[test]
    fn collect_story_narrations_ignores_live_latest_output_in_non_stable_phase() {
        let mut snapshot = sample_session_from_archive();
        snapshot.phase = TurnPhase::Simulation;
        snapshot.latest_narration = "这一段还在生成，不能进入分享摘要。".to_string();

        let narrations = collect_story_narrations(&snapshot);

        assert_eq!(narrations, vec!["钟声掠过雾墙。"]);
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
            simulators: vec![],
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

    fn sample_session_from_archive() -> Session {
        let payload = sample_payload();

        Session {
            phase: payload.turn_state.phase,
            turn_index: payload.turn_state.turn_index,
            active_turn_id: payload.turn_state.active_turn_id + 1,
            history: payload.history_log.rounds,
            current_task: None,
            tasks: Vec::new(),
            world_snapshot: payload.world_snapshot,
            latest_narration: "回廊尽头亮起一盏迟到的灯。".to_string(),
            current_protagonist_action: payload.protagonist_decision.committed_action,
            choices: payload.protagonist_decision.choices,
        }
    }
}

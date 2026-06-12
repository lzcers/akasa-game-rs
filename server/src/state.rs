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
        outcome::{PendingCharacterChoice, PlayerActionItem, PlayerActionType},
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
        BacktrackGameSessionData, BacktrackGameSessionRequest, BranchExplorationData,
        ChoiceExplorationData, ChoiceExplorationsData, ControlGameSessionData,
        ControlGameSessionRequest, CreateGameSessionData, CreateGameSessionRequest,
        GameSessionControlCommand, GameSessionWorldStateData, GeneratedProfilesData,
        RoundHistoryData, SaveExportData, SessionActionInput, SessionRoundsPageData,
        WorldStateData,
    },
    api::site::{AnalyticsBatchRequest, SubmitFeedbackData, ValidatedFeedbackRequest},
    database::AppDatabase,
    email::{FeedbackEmail, FeedbackMailer},
    error::AppError,
    session_archive::{
        SessionArchiveRepository, StoredBranchExploration, StoredChoiceExploration,
        StoredSessionMetadata, StoredStoryEdgeAction,
    },
    session_history::{RoundHistoryEntry, SessionHistoryLog, TurnPhase},
};

#[derive(Clone)]
pub struct AppState {
    // `engine` 是创建/恢复运行时会话的入口；`sessions` 只保存内存中的
    // 热/冷状态，持久化真相以 `session_archive_repo` 为准。
    engine: AkashicEngine,
    sessions: Arc<Mutex<HashMap<String, SessionRecord>>>,
    session_archive_repo: SessionArchiveRepository,
    // 冷会话恢复会读库并重建 engine，不能长时间占用 `sessions` 锁；
    // 这把锁只用来串行化 Cold -> Hot 的恢复过程。
    restore_lock: Arc<Mutex<()>>,
    analytics_repo: AnalyticsRepository,
    // 从环境变量/.env 配置的反馈邮件发送器；未配置时提交反馈会直接失败。
    feedback_mailer: FeedbackMailer,
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
    // engine 仍在内存中，可以接受控制命令并推送实时事件。
    Hot(HotSession),
    // reaper 正在给热会话做快照并关闭 engine。读取方会短暂等待，
    // 避免撞上半关闭状态。
    Cooling {
        _started_at: DateTime<Utc>,
    },
    // runtime 已被驱逐；世界状态/历史仍可从数据库读取，控制或流式请求
    // 会通过 `ensure_hot_session` 恢复成 Hot。
    Cold {
        _saved_at: DateTime<Utc>,
        _summary: Option<String>,
    },
}

struct HotSession {
    engine: AkashicSessionEngine,
    events_tx: broadcast::Sender<LiveEngineEvent>,
    live_events: Arc<Mutex<LiveEventLog>>,
    submitted_action_rounds: Arc<Mutex<HashSet<u64>>>,
    output_node_targets: Arc<Mutex<HashMap<u64, String>>>,
}

struct HotSessionAccess {
    session_id: String,
    engine: AkashicSessionEngine,
    events_tx: broadcast::Sender<LiveEngineEvent>,
    live_events: Arc<Mutex<LiveEventLog>>,
    submitted_action_rounds: Arc<Mutex<HashSet<u64>>>,
    output_node_targets: Arc<Mutex<HashMap<u64, String>>>,
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
        let character_profile = request.character_profile.trim();
        let key_story_beats = request.key_story_beats.trim();
        let character_name = request.character_name.trim();
        let character_name = if character_name.is_empty() {
            "玩家角色"
        } else {
            character_name
        };
        if world_profile.is_empty() || character_profile.is_empty() {
            return Err(AppError::bad_request(
                "`worldProfile` 与 `characterProfile` 不能为空。",
            ));
        }
        let engine = self
            .engine
            .create_session(
                &session_id,
                character_name,
                world_profile,
                character_profile,
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
                character_name: character_name.to_string(),
                world_profile: world_profile.to_string(),
                character_profile: character_profile.to_string(),
                key_story_beats: key_story_beats.to_string(),
            })
            .await
            .map_err(|err| AppError::internal(format!("写入会话元数据失败：{err:#}")))?;
        let fate_weaver =
            StoryAgent::new_fate_weaver(world_profile, character_profile, key_story_beats);
        let upper_narrator = StoryAgent::new_upper_narrator(world_profile, character_profile);
        let character_agent =
            StoryAgent::new_character_agent(character_name, world_profile, character_profile);
        self.session_archive_repo
            .replace_entity_contexts_from_contexts(
                &session_id,
                0,
                &[
                    ("FateWeaver", &fate_weaver.context),
                    ("UpperNarrator", &upper_narrator.context),
                    (character_name, &character_agent.context),
                ],
            )
            .await
            .map_err(|err| AppError::internal(format!("写入初始 entity context 失败：{err:#}")))?;
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
        let archive = self
            .archive_payload_for_session(session_id, title, None)
            .await?;
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
        // 导入存档会先替换该 session 的全部持久化状态，避免旧的未来轮次
        // 或旧 context 残留到新 runtime。
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
        let page_rounds = page
            .rounds
            .iter()
            .map(|entry| entry.round)
            .collect::<HashSet<_>>();
        let story_edge_actions = self
            .session_archive_repo
            .load_story_edge_actions(session_id)
            .await
            .map_err(|err| AppError::internal(format!("读取故事边行动失败：{err:#}")))?
            .into_iter()
            .filter(|action| page_rounds.contains(&action.round))
            .collect::<Vec<_>>();
        let choice_explorations = self
            .session_archive_repo
            .load_choice_explorations(session_id)
            .await
            .map_err(|err| AppError::internal(format!("读取选项探索状态失败：{err:#}")))?
            .into_iter()
            .filter(|exploration| page_rounds.contains(&exploration.round))
            .collect::<Vec<_>>();
        let branch_explorations = self
            .session_archive_repo
            .load_branch_explorations(session_id)
            .await
            .map_err(|err| AppError::internal(format!("读取分支探索状态失败：{err:#}")))?
            .into_iter()
            .filter(|exploration| page_rounds.contains(&exploration.round))
            .collect::<Vec<_>>();
        let rounds = rounds_with_story_edge_actions(page.rounds, story_edge_actions);

        Ok(SessionRoundsPageData {
            session_id: session_id.to_string(),
            rounds: rounds
                .into_iter()
                .map(|round| {
                    round_history_data_from_entry(round, &choice_explorations, &branch_explorations)
                })
                .collect(),
            next_before_round: page.next_before_round,
            has_more: page.has_more,
        })
    }

    pub async fn clone_game_session(
        &self,
        source_session_id: &str,
        source_round: Option<u64>,
    ) -> Result<GameSessionWorldStateData, AppError> {
        let source_session_id = source_session_id.trim();
        if source_session_id.is_empty() {
            return Err(AppError::bad_request("`sessionId` 不能为空。"));
        }

        let clone_session_id = format!("session-{}", Uuid::new_v4().simple());
        let mut payload = self
            .archive_payload_for_session(source_session_id, None, source_round)
            .await?;
        // 克隆会话本质是改写 archive 身份，再用改写后的 payload 同时重建
        // 持久化状态和新的热 runtime。
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

    pub async fn backtrack_game_session(
        &self,
        session_id: &str,
        request: BacktrackGameSessionRequest,
    ) -> Result<BacktrackGameSessionData, AppError> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err(AppError::bad_request("`sessionId` 不能为空。"));
        }
        let source_round = request.source_round;
        if source_round == 0 {
            return Err(AppError::bad_request("回溯章节必须大于 0。"));
        }

        let source_payload = self
            .archive_payload_for_session(session_id, None, Some(source_round))
            .await?;
        let actions = normalized_action_items(request.action)?;
        validate_selected_actions_against_choices(
            &actions,
            &source_payload.character_decision.choices,
        )?;

        self.ensure_hot_session(session_id, false).await?;
        let branch = self
            .session_archive_repo
            .prepare_backtrack_branch(session_id, source_round, &actions)
            .await
            .map_err(|err| AppError::internal(format!("创建回溯分支失败：{err:#}")))?;

        if !branch.requires_generation {
            let active_payload = self
                .archive_payload_for_session(session_id, None, None)
                .await?;
            self.replace_hot_session_from_payload(session_id, active_payload)
                .await?;
        } else {
            self.replace_hot_session_from_payload(session_id, source_payload)
                .await?;
            let hot = self.ensure_hot_session(session_id, false).await?;
            hot.output_node_targets
                .lock()
                .await
                .insert(branch.branch_round, branch.branch_node_id.clone());
            if let Err(error) = submit_prepared_action(&hot, source_round, actions).await {
                hot.output_node_targets
                    .lock()
                    .await
                    .remove(&branch.branch_round);
                return Err(error);
            }
        }

        let session = self.game_session_world_from_database(session_id).await?;
        Ok(BacktrackGameSessionData {
            session,
            source_round: branch.source_round,
            branch_round: branch.branch_round,
            reused_existing_branch: branch.reused_existing_branch,
        })
    }

    pub async fn get_game_session_narrations(
        &self,
        session_id: &str,
    ) -> Result<Vec<String>, AppError> {
        let payload = self
            .archive_payload_for_session(session_id, None, None)
            .await?;
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
                self.apply_action_from_database(&hot, session_id, action, request.expected_round)
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
        // 先回放内存中的短历史，再订阅新事件；短暂断线时无需重新拉全量历史。
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
        hot: &HotSessionAccess,
        session_id: &str,
        action: SessionActionInput,
        expected_round: Option<u64>,
    ) -> Result<ControlGameSessionData, AppError> {
        let expected_round = expected_round
            .filter(|round| *round > 0)
            .ok_or_else(|| AppError::bad_request("提交行动必须提供 expectedRound。"))?;
        let actions = normalized_action_items(action)?;

        if actions
            .iter()
            .any(|action| action.action_type == PlayerActionType::SelectedOption)
        {
            let choices = self.latest_choices_for_session(session_id).await?;
            validate_selected_actions_against_choices(&actions, &choices)?;
        }

        if self
            .session_archive_repo
            .has_story_edge_action_for_round(session_id, expected_round)
            .await
            .map_err(|err| AppError::internal(format!("检查重复行动失败：{err:#}")))?
        {
            return Err(AppError::bad_request("这一轮行动已经提交过。"));
        }

        submit_prepared_action(hot, expected_round, actions).await?;

        Ok(ControlGameSessionData {
            action: "submit_action".to_string(),
        })
    }

    async fn latest_choices_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<PendingCharacterChoice>, AppError> {
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
        enum HotSessionLookup {
            Cold(String),
            Cooling,
            Missing,
        }

        loop {
            // 先在 registry 锁内快速判定状态；真正的恢复会 await 读库和建
            // engine，所以必须在释放 `sessions` 锁之后做。
            let lookup = {
                let mut sessions = self.sessions.lock().await;
                match sessions.get_mut(session_id) {
                    Some(record) => {
                        record.touch(Utc::now());
                        match &record.slot {
                            SessionSlot::Hot(hot) => {
                                let engine = hot.engine.clone();
                                let events_tx = hot.events_tx.clone();
                                let live_events = Arc::clone(&hot.live_events);
                                let submitted_action_rounds =
                                    Arc::clone(&hot.submitted_action_rounds);
                                let output_node_targets = Arc::clone(&hot.output_node_targets);
                                if register_stream {
                                    record.active_streams += 1;
                                }
                                return Ok(HotSessionAccess {
                                    session_id: record.session_id.clone(),
                                    engine,
                                    events_tx,
                                    live_events,
                                    submitted_action_rounds,
                                    output_node_targets,
                                });
                            }
                            SessionSlot::Cooling { .. } => HotSessionLookup::Cooling,
                            SessionSlot::Cold { .. } => {
                                HotSessionLookup::Cold(record.session_id.clone())
                            }
                        }
                    }
                    None => HotSessionLookup::Missing,
                }
            };

            let cold_session_id = match lookup {
                HotSessionLookup::Cold(session_id) => session_id,
                HotSessionLookup::Cooling => {
                    // reaper 正在 Cooling。等它回到 Hot（冷却失败）或落到 Cold
                    //（冷却成功）后再继续。
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    continue;
                }
                HotSessionLookup::Missing => {
                    if self.register_cold_session_from_database(session_id).await? {
                        continue;
                    }
                    return Err(AppError::not_found(format!("未找到会话 `{session_id}`")));
                }
            };

            let _restore_guard = self.restore_lock.lock().await;
            // 等恢复锁期间，可能已有别的请求把同一个会话恢复好了；这里做
            // 二次检查，避免重复构造 engine 或重复增加 stream 计数。
            if let Some(access) = self
                .hot_session_access_if_available(session_id, register_stream)
                .await?
            {
                return Ok(access);
            }

            let payload = self
                .archive_payload_for_session(&cold_session_id, None, None)
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
                    submitted_action_rounds: Arc::clone(&hot.submitted_action_rounds),
                    output_node_targets: Arc::clone(&hot.output_node_targets),
                });
            }
        }
    }

    async fn replace_hot_session_from_payload(
        &self,
        session_id: &str,
        payload: archive::SessionArchivePayload,
    ) -> Result<(), AppError> {
        archive::validate_archive_payload(&payload).map_err(AppError::internal)?;
        let restored_phase = payload.turn_state.phase;
        let engine = archive::load_archive_payload(&self.engine, payload)
            .await
            .map_err(AppError::internal)?;

        let mut sessions = self.sessions.lock().await;
        let record = sessions
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found(format!("未找到会话 `{session_id}`")))?;
        record.touch(Utc::now());
        record.last_phase = restored_phase;

        match &mut record.slot {
            SessionSlot::Hot(hot) => {
                let events_tx = hot.events_tx.clone();
                let live_events = Arc::clone(&hot.live_events);
                let submitted_action_rounds = Arc::new(Mutex::new(HashSet::new()));
                let output_node_targets = Arc::new(Mutex::new(HashMap::new()));
                spawn_engine_event_bridge(
                    session_id,
                    &engine,
                    events_tx.clone(),
                    Arc::clone(&live_events),
                    self.session_archive_repo.clone(),
                    Arc::clone(&output_node_targets),
                );
                *hot = HotSession {
                    engine,
                    events_tx,
                    live_events,
                    submitted_action_rounds,
                    output_node_targets,
                };
            }
            SessionSlot::Cooling { .. } | SessionSlot::Cold { .. } => {
                record.slot = SessionSlot::Hot(build_hot_session(
                    session_id,
                    engine,
                    self.lifecycle_config.live_event_history_capacity,
                    self.session_archive_repo.clone(),
                    restored_phase,
                ));
            }
        }

        Ok(())
    }

    async fn register_cold_session_from_database(
        &self,
        session_id: &str,
    ) -> Result<bool, AppError> {
        let Some(metadata) = self
            .session_archive_repo
            .load_session_metadata(session_id)
            .await
            .map_err(|err| AppError::internal(format!("读取会话元数据失败：{err:#}")))?
        else {
            return Ok(false);
        };

        let now = Utc::now();
        let mut sessions = self.sessions.lock().await;
        sessions
            .entry(metadata.session_id.clone())
            .or_insert(SessionRecord {
                session_id: metadata.session_id,
                _created_at: now,
                last_accessed_at: now,
                active_streams: 0,
                last_phase: metadata.phase,
                slot: SessionSlot::Cold {
                    _saved_at: now,
                    _summary: None,
                },
            });
        Ok(true)
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
        let submitted_action_rounds = Arc::clone(&hot.submitted_action_rounds);
        let output_node_targets = Arc::clone(&hot.output_node_targets);
        if register_stream {
            record.active_streams += 1;
        }
        Ok(Some(HotSessionAccess {
            session_id: record.session_id.clone(),
            engine,
            events_tx,
            live_events,
            submitted_action_rounds,
            output_node_targets,
        }))
    }

    async fn archive_payload_for_session(
        &self,
        session_id: &str,
        title: Option<&str>,
        completed_round: Option<u64>,
    ) -> Result<archive::SessionArchivePayload, AppError> {
        self.touch_session(session_id).await?;
        let mut payload = self
            .archive_payload_from_database(session_id, title, completed_round)
            .await?;
        // 数据库可能记录了生成中的中间阶段；导出/克隆需要稳定可玩的状态，
        // 所以从完整历史里回退到最近完成的轮次。
        stabilize_archive_payload_from_history(&mut payload, title, completed_round)
            .map_err(AppError::bad_request)?;
        Ok(payload)
    }

    async fn persist_payload_database_state(
        &self,
        payload: &archive::SessionArchivePayload,
    ) -> Result<(), AppError> {
        self.session_archive_repo
            .clear_session_state(&payload.session_id)
            .await
            .map_err(|err| AppError::internal(format!("清理旧会话归档状态失败：{err:#}")))?;
        self.session_archive_repo
            .save_session_metadata(&StoredSessionMetadata {
                session_id: payload.session_id.clone(),
                character_name: payload.character_name.clone(),
                world_profile: payload.world_profile.clone(),
                character_profile: payload.character_profile.clone(),
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
            .replace_entity_contexts_from_contexts(
                &payload.session_id,
                payload
                    .turn_state
                    .active_turn_id
                    .max(payload.turn_state.turn_index),
                &[
                    ("FateWeaver", &payload.fate_weaver),
                    ("UpperNarrator", &payload.upper_narrator),
                    (payload.character_name.as_str(), &payload.character_agent),
                ],
            )
            .await
            .map_err(|err| AppError::internal(format!("写入 entity context 失败：{err:#}")))?;
        Ok(())
    }

    async fn game_session_world_from_database(
        &self,
        session_id: &str,
    ) -> Result<GameSessionWorldStateData, AppError> {
        let metadata = self.load_session_metadata(session_id).await?;
        let rounds = self.load_session_rounds(session_id).await?;
        let choice_explorations = self
            .session_archive_repo
            .load_choice_explorations(session_id)
            .await
            .map_err(|err| AppError::internal(format!("读取选项探索状态失败：{err:#}")))?;
        let branch_explorations = self
            .session_archive_repo
            .load_branch_explorations(session_id)
            .await
            .map_err(|err| AppError::internal(format!("读取分支探索状态失败：{err:#}")))?;
        Ok(world_state_from_database(
            metadata,
            &rounds,
            &choice_explorations,
            &branch_explorations,
        ))
    }

    async fn archive_payload_from_database(
        &self,
        session_id: &str,
        title: Option<&str>,
        context_through_round: Option<u64>,
    ) -> Result<archive::SessionArchivePayload, AppError> {
        let metadata = self.load_session_metadata(session_id).await?;
        let rounds = self.load_session_rounds(session_id).await?;
        let contexts = self
            .load_entity_context_map(session_id, context_through_round)
            .await?;
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

    async fn load_entity_context_map(
        &self,
        session_id: &str,
        through_round: Option<u64>,
    ) -> Result<HashMap<String, Context>, AppError> {
        let contexts = self
            .session_archive_repo
            .load_entity_contexts_through_round(session_id, through_round)
            .await
            .map_err(|err| AppError::internal(format!("读取 entity context 失败：{err:#}")))?;
        Ok(contexts
            .into_iter()
            .map(|context| (context.entity_name, context.context))
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
            // 活跃 SSE 流会把会话钉在内存里；非稳定阶段也不驱逐，避免下次
            // 控制请求从不可恢复的中间态继续。
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

        // 快照和关闭 engine 都在 registry 锁外执行。任何一步失败，都把原来的
        // hot runtime 放回去，保证调用方仍能继续使用当前会话。
        let payload = match self
            .archive_payload_for_session(session_id, None, None)
            .await
        {
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
        // Drop 里不能 await，因此异步释放 stream pin；用 saturating_sub 防止
        // 重复释放或竞态导致计数下溢。
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
    let (events_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let live_events = Arc::new(Mutex::new(LiveEventLog::with_capacity(
        live_event_history_capacity,
    )));
    let submitted_action_rounds = Arc::new(Mutex::new(HashSet::new()));
    let output_node_targets = Arc::new(Mutex::new(HashMap::new()));
    // 这个后台任务把 engine 事件同时桥接到实时订阅者和归档数据库，避免请求
    // 路径手动维护每一种 turn 事件的持久化细节。
    spawn_engine_event_bridge(
        session_id,
        &engine,
        events_tx.clone(),
        Arc::clone(&live_events),
        session_archive_repo,
        Arc::clone(&output_node_targets),
    );

    HotSession {
        engine,
        events_tx,
        live_events,
        submitted_action_rounds,
        output_node_targets,
    }
}

fn spawn_engine_event_bridge(
    session_id: &str,
    engine: &AkashicSessionEngine,
    events_tx: broadcast::Sender<LiveEngineEvent>,
    live_events: Arc<Mutex<LiveEventLog>>,
    session_archive_repo: SessionArchiveRepository,
    output_node_targets: Arc<Mutex<HashMap<u64, String>>>,
) {
    let mut event_rx = engine.subscribe_session_events();
    let session_id_for_event_task = session_id.to_string();
    tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    let live_event = {
                        let mut live_events = live_events.lock().await;
                        live_events.push(event.clone())
                    };
                    let _ = events_tx.send(live_event);

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
                        EngineEvent::EntityContextItemAppended(update) => {
                            if let Err(error) =
                                session_archive_repo.save_entity_context_item(&update).await
                            {
                                tracing::warn!(
                                    "failed to persist entity context item for {} / {}: {:?}",
                                    update.session_id,
                                    update.entity_name,
                                    error
                                );
                            }
                        }
                        EngineEvent::EntityContextRollback(rollback) => {
                            if let Err(error) = session_archive_repo
                                .save_entity_context_rollback(&rollback)
                                .await
                            {
                                tracing::warn!(
                                    "failed to persist entity context rollback for {} / {}: {:?}",
                                    rollback.session_id,
                                    rollback.entity_name,
                                    error
                                );
                            }
                        }
                        EngineEvent::FlowTurnUpdate(update) => {
                            let persisted_phase = persisted_phase_for_flow_turn_update(&update);
                            let output_node_target =
                                output_node_targets.lock().await.get(&update.round).cloned();
                            if let Some(node_id) = output_node_target.as_deref() {
                                if let Err(error) = session_archive_repo
                                    .update_session_turn_state_for_node(
                                        &update.session_id,
                                        node_id,
                                        persisted_phase,
                                    )
                                    .await
                                {
                                    tracing::warn!(
                                        "failed to update session turn state for {} at {}: {:?}",
                                        update.session_id,
                                        node_id,
                                        error
                                    );
                                }
                                if let Err(error) = session_archive_repo
                                    .save_flow_turn_update_for_node(&update, node_id)
                                    .await
                                {
                                    tracing::warn!(
                                        "failed to persist entity flow output for {} at {}: {:?}",
                                        session_id_for_event_task,
                                        node_id,
                                        error
                                    );
                                }
                            } else {
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
                                        "failed to persist entity flow output for {}: {:?}",
                                        session_id_for_event_task,
                                        error
                                    );
                                }
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
                            output_node_targets.lock().await.remove(&completed.round);
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
                            output_node_targets.lock().await.remove(&end.round);
                        }
                        EngineEvent::FlowTurnError(error_event) => {
                            if let Err(error) = session_archive_repo
                                .record_flow_turn_error(&error_event)
                                .await
                            {
                                tracing::warn!(
                                    "failed to persist errored flow turn for {} / {}: {:?}",
                                    error_event.session_id,
                                    error_event.round,
                                    error
                                );
                            }
                            output_node_targets.lock().await.remove(&error_event.round);
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        "session event persistence lagged for {} and skipped {} events",
                        session_id_for_event_task,
                        skipped
                    );
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn normalized_action_items(action: SessionActionInput) -> Result<Vec<PlayerActionItem>, AppError> {
    let actions = action
        .actions
        .into_iter()
        .map(PlayerActionItem::normalized)
        .filter(|action| !action.action.is_empty())
        .collect::<Vec<_>>();
    if actions.is_empty() {
        return Err(AppError::bad_request("提交行动不能为空。"));
    }
    Ok(actions)
}

fn validate_selected_actions_against_choices(
    actions: &[PlayerActionItem],
    choices: &[PendingCharacterChoice],
) -> Result<(), AppError> {
    for selected_action in actions
        .iter()
        .filter(|action| action.action_type == PlayerActionType::SelectedOption)
    {
        let is_valid_option = choices
            .iter()
            .any(|choice| choice.option.action == selected_action.action);
        if !is_valid_option {
            return Err(AppError::bad_request("当前所选行动不在候选列表中。"));
        }
    }
    Ok(())
}

async fn submit_prepared_action(
    hot: &HotSessionAccess,
    expected_round: u64,
    actions: Vec<PlayerActionItem>,
) -> Result<(), AppError> {
    {
        let mut submitted_rounds = hot.submitted_action_rounds.lock().await;
        if !submitted_rounds.insert(expected_round) {
            return Err(AppError::bad_request("这一轮行动正在提交，请勿重复选择。"));
        }
    }

    if let Err(error) = hot
        .engine
        .submit_player_action_for_turn(expected_round, SessionActionInput { actions })
        .await
    {
        hot.submitted_action_rounds
            .lock()
            .await
            .remove(&expected_round);
        return Err(AppError::bad_request(error));
    }

    Ok(())
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
    if update.stage == TurnPhase::Application && update.output_type == AgentOutputType::Json {
        TurnPhase::AwaitingPlayer
    } else {
        update.stage
    }
}

fn world_state_from_database(
    metadata: StoredSessionMetadata,
    rounds: &[RoundHistoryEntry],
    choice_explorations: &[StoredChoiceExploration],
    branch_explorations: &[StoredBranchExploration],
) -> GameSessionWorldStateData {
    let world_snapshot = latest_world_snapshot(rounds).unwrap_or_default();
    let latest_narration = latest_narration_from_rounds(rounds, &world_snapshot);
    let current_outcome = latest_committed_actions(rounds)
        .map(|actions| summarize_actions(&actions))
        .unwrap_or_else(|| {
            if metadata.phase == TurnPhase::Start {
                "start".to_string()
            } else {
                "尚未做出选择".to_string()
            }
        });
    let (choices, choice_explorations, branch_explorations) =
        latest_choices_round_from_rounds(rounds)
            .map(|round| {
                (
                    round.choices.clone(),
                    choice_explorations_for_round(round, choice_explorations),
                    branch_explorations_for_round(round.round, branch_explorations),
                )
            })
            .unwrap_or_default();

    GameSessionWorldStateData {
        session_id: metadata.session_id,
        generated_profiles: GeneratedProfilesData {
            world: metadata.world_profile,
            character: metadata.character_profile,
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
        choice_explorations,
        branch_explorations,
    }
}

fn archive_payload_from_database(
    metadata: StoredSessionMetadata,
    rounds: Vec<RoundHistoryEntry>,
    contexts: HashMap<String, Context>,
    requested_title: Option<&str>,
) -> archive::SessionArchivePayload {
    let world_snapshot = latest_world_snapshot(&rounds).unwrap_or_default();
    let committed_actions = latest_committed_actions(&rounds)
        .unwrap_or_else(|| vec![PlayerActionItem::character_free_text("start")]);
    let choices = latest_choices_from_rounds(&rounds);
    let round_for_title = metadata.active_turn_id.max(metadata.turn_index).max(1);
    let title = archive_title_for_round(
        requested_title,
        "",
        &world_snapshot.scene_title,
        round_for_title,
    );
    let character_name = metadata.character_name;

    archive::SessionArchivePayload {
        session_id: metadata.session_id,
        title,
        character_name: character_name.clone(),
        world_profile: metadata.world_profile,
        character_profile: metadata.character_profile,
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
        character_agent: contexts.get(&character_name).cloned().unwrap_or_default(),
        world_snapshot,
        character_decision: archive::CharacterDecisionArchive {
            committed_actions,
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

fn latest_choices_from_rounds(rounds: &[RoundHistoryEntry]) -> Vec<PendingCharacterChoice> {
    latest_choices_round_from_rounds(rounds)
        .map(|round| round.choices.clone())
        .unwrap_or_default()
}

fn latest_choices_round_from_rounds(rounds: &[RoundHistoryEntry]) -> Option<&RoundHistoryEntry> {
    rounds.iter().rev().find(|round| !round.choices.is_empty())
}

fn round_history_data_from_entry(
    entry: RoundHistoryEntry,
    choice_explorations: &[StoredChoiceExploration],
    branch_explorations: &[StoredBranchExploration],
) -> RoundHistoryData {
    let mut data = RoundHistoryData::from(entry.clone());
    data.choice_explorations = choice_explorations_for_round(&entry, choice_explorations);
    data.branch_explorations = branch_explorations_for_round(entry.round, branch_explorations);
    data
}

fn choice_explorations_for_round(
    round: &RoundHistoryEntry,
    choice_explorations: &[StoredChoiceExploration],
) -> ChoiceExplorationsData {
    let visited_actions = choice_explorations
        .iter()
        .filter(|exploration| exploration.round == round.round)
        .map(|exploration| exploration.action.as_str())
        .collect::<HashSet<_>>();

    round
        .choices
        .iter()
        .map(|choice| {
            (
                choice.option.action.clone(),
                ChoiceExplorationData {
                    visited: visited_actions.contains(choice.option.action.as_str()),
                },
            )
        })
        .collect()
}

fn branch_explorations_for_round(
    round: u64,
    branch_explorations: &[StoredBranchExploration],
) -> Vec<BranchExplorationData> {
    branch_explorations
        .iter()
        .filter(|exploration| exploration.round == round)
        .map(|exploration| BranchExplorationData {
            action: exploration.action.clone(),
            visited: exploration.visited,
        })
        .collect()
}

fn latest_committed_actions(rounds: &[RoundHistoryEntry]) -> Option<Vec<PlayerActionItem>> {
    rounds
        .iter()
        .rev()
        .map(|round| {
            round
                .committed_actions
                .iter()
                .cloned()
                .map(PlayerActionItem::normalized)
                .filter(|action| !action.action.is_empty())
                .collect::<Vec<_>>()
        })
        .find(|actions| !actions.is_empty())
}

fn rounds_with_story_edge_actions(
    mut rounds: Vec<RoundHistoryEntry>,
    story_edge_actions: Vec<StoredStoryEdgeAction>,
) -> Vec<RoundHistoryEntry> {
    for input in story_edge_actions {
        let action = input.action.normalized();
        if action.action.is_empty() {
            continue;
        }

        if let Some(round) = rounds.iter_mut().find(|round| round.round == input.round) {
            if let Some(existing) = round
                .committed_actions
                .iter_mut()
                .find(|existing| existing.character_name == action.character_name)
            {
                *existing = action;
            } else {
                round.committed_actions.push(action);
            }
        } else {
            rounds.push(RoundHistoryEntry {
                round: input.round,
                committed_actions: vec![action],
                ..RoundHistoryEntry::default()
            });
        }
    }
    rounds.sort_by_key(|round| round.round);
    rounds
}

fn summarize_actions(actions: &[PlayerActionItem]) -> String {
    match actions {
        [] => "start".to_string(),
        [single] => single.action.clone(),
        many => many
            .iter()
            .map(|action| format!("{}: {}", action.character_name, action.action))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn stabilize_archive_payload_from_history(
    payload: &mut archive::SessionArchivePayload,
    requested_title: Option<&str>,
    requested_completed_round: Option<u64>,
) -> Result<(), String> {
    if requested_completed_round.is_none() && is_archive_stable_phase(payload.turn_state.phase) {
        return Ok(());
    }

    let completed_round = payload
        .history_log
        .rounds
        .iter()
        .rev()
        .find(|entry| {
            requested_completed_round.is_none_or(|round| entry.round == round)
                && is_completed_dialogue_round(entry)
        })
        .cloned()
        .ok_or_else(|| {
            requested_completed_round
                .map(|round| format!("第 {round} 章还没有可继续的已完成记录。"))
                .unwrap_or_else(|| "当前会话还没有已完成的对话可用于创建存档".to_string())
        })?;
    let completed_round_id = completed_round.round;
    let world_snapshot = completed_round
        .world_snapshot
        .clone()
        .expect("completed dialogue rounds require a world snapshot");
    let committed_actions = payload
        .history_log
        .rounds
        .iter()
        .rev()
        .filter(|entry| entry.round < completed_round_id)
        .map(|entry| entry.committed_actions.clone())
        .find(|actions| !actions.is_empty())
        .unwrap_or_else(|| vec![PlayerActionItem::character_free_text("start")]);
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
        entry.committed_actions.clear();
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
    payload.character_decision.committed_actions = committed_actions;
    payload.character_decision.choices = completed_round.choices;
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
    use crate::api::archive::{CharacterDecisionArchive, SessionArchivePayload, TurnStateArchive};
    use agent::agent::context::Context;
    use rusqlite::{Connection, params};
    use serde_json::Value;
    use story_engine::components::{
        outcome::{CharacterOption, PendingCharacterChoice},
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

    #[tokio::test]
    async fn load_archive_restores_selected_choice_for_each_history_round() {
        let state = test_state();
        let total_rounds = 3;
        let compressed =
            archive::compress_archive_payload(&sample_payload_with_rounds(total_rounds))
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
        assert!(cloned.session_id.starts_with("session-"));
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

    fn sample_payload() -> SessionArchivePayload {
        SessionArchivePayload {
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
                        committed_actions: vec![PlayerActionItem::character_selected_option(
                            &option,
                        )],
                    }
                })
                .collect(),
        };
        payload
    }
}

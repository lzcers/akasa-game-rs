mod dto;

pub use dto::*;

use std::{convert::Infallible, time::Duration};

use agent::{
    agent::{CallModelEvent, call_model},
    core::Message,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Response},
};
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use story_engine::{
    resources::session_events::EngineEvent,
    utils::{build_chat_model, parse_json_response},
};
use tokio::sync::broadcast;

use crate::{
    error::AppError,
    state::{AppState, LiveEngineEvent},
};

use super::dto::ApiResponse;

type ApiResult<T> = Result<Json<ApiResponse<T>>, AppError>;
type StorySseResult = Result<Response, AppError>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamDoneData {
    route: &'static str,
    session_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamWarningData {
    session_id: String,
    reason: &'static str,
    skipped: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamHandshakeData {
    session_id: String,
    protocol: &'static str,
    note: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamLiveEngineEvent {
    event_id: u64,
    event: Value,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamQuery {
    since: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(Deserialize)]
struct StorySummaryPayload {
    summary: String,
}

pub async fn create_game_session(
    State(state): State<AppState>,
    Json(request): Json<CreateGameSessionRequest>,
) -> ApiResult<CreateGameSessionData> {
    let session = state.create_game_session(request).await?;
    Ok(Json(ApiResponse::ok(session)))
}

pub async fn save_export(
    State(state): State<AppState>,
    Path(path): Path<SessionPath>,
    Json(request): Json<SaveExportRequest>,
) -> ApiResult<SaveExportData> {
    let exported = state
        .export_save_archive(&path.session_id, request.title.as_deref())
        .await?;
    Ok(Json(ApiResponse::ok(exported)))
}

pub async fn load_archive(
    State(state): State<AppState>,
    Json(request): Json<LoadArchiveRequest>,
) -> ApiResult<GameSessionWorldStateData> {
    let session = state
        .load_game_session_from_archive(request.compressed_archive)
        .await?;
    Ok(Json(ApiResponse::ok(session)))
}

pub async fn get_game_session_world(
    State(state): State<AppState>,
    Path(path): Path<SessionPath>,
) -> ApiResult<GameSessionWorldStateData> {
    let state_view = state.get_game_session_world(&path.session_id).await?;
    Ok(Json(ApiResponse::ok(state_view)))
}

pub async fn get_game_session_rounds(
    State(state): State<AppState>,
    Path(path): Path<SessionPath>,
    Query(query): Query<SessionRoundsQuery>,
) -> ApiResult<SessionRoundsPageData> {
    let page = state
        .get_game_session_rounds(
            &path.session_id,
            query.before_round,
            query.limit.unwrap_or(DEFAULT_SESSION_ROUNDS_LIMIT),
        )
        .await?;
    Ok(Json(ApiResponse::ok(page)))
}

pub async fn clone_game_session(
    State(state): State<AppState>,
    Path(path): Path<SessionPath>,
    Query(query): Query<CloneGameSessionQuery>,
) -> ApiResult<GameSessionWorldStateData> {
    let state_view = state
        .clone_game_session(&path.session_id, query.round)
        .await?;
    Ok(Json(ApiResponse::ok(state_view)))
}

pub async fn generate_story_summary(
    State(state): State<AppState>,
    Path(path): Path<SessionPath>,
) -> ApiResult<StorySummaryData> {
    let narrations = state.get_game_session_narrations(&path.session_id).await?;
    if narrations.is_empty() {
        return Err(AppError::bad_request(
            "当前会话还没有可用于摘要的 narration 文本。",
        ));
    }

    let mut model = build_chat_model();
    model.set_output_json(true);

    let messages = vec![
        Message::system(
            r#"你是一名互动叙事编辑，负责把多段游戏 narration 原文整理成一段更适合展示给玩家阅读的故事摘要文案。

请严格遵守以下规则：
1. 只能依据用户提供的 narration 原文总结，不得编造原文中不存在的人物、事件、结局或设定。
2. 输出目标是一段连贯、好读、有吸引力的中文摘要文案，不是提纲、时间线、分析报告，也不是旁白续写。
3. 需要保留故事中的核心冲突、氛围变化、关键推进与悬念，但不要逐句复述。
4. 语气偏文学化、具画面感，适合放在 UI 中给玩家快速回顾剧情。
5. 摘要应聚焦“已经发生了什么、局势如何变化、人物正被什么逼近”，长度控制在 120 到 220 字。
6. 如果原文信息有限，就做克制概括，不要为了凑长度扩写。

请按以下 JSON 结构输出，字段名必须完全一致：
{
  "summary": "摘要文案"
}

输出要求：
1. 只输出一个合法 JSON 对象。
2. 不要输出代码块、解释、标题或对象外文本。
3. `summary` 必须是非空字符串。"#,
        ),
        Message::user(format!(
            "请基于以下 narration 原文生成摘要：\n\n{}",
            format_story_narrations(&narrations)
        )),
    ];

    let mut stream = std::pin::pin!(call_model(&model, &messages, None));

    while let Some(event) = stream.next().await {
        match event {
            CallModelEvent::Completed { content, .. } => {
                let summary = parse_story_summary(&content).map_err(|message| {
                    AppError::internal(format!("模型返回的摘要格式不符合预期：{message}"))
                })?;
                return Ok(Json(ApiResponse::ok(StorySummaryData {
                    summary,
                    narration_count: narrations.len(),
                })));
            }
            CallModelEvent::Error(message) => {
                return Err(AppError::internal(format!("生成故事摘要失败：{message}")));
            }
            CallModelEvent::TextChunk(_) | CallModelEvent::ReasoningChunk(_) => {}
        }
    }

    Err(AppError::internal("模型未返回完整摘要结果。"))
}

pub async fn control_game_session(
    State(state): State<AppState>,
    Path(path): Path<SessionPath>,
    Json(request): Json<ControlGameSessionRequest>,
) -> ApiResult<ControlGameSessionData> {
    let result = state
        .control_game_session(&path.session_id, request)
        .await?;
    Ok(Json(ApiResponse::ok(result)))
}

pub async fn stream_game_session(
    State(state): State<AppState>,
    Path(path): Path<SessionPath>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> StorySseResult {
    let since = query.since.or_else(|| last_event_id_from_headers(&headers));
    let live_stream = state
        .open_game_session_stream(&path.session_id, since)
        .await?;
    let session_id = live_stream.session_id.clone();
    let lease = live_stream.lease;

    let handshake_stream = stream::iter([Ok::<_, Infallible>(sse_json_event(
        "stream.handshake",
        StreamHandshakeData {
            session_id: session_id.clone(),
            protocol: "sse",
            note: "subscribed",
        },
    ))]);
    let replay_stream = stream::iter(
        live_stream
            .replayed_events
            .into_iter()
            .filter_map(engine_event_sse)
            .map(Ok::<_, Infallible>),
    );
    let live_stream = stream::unfold(
        Some((live_stream.event_rx, session_id, lease)),
        |state| async move {
            let (mut event_rx, session_id, lease) = state?;

            loop {
                match event_rx.recv().await {
                    Ok(event) => {
                        if let Some(event) = engine_event_sse(event) {
                            return Some((Ok(event), Some((event_rx, session_id, lease))));
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        return Some((
                            Ok(sse_json_event(
                                "stream.warning",
                                StreamWarningData {
                                    session_id: session_id.clone(),
                                    reason: "lagged",
                                    skipped,
                                },
                            )),
                            Some((event_rx, session_id, lease)),
                        ));
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        return Some((
                            Ok(sse_done_event("stream_game_session.done", Some(session_id))),
                            None,
                        ));
                    }
                }
            }
        },
    );

    Ok(
        Sse::new(handshake_stream.chain(replay_stream).chain(live_stream))
            .keep_alive(
                KeepAlive::new()
                    .interval(Duration::from_secs(15))
                    .text("keep-alive"),
            )
            .into_response(),
    )
}

fn sse_json_event<T>(name: &str, data: T) -> Event
where
    T: Serialize,
{
    Event::default()
        .event(name)
        .json_data(data)
        .expect("failed to serialize SSE event")
}

fn sse_done_event(route: &'static str, session_id: Option<String>) -> Event {
    sse_json_event(route, StreamDoneData { route, session_id })
}

fn last_event_id_from_headers(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
}

fn engine_event_sse(update: LiveEngineEvent) -> Option<Event> {
    let event_id = update.event_id;
    compact_live_engine_event(update)
        .map(|event| sse_json_event("engine.event", event).id(event_id.to_string()))
}

fn compact_live_engine_event(update: LiveEngineEvent) -> Option<StreamLiveEngineEvent> {
    let event = compact_engine_event(update.event)?;
    Some(StreamLiveEngineEvent {
        event_id: update.event_id,
        event,
    })
}

fn compact_engine_event(event: EngineEvent) -> Option<Value> {
    let mut event = serde_json::to_value(event).expect("failed to serialize engine event");
    let Some(event_object) = event.as_object_mut() else {
        return Some(event);
    };
    let event_type = event_object.get("type").and_then(Value::as_str);

    match event_type {
        Some(
            "session_created"
            | "task_completed"
            | "entity_context_item_appended"
            | "entity_context_rollback",
        ) => None,
        Some("task_update")
            if event_object.get("entity_name").and_then(Value::as_str) != Some("UpperNarrator") =>
        {
            None
        }
        Some("flow_turn_update") => {
            event_object.remove("content");
            Some(event)
        }
        _ => Some(event),
    }
}

fn format_story_narrations(narrations: &[String]) -> String {
    narrations
        .iter()
        .enumerate()
        .map(|(index, text)| format!("第{}段\n{}", index + 1, text.trim()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn parse_story_summary(content: &str) -> Result<String, String> {
    let parsed = parse_json_response::<StorySummaryPayload>(content)?;
    validate_story_summary(parsed)
}

fn validate_story_summary(payload: StorySummaryPayload) -> Result<String, String> {
    let summary = payload.summary.trim();
    if summary.is_empty() {
        return Err("`summary` 不能为空。".to_string());
    }

    Ok(summary.to_string())
}

const DEFAULT_SESSION_ROUNDS_LIMIT: usize = 30;

#[cfg(test)]
mod tests {
    use super::*;
    use story_engine::{
        components::{agent::AgentOutputType, turn_flow::TurnStage},
        resources::session_events::{
            EntityContextItemAppended, EntityContextRollback, EntityContextRollbackPolicy,
            FlowTurnUpdate, SessionCreated, TaskCompleted, TaskUpdate,
        },
    };

    #[test]
    fn format_story_narrations_numbers_and_trims_segments() {
        let formatted = format_story_narrations(&[
            "  雨水从钟楼裂隙里渗下来。 ".to_string(),
            "她听见地下回廊传来迟缓脚步。".to_string(),
        ]);

        assert_eq!(
            formatted,
            "第1段\n雨水从钟楼裂隙里渗下来。\n\n第2段\n她听见地下回廊传来迟缓脚步。"
        );
    }

    #[test]
    fn parse_story_summary_rejects_empty_summary() {
        let error = parse_story_summary(r#"{"summary":"   "}"#).expect_err("summary should fail");

        assert_eq!(error, "`summary` 不能为空。");
    }

    #[test]
    fn last_event_id_from_headers_parses_eventsource_resume_id() {
        let mut headers = HeaderMap::new();
        headers.insert("Last-Event-ID", "17".parse().expect("valid header"));

        assert_eq!(last_event_id_from_headers(&headers), Some(17));
    }

    #[test]
    fn compact_engine_event_keeps_task_update_chunks() {
        let event = compact_engine_event(EngineEvent::TaskUpdate(TaskUpdate {
            session_id: "session-1".to_string(),
            round: 2,
            entity_name: "UpperNarrator".to_string(),
            chunk: "雨声".to_string(),
        }))
        .expect("task updates should be streamed");

        assert_eq!(event["type"], "task_update");
        assert_eq!(event["chunk"], "雨声");
    }

    #[test]
    fn compact_engine_event_filters_non_narrator_task_updates() {
        let event = compact_engine_event(EngineEvent::TaskUpdate(TaskUpdate {
            session_id: "session-1".to_string(),
            round: 2,
            entity_name: "FateWeaver".to_string(),
            chunk: r#"{"large":"internal json chunk"}"#.to_string(),
        }));

        assert!(event.is_none());
    }

    #[test]
    fn compact_engine_event_filters_session_created() {
        let event = compact_engine_event(EngineEvent::SessionCreated(SessionCreated {
            session_id: "session-1".to_string(),
            character_name: "洛寒".to_string(),
            world_profile: "world".to_string(),
            character_profile: "hero".to_string(),
            key_story_beats: "beats".to_string(),
        }));

        assert!(event.is_none());
    }

    #[test]
    fn compact_engine_event_filters_completed_tasks() {
        let event = compact_engine_event(EngineEvent::TaskCompleted(TaskCompleted {
            session_id: "session-1".to_string(),
            round: 2,
            entity_name: "UpperNarrator".to_string(),
            content: "完整叙事正文".to_string(),
        }));

        assert!(event.is_none());
    }

    #[test]
    fn compact_engine_event_filters_entity_context_items() {
        let event = compact_engine_event(EngineEvent::EntityContextItemAppended(
            EntityContextItemAppended {
                session_id: "session-1".to_string(),
                round: 2,
                entity_name: "UpperNarrator".to_string(),
                message: Message::user("internal context"),
            },
        ));

        assert!(event.is_none());
    }

    #[test]
    fn compact_engine_event_filters_entity_context_rollbacks() {
        let event =
            compact_engine_event(EngineEvent::EntityContextRollback(EntityContextRollback {
                session_id: "session-1".to_string(),
                round: 2,
                entity_name: "UpperNarrator".to_string(),
                policy: EntityContextRollbackPolicy::LatestInput,
            }));

        assert!(event.is_none());
    }

    #[test]
    fn compact_engine_event_removes_flow_turn_update_content() {
        let event = compact_engine_event(EngineEvent::FlowTurnUpdate(FlowTurnUpdate {
            session_id: "session-1".to_string(),
            round: 2,
            stage: TurnStage::Application,
            entity_name: "UpperNarrator".to_string(),
            output_type: AgentOutputType::Text,
            content: "完整叙事正文".to_string(),
        }))
        .expect("flow turn updates should still be streamed");

        assert_eq!(event["type"], "flow_turn_update");
        assert_eq!(event["stage"], "application");
        assert!(event.get("content").is_none());
    }
}

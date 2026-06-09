use serde::{Deserialize, Serialize};
use serde_json::Value;
use story_engine::resources::{
    export::TaskView,
    history::RoundHistoryEntry,
    protagonist_action::{PendingProtagonistChoice, PlayerActionInput},
    turn_state::TurnPhase,
    world_snapshot::{ItemState, NpcState, OngoingEvent, WorldSnapshot},
};

use crate::error::AppError;

const FEEDBACK_CONTENT_MAX_CHARS: usize = 5000;
const FEEDBACK_METADATA_MAX_CHARS: usize = 500;

#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: T,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionPath {
    pub session_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsBatchRequest {
    pub events: Vec<AnalyticsEventInput>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsBatchData {
    pub accepted: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsEventInput {
    pub id: String,
    pub event_name: String,
    pub anonymous_user_id: String,
    pub client_session_id: String,
    pub game_session_id: Option<String>,
    pub source_session_id: Option<String>,
    pub occurred_at: String,
    pub app: String,
    pub app_version: Option<String>,
    pub path: Option<String>,
    pub referrer_domain: Option<String>,
    pub utm_source: Option<String>,
    pub utm_medium: Option<String>,
    pub utm_campaign: Option<String>,
    pub device_type: Option<String>,
    pub platform: Option<String>,
    #[serde(default)]
    pub properties: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackType {
    Bug,
    Suggestion,
    Story,
    Other,
}

impl FeedbackType {
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::Suggestion => "suggestion",
            Self::Story => "story",
            Self::Other => "other",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Bug => "问题反馈",
            Self::Suggestion => "功能建议",
            Self::Story => "剧情体验",
            Self::Other => "其他反馈",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitFeedbackRequest {
    #[serde(rename = "type")]
    pub feedback_type: FeedbackType,
    pub email: Option<String>,
    pub content: String,
    pub page: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ValidatedFeedbackRequest {
    pub feedback_type: FeedbackType,
    pub email: Option<String>,
    pub content: String,
    pub page: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitFeedbackData {
    pub feedback_id: String,
    pub accepted: bool,
}

impl SubmitFeedbackRequest {
    pub fn validate(self) -> Result<ValidatedFeedbackRequest, AppError> {
        let content = self.content.trim();
        if content.is_empty() {
            return Err(AppError::bad_request("`content` 不能为空。"));
        }
        if content.chars().count() > FEEDBACK_CONTENT_MAX_CHARS {
            return Err(AppError::bad_request(format!(
                "`content` 不能超过 {FEEDBACK_CONTENT_MAX_CHARS} 个字符。"
            )));
        }

        let email = normalize_optional(self.email, 254, "`email`")?;
        if let Some(email) = &email {
            email
                .parse::<lettre::message::Mailbox>()
                .map_err(|_| AppError::bad_request("`email` 不是有效邮箱地址。"))?;
        }

        Ok(ValidatedFeedbackRequest {
            feedback_type: self.feedback_type,
            email,
            content: content.to_string(),
            page: normalize_optional(self.page, FEEDBACK_METADATA_MAX_CHARS, "`page`")?,
            user_agent: normalize_optional(
                self.user_agent,
                FEEDBACK_METADATA_MAX_CHARS,
                "`userAgent`",
            )?,
        })
    }
}

fn normalize_optional(
    value: Option<String>,
    max_chars: usize,
    field_name: &'static str,
) -> Result<Option<String>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > max_chars {
        return Err(AppError::bad_request(format!(
            "{field_name} 不能超过 {max_chars} 个字符。"
        )));
    }
    Ok(Some(value.to_string()))
}

impl AnalyticsEventInput {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.id.trim().is_empty() {
            return Err(AppError::bad_request("analytics event `id` 不能为空。"));
        }
        if self.event_name.trim().is_empty() {
            return Err(AppError::bad_request(
                "analytics event `eventName` 不能为空。",
            ));
        }
        if self.anonymous_user_id.trim().is_empty() {
            return Err(AppError::bad_request(
                "analytics event `anonymousUserId` 不能为空。",
            ));
        }
        if self.client_session_id.trim().is_empty() {
            return Err(AppError::bad_request(
                "analytics event `clientSessionId` 不能为空。",
            ));
        }
        if self.app.trim().is_empty() {
            return Err(AppError::bad_request("analytics event `app` 不能为空。"));
        }
        if self.occurred_at.trim().is_empty() {
            return Err(AppError::bad_request(
                "analytics event `occurredAt` 不能为空。",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenerateProfilesRequest {
    pub prompt: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateProfilesData {
    pub world: String,
    pub protagonist: String,
    pub key_story_beats: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorySummaryData {
    pub summary: String,
    pub narration_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGameSessionRequest {
    pub world_profile: String,
    pub protagonist_profile: String,
    #[serde(default)]
    pub key_story_beats: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGameSessionData {
    pub session_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveExportData {
    pub session_id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub compressed_archive: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveExportRequest {
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadArchiveRequest {
    pub compressed_archive: String,
}

pub type SessionActionInput = PlayerActionInput;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GameSessionControlCommand {
    Continue,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlGameSessionRequest {
    pub control: Option<GameSessionControlCommand>,
    pub action: Option<SessionActionInput>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSessionWorldStateData {
    pub session_id: String,
    pub status: String,
    pub phase: TurnPhase,
    pub turn_index: u64,
    pub active_turn_id: u64,
    pub world_state: WorldStateData,
    pub history: Vec<RoundHistoryData>,
    pub current_task: Option<TaskView>,
    pub tasks: Vec<TaskView>,
    pub latest_narration: String,
    pub current_protagonist_action: String,
    pub choices: Vec<PendingProtagonistChoice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundHistoryData {
    pub round: u64,
    pub world_state: Option<WorldStateData>,
    pub narration_text: String,
    pub choices: Vec<PendingProtagonistChoice>,
    pub committed_action: Option<String>,
    pub selected_choice_text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldStateData {
    pub round: u64,
    pub scene_title: String,
    pub time_absolute: String,
    pub time_relative: Option<String>,
    pub location_name: String,
    pub location_exits: Vec<String>,
    pub location_status: String,
    pub description: String,
    pub current_event: String,
    pub new_info: Vec<String>,
    pub inner_conflict: String,
    pub hard_anchors: Vec<String>,
    pub pace: String,
    pub atmosphere: String,
    pub focal_point: String,
    pub protagonist_condition: String,
    pub protagonist_known_secrets: Vec<String>,
    pub npcs: Vec<NpcStateData>,
    pub items: Vec<ItemStateData>,
    pub events_in_progress: Vec<OngoingEventData>,
    pub unsolved_threads: Vec<String>,
    pub pacing_note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcStateData {
    pub name: String,
    pub location: String,
    pub mood: String,
    pub attitude: String,
    pub goal: String,
    pub secrets: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemStateData {
    pub name: String,
    pub location: String,
    pub status: String,
    pub awareness: String,
    pub relevance: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OngoingEventData {
    pub name: String,
    pub status: String,
    pub escalation_trigger: String,
}

impl From<WorldSnapshot> for WorldStateData {
    fn from(value: WorldSnapshot) -> Self {
        Self {
            round: value.round,
            scene_title: value.scene_title,
            time_absolute: value.time_absolute,
            time_relative: value.time_relative,
            location_name: value.location_name,
            location_exits: value.location_exits,
            location_status: value.location_status,
            description: value.description,
            current_event: value.current_event,
            new_info: value.new_info,
            inner_conflict: value.inner_conflict,
            hard_anchors: value.hard_anchors,
            pace: value.pace,
            atmosphere: value.atmosphere,
            focal_point: value.focal_point,
            protagonist_condition: value.protagonist_condition,
            protagonist_known_secrets: value.protagonist_known_secrets,
            npcs: value.npcs.into_iter().map(Into::into).collect(),
            items: value.items.into_iter().map(Into::into).collect(),
            events_in_progress: value
                .events_in_progress
                .into_iter()
                .map(Into::into)
                .collect(),
            unsolved_threads: value.unsolved_threads,
            pacing_note: value.pacing_note,
        }
    }
}

impl From<RoundHistoryEntry> for RoundHistoryData {
    fn from(value: RoundHistoryEntry) -> Self {
        let selected_choice_text = value.committed_action.as_ref().and_then(|action| {
            value.choices.iter().find_map(|choice| {
                (choice.option.action == *action).then(|| choice.option.title.clone())
            })
        });

        Self {
            round: value.round,
            world_state: value.world_snapshot.map(Into::into),
            narration_text: value.narration_text.unwrap_or_default(),
            choices: value.choices,
            committed_action: value.committed_action,
            selected_choice_text,
        }
    }
}

impl From<NpcState> for NpcStateData {
    fn from(value: NpcState) -> Self {
        Self {
            name: value.name,
            location: value.location,
            mood: value.mood,
            attitude: value.attitude,
            goal: value.goal,
            secrets: value.secrets,
        }
    }
}

impl From<ItemState> for ItemStateData {
    fn from(value: ItemState) -> Self {
        Self {
            name: value.name,
            location: value.location,
            status: value.status,
            awareness: value.awareness,
            relevance: value.relevance,
        }
    }
}

impl From<OngoingEvent> for OngoingEventData {
    fn from(value: OngoingEvent) -> Self {
        Self {
            name: value.name,
            status: value.status,
            escalation_trigger: value.escalation_trigger,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlGameSessionData {
    pub action: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_feedback_request_trims_content_and_optional_fields() {
        let validated = SubmitFeedbackRequest {
            feedback_type: FeedbackType::Suggestion,
            email: Some(" player@example.com ".to_string()),
            content: "  希望补充导出邮件功能。 ".to_string(),
            page: Some(" /feedback ".to_string()),
            user_agent: Some(" test-browser ".to_string()),
        }
        .validate()
        .expect("feedback request should validate");

        assert_eq!(validated.email.as_deref(), Some("player@example.com"));
        assert_eq!(validated.content, "希望补充导出邮件功能。");
        assert_eq!(validated.page.as_deref(), Some("/feedback"));
        assert_eq!(validated.user_agent.as_deref(), Some("test-browser"));
    }

    #[test]
    fn submit_feedback_request_rejects_invalid_email() {
        let error = SubmitFeedbackRequest {
            feedback_type: FeedbackType::Bug,
            email: Some("not-an-email".to_string()),
            content: "提交时出现错误。".to_string(),
            page: None,
            user_agent: None,
        }
        .validate()
        .expect_err("invalid email should fail");

        assert!(format!("{error:?}").contains("BAD_REQUEST"));
    }
}

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppError;

const FEEDBACK_CONTENT_MAX_CHARS: usize = 5000;
const FEEDBACK_METADATA_MAX_CHARS: usize = 500;

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

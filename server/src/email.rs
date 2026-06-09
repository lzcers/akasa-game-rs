use std::{collections::HashMap, env, fs, path::PathBuf, sync::OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    address::Address,
    message::{Mailbox, MultiPart},
    transport::smtp::authentication::Credentials,
};
use tracing::warn;

use crate::api::dto::ValidatedFeedbackRequest;

#[derive(Clone)]
pub struct FeedbackMailer {
    config: Option<FeedbackMailConfig>,
}

#[derive(Clone)]
struct FeedbackMailConfig {
    email: String,
    email_token: String,
    smtp_server: String,
}

pub struct FeedbackEmail<'a> {
    pub feedback_id: &'a str,
    pub submitted_at: &'a str,
    pub request: &'a ValidatedFeedbackRequest,
}

impl FeedbackMailer {
    pub fn from_env() -> Self {
        let email = env_value("EMAIL").or_else(|| env_value("email"));
        let email_token = env_value("EMAIL_TOKEN").or_else(|| env_value("email_token"));
        let smtp_server = env_value("SMTP_SERVER").or_else(|| env_value("smtp_server"));

        let config = match (email, email_token, smtp_server) {
            (Some(email), Some(email_token), Some(smtp_server)) => Some(FeedbackMailConfig {
                email,
                email_token,
                smtp_server,
            }),
            _ => {
                warn!(
                    "feedback mailer disabled: set EMAIL/email, EMAIL_TOKEN/email_token and SMTP_SERVER/smtp_server in the environment or .env to enable it"
                );
                None
            }
        };

        Self { config }
    }

    pub async fn send_feedback(&self, feedback: FeedbackEmail<'_>) -> Result<()> {
        let config = self.config.as_ref().context(
            "邮件环境变量未配置：EMAIL/email、EMAIL_TOKEN/email_token、SMTP_SERVER/smtp_server",
        )?;
        let message = build_feedback_message(config, &feedback)?;
        let mailer = build_transport(config)?;

        mailer.send(message).await.context("SMTP 发送失败")?;

        Ok(())
    }
}

pub fn feedback_subject(feedback: &FeedbackEmail<'_>) -> String {
    format!(
        "[AKASA-FEEDBACK][{}][{}] {}",
        feedback.request.feedback_type.tag(),
        feedback.feedback_id,
        feedback.request.feedback_type.label()
    )
}

pub fn feedback_body(feedback: &FeedbackEmail<'_>) -> String {
    let request = feedback.request;
    format!(
        concat!(
            "Akasa Feedback\n",
            "Filter-Key: AKASA-FEEDBACK\n",
            "Feedback-ID: {feedback_id}\n",
            "Type: {type_tag} / {type_label}\n",
            "Submitted-At: {submitted_at}\n",
            "Contact: {contact}\n",
            "Page: {page}\n",
            "User-Agent: {user_agent}\n",
            "\n",
            "---- Feedback Content ----\n",
            "{content}\n"
        ),
        feedback_id = feedback.feedback_id,
        type_tag = request.feedback_type.tag(),
        type_label = request.feedback_type.label(),
        submitted_at = feedback.submitted_at,
        contact = request.email.as_deref().unwrap_or("未提供"),
        page = request.page.as_deref().unwrap_or("未提供"),
        user_agent = request.user_agent.as_deref().unwrap_or("未提供"),
        content = request.content
    )
}

pub fn feedback_html_body(feedback: &FeedbackEmail<'_>) -> String {
    let request = feedback.request;
    format!(
        concat!(
            "<!doctype html>",
            "<html><head><meta charset=\"utf-8\"></head>",
            "<body>",
            "<h1>Akasa Feedback</h1>",
            "<table>",
            "<tr><th align=\"left\">Filter-Key</th><td>AKASA-FEEDBACK</td></tr>",
            "<tr><th align=\"left\">Feedback-ID</th><td>{feedback_id}</td></tr>",
            "<tr><th align=\"left\">Type</th><td>{type_tag} / {type_label}</td></tr>",
            "<tr><th align=\"left\">Submitted-At</th><td>{submitted_at}</td></tr>",
            "<tr><th align=\"left\">Contact</th><td>{contact}</td></tr>",
            "<tr><th align=\"left\">Page</th><td>{page}</td></tr>",
            "<tr><th align=\"left\">User-Agent</th><td>{user_agent}</td></tr>",
            "</table>",
            "<h2>Feedback Content</h2>",
            "<pre style=\"white-space:pre-wrap\">{content}</pre>",
            "</body></html>"
        ),
        feedback_id = escape_html(feedback.feedback_id),
        type_tag = escape_html(request.feedback_type.tag()),
        type_label = escape_html(request.feedback_type.label()),
        submitted_at = escape_html(feedback.submitted_at),
        contact = escape_html(request.email.as_deref().unwrap_or("未提供")),
        page = escape_html(request.page.as_deref().unwrap_or("未提供")),
        user_agent = escape_html(request.user_agent.as_deref().unwrap_or("未提供")),
        content = escape_html(&request.content)
    )
}

fn build_feedback_message(
    config: &FeedbackMailConfig,
    feedback: &FeedbackEmail<'_>,
) -> Result<Message> {
    let sender = Mailbox::new(
        Some("Akasa Feedback".to_string()),
        config
            .email
            .parse::<Address>()
            .context("EMAIL/email 不是有效邮箱地址")?,
    );
    let recipient = config
        .email
        .parse::<Mailbox>()
        .context("EMAIL/email 不是有效收件地址")?;

    let mut builder = Message::builder()
        .from(sender)
        .to(recipient)
        .subject(feedback_subject(feedback));

    if let Some(email) = &feedback.request.email {
        builder = builder.reply_to(
            email
                .parse::<Mailbox>()
                .context("反馈联系邮箱不是有效邮箱地址")?,
        );
    }

    builder
        .multipart(MultiPart::alternative_plain_html(
            feedback_body(feedback),
            feedback_html_body(feedback),
        ))
        .context("构建反馈邮件失败")
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn build_transport(config: &FeedbackMailConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let smtp_server = config.smtp_server.trim();
    if smtp_server.is_empty() {
        bail!("SMTP_SERVER/smtp_server 不能为空");
    }

    let builder = if smtp_server.contains("://") {
        AsyncSmtpTransport::<Tokio1Executor>::from_url(smtp_server)
            .with_context(|| format!("无法解析 SMTP URL `{smtp_server}`"))?
    } else {
        let (host, port) = parse_host_port(smtp_server)?;
        let mut builder = if matches!(port, Some(465) | None) {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
                .with_context(|| format!("无法创建 SMTPS 连接 `{host}`"))?
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
                .with_context(|| format!("无法创建 STARTTLS SMTP 连接 `{host}`"))?
        };

        if let Some(port) = port {
            builder = builder.port(port);
        }
        builder
    };

    Ok(builder
        .credentials(Credentials::new(
            config.email.clone(),
            config.email_token.clone(),
        ))
        .build())
}

fn parse_host_port(smtp_server: &str) -> Result<(String, Option<u16>)> {
    let smtp_server = smtp_server.trim();
    if smtp_server.is_empty() {
        bail!("SMTP_SERVER/smtp_server 不能为空");
    }

    let Some((host, port)) = smtp_server.rsplit_once(':') else {
        return Ok((smtp_server.to_string(), None));
    };
    if host.contains(':') {
        return Ok((smtp_server.to_string(), None));
    }
    if host.trim().is_empty() || port.trim().is_empty() {
        bail!("SMTP_SERVER/smtp_server 格式应为 host 或 host:port");
    }

    let port = port
        .parse::<u16>()
        .map_err(|_| anyhow!("SMTP_SERVER/smtp_server 端口不是有效数字"))?;
    Ok((host.trim().to_string(), Some(port)))
}

fn env_value(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            dotenv_values()
                .get(key)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

fn dotenv_values() -> &'static HashMap<String, String> {
    static DOTENV_VALUES: OnceLock<HashMap<String, String>> = OnceLock::new();
    DOTENV_VALUES.get_or_init(load_dotenv_values)
}

fn load_dotenv_values() -> HashMap<String, String> {
    let mut values = HashMap::new();
    for path in dotenv_candidate_paths() {
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };

        for (key, value) in parse_dotenv_values(&contents) {
            values.entry(key).or_insert(value);
        }
    }
    values
}

fn dotenv_candidate_paths() -> Vec<PathBuf> {
    let Ok(cwd) = env::current_dir() else {
        return Vec::new();
    };

    let mut paths = vec![cwd.join(".env")];
    if let Some(parent) = cwd.parent() {
        paths.push(parent.join(".env"));
    }
    paths
}

fn parse_dotenv_values(contents: &str) -> HashMap<String, String> {
    contents
        .lines()
        .filter_map(parse_dotenv_line)
        .collect::<HashMap<_, _>>()
}

fn parse_dotenv_line(line: &str) -> Option<(String, String)> {
    let line = line.trim_start();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let line = line.strip_prefix("export ").unwrap_or(line);
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }

    Some((key.to_string(), parse_dotenv_value(value)))
}

fn parse_dotenv_value(value: &str) -> String {
    let value = value.trim();
    if let Some(value) = value.strip_prefix('"') {
        return parse_double_quoted_dotenv_value(value);
    }
    if let Some(value) = value.strip_prefix('\'') {
        return value
            .split_once('\'')
            .map(|(quoted, _)| quoted)
            .unwrap_or(value)
            .to_string();
    }

    let value = value
        .split_once(" #")
        .map(|(value, _)| value)
        .unwrap_or(value);
    value.trim().to_string()
}

fn parse_double_quoted_dotenv_value(value: &str) -> String {
    let mut parsed = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') => parsed.push('\n'),
                Some('r') => parsed.push('\r'),
                Some('t') => parsed.push('\t'),
                Some('"') => parsed.push('"'),
                Some('\\') => parsed.push('\\'),
                Some(other) => {
                    parsed.push('\\');
                    parsed.push(other);
                }
                None => parsed.push('\\'),
            },
            _ => parsed.push(ch),
        }
    }
    parsed
}

#[cfg(test)]
mod tests {
    use crate::api::dto::{FeedbackType, ValidatedFeedbackRequest};

    use super::*;

    #[test]
    fn feedback_subject_contains_filter_prefix_type_and_id() {
        let request = feedback_request(FeedbackType::Suggestion);
        let feedback = FeedbackEmail {
            feedback_id: "feedback-abc123",
            submitted_at: "2026-06-09T12:00:00Z",
            request: &request,
        };

        assert_eq!(
            feedback_subject(&feedback),
            "[AKASA-FEEDBACK][suggestion][feedback-abc123] 功能建议"
        );
    }

    #[test]
    fn feedback_body_formats_searchable_metadata() {
        let request = feedback_request(FeedbackType::Bug);
        let feedback = FeedbackEmail {
            feedback_id: "feedback-def456",
            submitted_at: "2026-06-09T12:00:00Z",
            request: &request,
        };
        let body = feedback_body(&feedback);

        assert!(body.contains("Filter-Key: AKASA-FEEDBACK"));
        assert!(body.contains("Feedback-ID: feedback-def456"));
        assert!(body.contains("Type: bug / 问题反馈"));
        assert!(body.contains("Contact: player@example.com"));
        assert!(body.contains("Page: /feedback"));
        assert!(body.contains("---- Feedback Content ----"));
    }

    #[test]
    fn build_feedback_message_contains_plain_and_html_bodies() {
        let request = ValidatedFeedbackRequest {
            feedback_type: FeedbackType::Suggestion,
            email: Some("player@example.com".to_string()),
            content: "Please keep this visible.".to_string(),
            page: Some("/feedback".to_string()),
            user_agent: Some("unit-test".to_string()),
        };
        let feedback = FeedbackEmail {
            feedback_id: "feedback-visible",
            submitted_at: "2026-06-09T12:00:00Z",
            request: &request,
        };
        let config = FeedbackMailConfig {
            email: "zack@ksana.net".to_string(),
            email_token: "token".to_string(),
            smtp_server: "smtp.example.com".to_string(),
        };
        let message =
            build_feedback_message(&config, &feedback).expect("feedback message should build");
        let formatted = String::from_utf8(message.formatted()).expect("message should be utf8");

        assert!(formatted.contains("Content-Type: multipart/alternative;"));
        assert!(formatted.contains("Content-Type: text/plain;"));
        assert!(formatted.contains("Content-Type: text/html;"));
        assert!(formatted.contains("Please keep this visible."));
    }

    #[test]
    fn feedback_html_body_escapes_user_content() {
        let request = ValidatedFeedbackRequest {
            feedback_type: FeedbackType::Bug,
            email: None,
            content: "<script>alert(\"x\")</script>".to_string(),
            page: None,
            user_agent: None,
        };
        let feedback = FeedbackEmail {
            feedback_id: "feedback-html",
            submitted_at: "2026-06-09T12:00:00Z",
            request: &request,
        };

        let html = feedback_html_body(&feedback);

        assert!(html.contains("&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn parse_host_port_accepts_host_and_port() {
        assert_eq!(
            parse_host_port("smtp.example.com:587").expect("host port should parse"),
            ("smtp.example.com".to_string(), Some(587))
        );
    }

    #[test]
    fn parse_dotenv_values_accepts_spaced_and_quoted_values() {
        let values = parse_dotenv_values(
            r#"
            # comment
            EMAIL = "zack@ksana.net"
            EMAIL_TOKEN='secret-token'
            SMTP_SERVER=smtp.example.com:587 # inline comment
            "#,
        );

        assert_eq!(
            values.get("EMAIL").map(String::as_str),
            Some("zack@ksana.net")
        );
        assert_eq!(
            values.get("EMAIL_TOKEN").map(String::as_str),
            Some("secret-token")
        );
        assert_eq!(
            values.get("SMTP_SERVER").map(String::as_str),
            Some("smtp.example.com:587")
        );
    }

    fn feedback_request(feedback_type: FeedbackType) -> ValidatedFeedbackRequest {
        ValidatedFeedbackRequest {
            feedback_type,
            email: Some("player@example.com".to_string()),
            content: "希望可以导出更多故事片段。".to_string(),
            page: Some("/feedback".to_string()),
            user_agent: Some("unit-test".to_string()),
        }
    }
}

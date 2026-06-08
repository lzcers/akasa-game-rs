use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, params};
use tokio::sync::Mutex;

use crate::api::dto::AnalyticsEventInput;

const CREATE_ANALYTICS_EVENTS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS analytics_events (
    id TEXT PRIMARY KEY,
    event_name TEXT NOT NULL,
    anonymous_user_id TEXT NOT NULL,
    client_session_id TEXT NOT NULL,
    game_session_id TEXT,
    source_session_id TEXT,
    occurred_at TEXT NOT NULL,
    received_at TEXT NOT NULL,
    app TEXT NOT NULL,
    app_version TEXT,
    path TEXT,
    referrer_domain TEXT,
    utm_source TEXT,
    utm_medium TEXT,
    utm_campaign TEXT,
    device_type TEXT,
    platform TEXT,
    properties_json TEXT NOT NULL
);
"#;

const CREATE_ANALYTICS_EVENT_TIME_INDEX_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_analytics_events_name_time
ON analytics_events(event_name, occurred_at);
"#;

const CREATE_ANALYTICS_USER_INDEX_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_analytics_events_user_time
ON analytics_events(anonymous_user_id, occurred_at);
"#;

const CREATE_ANALYTICS_GAME_SESSION_INDEX_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_analytics_events_game_session
ON analytics_events(game_session_id);
"#;

const CREATE_ANALYTICS_SOURCE_SESSION_INDEX_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_analytics_events_source_session
ON analytics_events(source_session_id);
"#;

#[derive(Debug, Clone)]
pub struct AnalyticsRepository {
    db_path: PathBuf,
    write_lock: Arc<Mutex<()>>,
}

impl AnalyticsRepository {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn append_events(&self, events: &[AnalyticsEventInput]) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        let _guard = self.write_lock.lock().await;
        ensure_parent_dir(&self.db_path)?;

        let mut conn = Connection::open(&self.db_path).with_context(|| {
            format!(
                "failed to open analytics sqlite database `{}`",
                self.db_path.display()
            )
        })?;
        init_schema(&conn)?;

        let tx = conn
            .transaction()
            .context("failed to open analytics transaction")?;
        for event in events {
            let received_at = Utc::now().to_rfc3339();
            let properties_json = serde_json::to_string(&event.properties)
                .context("failed to serialize properties")?;
            tx.execute(
                r#"
                INSERT OR IGNORE INTO analytics_events (
                    id,
                    event_name,
                    anonymous_user_id,
                    client_session_id,
                    game_session_id,
                    source_session_id,
                    occurred_at,
                    received_at,
                    app,
                    app_version,
                    path,
                    referrer_domain,
                    utm_source,
                    utm_medium,
                    utm_campaign,
                    device_type,
                    platform,
                    properties_json
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                    ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
                )
                "#,
                params![
                    event.id,
                    event.event_name,
                    event.anonymous_user_id,
                    event.client_session_id,
                    event.game_session_id,
                    event.source_session_id,
                    event.occurred_at,
                    received_at,
                    event.app,
                    event.app_version,
                    event.path,
                    event.referrer_domain,
                    event.utm_source,
                    event.utm_medium,
                    event.utm_campaign,
                    event.device_type,
                    event.platform,
                    properties_json,
                ],
            )
            .with_context(|| format!("failed to insert analytics event `{}`", event.id))?;
        }
        tx.commit()
            .context("failed to commit analytics transaction")?;

        Ok(events.len())
    }
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(CREATE_ANALYTICS_EVENTS_TABLE_SQL)
        .context("failed to create analytics_events table")?;
    conn.execute_batch(CREATE_ANALYTICS_EVENT_TIME_INDEX_SQL)
        .context("failed to create analytics event time index")?;
    conn.execute_batch(CREATE_ANALYTICS_USER_INDEX_SQL)
        .context("failed to create analytics user index")?;
    conn.execute_batch(CREATE_ANALYTICS_GAME_SESSION_INDEX_SQL)
        .context("failed to create analytics game session index")?;
    conn.execute_batch(CREATE_ANALYTICS_SOURCE_SESSION_INDEX_SQL)
        .context("failed to create analytics source session index")?;
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create analytics parent directory `{}`",
                parent.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use uuid::Uuid;

    #[tokio::test]
    async fn append_events_writes_rows_to_sqlite() {
        let db_path = std::env::temp_dir().join(format!(
            "akasa-analytics-repo-{}.sqlite3",
            Uuid::new_v4().simple()
        ));
        let repo = AnalyticsRepository::new(&db_path);
        let event = AnalyticsEventInput {
            id: "evt-test".to_string(),
            event_name: "choice_submitted".to_string(),
            anonymous_user_id: "anon-test".to_string(),
            client_session_id: "visit-test".to_string(),
            game_session_id: Some("session-test".to_string()),
            source_session_id: Some("session-source".to_string()),
            occurred_at: "2026-06-08T12:00:00Z".to_string(),
            app: "game-web".to_string(),
            app_version: Some("test".to_string()),
            path: Some("/play".to_string()),
            referrer_domain: Some("example.com".to_string()),
            utm_source: Some("bilibili".to_string()),
            utm_medium: Some("social".to_string()),
            utm_campaign: Some("demo".to_string()),
            device_type: Some("desktop".to_string()),
            platform: Some("MacIntel".to_string()),
            properties: json!({
                "round": 2,
                "choiceType": "selected_option"
            }),
        };

        let accepted = repo
            .append_events(&[event])
            .await
            .expect("event should write");

        assert_eq!(accepted, 1);

        let conn = Connection::open(&db_path).expect("sqlite db should open");
        let row: (String, String, String) = conn
            .query_row(
                r#"
                SELECT event_name, utm_source, properties_json
                FROM analytics_events
                WHERE id = 'evt-test'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("analytics event should be queryable");

        assert_eq!(row.0, "choice_submitted");
        assert_eq!(row.1, "bilibili");
        assert!(row.2.contains("\"round\":2"));
    }
}

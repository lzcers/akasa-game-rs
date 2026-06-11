use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::{api::site::AnalyticsEventInput, database::AppDatabase};

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
    ip_address TEXT,
    properties_json TEXT NOT NULL
);
"#;

const ADD_ANALYTICS_IP_ADDRESS_COLUMN_SQL: &str = r#"
ALTER TABLE analytics_events ADD COLUMN ip_address TEXT;
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
    db: AppDatabase,
}

impl AnalyticsRepository {
    pub fn new(db: AppDatabase) -> Self {
        Self { db }
    }

    pub async fn append_events(
        &self,
        events: &[AnalyticsEventInput],
        ip_address: Option<&str>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        let _guard = self.db.lock().await;
        let mut conn = self.db.open_connection("analytics")?;
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
                    ip_address,
                    properties_json
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                    ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
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
                    ip_address,
                    properties_json,
                ],
            )
            .with_context(|| format!("failed to insert analytics event `{}`", event.id))?;
        }
        tx.commit()
            .context("failed to commit analytics transaction")?;

        Ok(events.len())
    }

    pub async fn summary(&self, range_hours: u32) -> Result<AnalyticsSummary> {
        let _guard = self.db.lock().await;
        let conn = self.db.open_connection("analytics")?;
        init_schema(&conn)?;

        let range_hours = range_hours.clamp(1, 24 * 30);
        let since = (Utc::now() - Duration::hours(i64::from(range_hours))).to_rfc3339();
        let totals = read_totals(&conn, &since)?;
        let funnel = read_funnel(&conn, &since)?;
        let top_sources = read_top_sources(&conn, &since)?;
        let top_events = read_top_events(&conn, &since)?;
        let recent_events = read_recent_events(&conn, &since)?;

        Ok(AnalyticsSummary {
            range_hours,
            generated_at: Utc::now().to_rfc3339(),
            totals,
            funnel,
            top_sources,
            top_events,
            recent_events,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsSummary {
    pub range_hours: u32,
    pub generated_at: String,
    pub totals: AnalyticsTotals,
    pub funnel: Vec<AnalyticsCount>,
    pub top_sources: Vec<AnalyticsSourceCount>,
    pub top_events: Vec<AnalyticsCount>,
    pub recent_events: Vec<AnalyticsRecentEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsTotals {
    pub events: u64,
    pub unique_users: u64,
    pub visits: u64,
    pub game_sessions: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsCount {
    pub event_name: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsSourceCount {
    pub source_type: String,
    pub source: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsRecentEvent {
    pub occurred_at: String,
    pub event_name: String,
    pub anonymous_user_id: String,
    pub client_session_id: String,
    pub game_session_id: Option<String>,
    pub path: Option<String>,
    pub source: Option<String>,
    pub device_type: Option<String>,
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(CREATE_ANALYTICS_EVENTS_TABLE_SQL)
        .context("failed to create analytics_events table")?;
    if !analytics_events_has_column(conn, "ip_address")? {
        conn.execute_batch(ADD_ANALYTICS_IP_ADDRESS_COLUMN_SQL)
            .context("failed to add analytics_events ip_address column")?;
    }
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

fn analytics_events_has_column(conn: &Connection, column_name: &str) -> Result<bool> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(analytics_events)")
        .context("failed to inspect analytics_events columns")?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .context("failed to query analytics_events columns")?;
    let columns = collect_rows(rows)?;
    Ok(columns.iter().any(|column| column == column_name))
}

fn read_totals(conn: &Connection, since: &str) -> Result<AnalyticsTotals> {
    conn.query_row(
        r#"
        SELECT
            COUNT(*),
            COUNT(DISTINCT anonymous_user_id),
            COUNT(DISTINCT client_session_id),
            COUNT(DISTINCT game_session_id)
        FROM analytics_events
        WHERE occurred_at >= ?1
        "#,
        params![since],
        |row| {
            Ok(AnalyticsTotals {
                events: row.get::<_, i64>(0)? as u64,
                unique_users: row.get::<_, i64>(1)? as u64,
                visits: row.get::<_, i64>(2)? as u64,
                game_sessions: row.get::<_, i64>(3)? as u64,
            })
        },
    )
    .context("failed to read analytics totals")
}

fn read_funnel(conn: &Connection, since: &str) -> Result<Vec<AnalyticsCount>> {
    const FUNNEL_EVENTS: [&str; 6] = [
        "app_opened",
        "creation_submitted",
        "profile_generate_completed",
        "generated_profiles_accepted",
        "round_reached",
        "ending_viewed",
    ];

    let mut counts = Vec::with_capacity(FUNNEL_EVENTS.len());
    for event_name in FUNNEL_EVENTS {
        let count = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM analytics_events
                WHERE occurred_at >= ?1 AND event_name = ?2
                "#,
                params![since, event_name],
                |row| row.get::<_, i64>(0),
            )
            .with_context(|| format!("failed to read analytics funnel event `{event_name}`"))?;
        counts.push(AnalyticsCount {
            event_name: event_name.to_string(),
            count: count as u64,
        });
    }
    Ok(counts)
}

fn read_top_sources(conn: &Connection, since: &str) -> Result<Vec<AnalyticsSourceCount>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT source_type, source, COUNT(*) AS event_count
            FROM (
                SELECT
                    CASE
                        WHEN NULLIF(utm_source, '') IS NOT NULL THEN 'utm_source'
                        WHEN NULLIF(referrer_domain, '') IS NOT NULL THEN 'referrer_domain'
                        WHEN NULLIF(source_session_id, '') IS NOT NULL THEN 'source_session'
                        ELSE 'direct'
                    END AS source_type,
                    CASE
                        WHEN NULLIF(utm_source, '') IS NOT NULL THEN utm_source
                        WHEN NULLIF(referrer_domain, '') IS NOT NULL THEN referrer_domain
                        WHEN NULLIF(source_session_id, '') IS NOT NULL THEN source_session_id
                        ELSE 'direct'
                    END AS source
                FROM analytics_events
                WHERE occurred_at >= ?1
            )
            GROUP BY source_type, source
            ORDER BY event_count DESC, source ASC
            LIMIT 10
            "#,
        )
        .context("failed to prepare analytics top sources query")?;

    let rows = stmt
        .query_map(params![since], |row| {
            Ok(AnalyticsSourceCount {
                source_type: row.get(0)?,
                source: row.get(1)?,
                count: row.get::<_, i64>(2)? as u64,
            })
        })
        .context("failed to read analytics top sources")?;

    collect_rows(rows)
}

fn read_top_events(conn: &Connection, since: &str) -> Result<Vec<AnalyticsCount>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT event_name, COUNT(*) AS event_count
            FROM analytics_events
            WHERE occurred_at >= ?1
            GROUP BY event_name
            ORDER BY event_count DESC, event_name ASC
            LIMIT 12
            "#,
        )
        .context("failed to prepare analytics top events query")?;

    let rows = stmt
        .query_map(params![since], |row| {
            Ok(AnalyticsCount {
                event_name: row.get(0)?,
                count: row.get::<_, i64>(1)? as u64,
            })
        })
        .context("failed to read analytics top events")?;

    collect_rows(rows)
}

fn read_recent_events(conn: &Connection, since: &str) -> Result<Vec<AnalyticsRecentEvent>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT
                occurred_at,
                event_name,
                anonymous_user_id,
                client_session_id,
                game_session_id,
                path,
                COALESCE(NULLIF(utm_source, ''), NULLIF(referrer_domain, ''), NULLIF(source_session_id, '')),
                device_type
            FROM analytics_events
            WHERE occurred_at >= ?1
            ORDER BY occurred_at DESC
            LIMIT 25
            "#,
        )
        .context("failed to prepare analytics recent events query")?;

    let rows = stmt
        .query_map(params![since], |row| {
            Ok(AnalyticsRecentEvent {
                occurred_at: row.get(0)?,
                event_name: row.get(1)?,
                anonymous_user_id: row.get(2)?,
                client_session_id: row.get(3)?,
                game_session_id: row.get(4)?,
                path: row.get(5)?,
                source: row.get(6)?,
                device_type: row.get(7)?,
            })
        })
        .context("failed to read analytics recent events")?;

    collect_rows(rows)
}

fn collect_rows<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> Result<Vec<T>> {
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to collect analytics rows")
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
        let repo = AnalyticsRepository::new(AppDatabase::new(&db_path));
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
            .append_events(&[event], Some("203.0.113.42"))
            .await
            .expect("event should write");

        assert_eq!(accepted, 1);

        let conn = Connection::open(&db_path).expect("sqlite db should open");
        let row: (String, String, String, String) = conn
            .query_row(
                r#"
                SELECT event_name, utm_source, ip_address, properties_json
                FROM analytics_events
                WHERE id = 'evt-test'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("analytics event should be queryable");

        assert_eq!(row.0, "choice_submitted");
        assert_eq!(row.1, "bilibili");
        assert_eq!(row.2, "203.0.113.42");
        assert!(row.3.contains("\"round\":2"));
    }

    #[tokio::test]
    async fn summary_returns_recent_analytics_rollups() {
        let db_path = std::env::temp_dir().join(format!(
            "akasa-analytics-summary-{}.sqlite3",
            Uuid::new_v4().simple()
        ));
        let repo = AnalyticsRepository::new(AppDatabase::new(&db_path));
        let now = Utc::now().to_rfc3339();
        let old = (Utc::now() - Duration::hours(48)).to_rfc3339();
        let events = vec![
            AnalyticsEventInput {
                id: "evt-open".to_string(),
                event_name: "app_opened".to_string(),
                anonymous_user_id: "anon-one".to_string(),
                client_session_id: "visit-one".to_string(),
                game_session_id: None,
                source_session_id: None,
                occurred_at: now.clone(),
                app: "game-web".to_string(),
                app_version: Some("test".to_string()),
                path: Some("/".to_string()),
                referrer_domain: None,
                utm_source: Some("bilibili".to_string()),
                utm_medium: Some("social".to_string()),
                utm_campaign: None,
                device_type: Some("desktop".to_string()),
                platform: Some("MacIntel".to_string()),
                properties: json!({}),
            },
            AnalyticsEventInput {
                id: "evt-create".to_string(),
                event_name: "creation_submitted".to_string(),
                anonymous_user_id: "anon-one".to_string(),
                client_session_id: "visit-one".to_string(),
                game_session_id: Some("session-one".to_string()),
                source_session_id: None,
                occurred_at: now.clone(),
                app: "game-web".to_string(),
                app_version: Some("test".to_string()),
                path: Some("/creation".to_string()),
                referrer_domain: None,
                utm_source: Some("bilibili".to_string()),
                utm_medium: Some("social".to_string()),
                utm_campaign: None,
                device_type: Some("desktop".to_string()),
                platform: Some("MacIntel".to_string()),
                properties: json!({}),
            },
            AnalyticsEventInput {
                id: "evt-old".to_string(),
                event_name: "ending_viewed".to_string(),
                anonymous_user_id: "anon-old".to_string(),
                client_session_id: "visit-old".to_string(),
                game_session_id: Some("session-old".to_string()),
                source_session_id: None,
                occurred_at: old,
                app: "game-web".to_string(),
                app_version: Some("test".to_string()),
                path: Some("/ending".to_string()),
                referrer_domain: Some("example.com".to_string()),
                utm_source: None,
                utm_medium: None,
                utm_campaign: None,
                device_type: Some("mobile".to_string()),
                platform: Some("iPhone".to_string()),
                properties: json!({}),
            },
        ];

        repo.append_events(&events, None)
            .await
            .expect("events should write");

        let summary = repo.summary(24).await.expect("summary should read");

        assert_eq!(summary.range_hours, 24);
        assert_eq!(summary.totals.events, 2);
        assert_eq!(summary.totals.unique_users, 1);
        assert_eq!(summary.totals.visits, 1);
        assert_eq!(summary.totals.game_sessions, 1);
        assert_eq!(summary.funnel[0].event_name, "app_opened");
        assert_eq!(summary.funnel[0].count, 1);
        assert_eq!(summary.funnel[1].event_name, "creation_submitted");
        assert_eq!(summary.funnel[1].count, 1);
        assert_eq!(summary.top_sources[0].source_type, "utm_source");
        assert_eq!(summary.top_sources[0].source, "bilibili");
        assert_eq!(summary.top_sources[0].count, 2);
        assert_eq!(summary.recent_events.len(), 2);
        assert!(
            summary
                .recent_events
                .iter()
                .all(|event| event.event_name != "ending_viewed")
        );
    }
}

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use tokio::sync::Mutex;

const CREATE_GAME_SESSION_ARCHIVES_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS game_session_archives (
    session_id TEXT PRIMARY KEY,
    compressed_archive TEXT NOT NULL,
    title TEXT,
    phase TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_accessed_at TEXT NOT NULL
);
"#;

#[derive(Debug, Clone)]
pub struct SessionArchiveRepository {
    db_path: PathBuf,
    write_lock: Arc<Mutex<()>>,
}

#[derive(Debug, Clone)]
pub struct StoredSessionArchive {
    pub session_id: String,
    pub compressed_archive: String,
    pub title: Option<String>,
    pub phase: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed_at: String,
}

impl SessionArchiveRepository {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn save_archive(&self, archive: StoredSessionArchive) -> Result<()> {
        let _guard = self.write_lock.lock().await;
        ensure_parent_dir(&self.db_path)?;

        let conn = open_connection(&self.db_path)?;
        init_schema(&conn)?;
        conn.execute(
            r#"
            INSERT INTO game_session_archives (
                session_id,
                compressed_archive,
                title,
                phase,
                created_at,
                updated_at,
                last_accessed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(session_id) DO UPDATE SET
                compressed_archive = excluded.compressed_archive,
                title = excluded.title,
                phase = excluded.phase,
                updated_at = excluded.updated_at,
                last_accessed_at = excluded.last_accessed_at
            "#,
            params![
                archive.session_id,
                archive.compressed_archive,
                archive.title,
                archive.phase,
                archive.created_at,
                archive.updated_at,
                archive.last_accessed_at,
            ],
        )
        .context("failed to upsert game session archive")?;

        Ok(())
    }

    pub async fn load_archive(&self, session_id: &str) -> Result<Option<StoredSessionArchive>> {
        let _guard = self.write_lock.lock().await;
        ensure_parent_dir(&self.db_path)?;

        let conn = open_connection(&self.db_path)?;
        init_schema(&conn)?;
        conn.query_row(
            r#"
            SELECT
                session_id,
                compressed_archive,
                title,
                phase,
                created_at,
                updated_at,
                last_accessed_at
            FROM game_session_archives
            WHERE session_id = ?1
            "#,
            params![session_id],
            |row| {
                Ok(StoredSessionArchive {
                    session_id: row.get(0)?,
                    compressed_archive: row.get(1)?,
                    title: row.get(2)?,
                    phase: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    last_accessed_at: row.get(6)?,
                })
            },
        )
        .optional()
        .context("failed to load game session archive")
    }
}

fn open_connection(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path).with_context(|| {
        format!(
            "failed to open session archive sqlite database `{}`",
            db_path.display()
        )
    })?;
    conn.busy_timeout(Duration::from_secs(5))
        .context("failed to configure session archive sqlite busy timeout")?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("failed to enable WAL for session archive sqlite database")?;
    Ok(conn)
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(CREATE_GAME_SESSION_ARCHIVES_TABLE_SQL)
        .context("failed to initialize game session archive schema")
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create session archive database directory `{}`",
                parent.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn save_archive_upserts_and_loads_by_session_id() {
        let repo = test_repo();
        let archive = StoredSessionArchive {
            session_id: "session-one".to_string(),
            compressed_archive: "first".to_string(),
            title: Some("First".to_string()),
            phase: "awaiting_player".to_string(),
            created_at: "2026-06-10T00:00:00Z".to_string(),
            updated_at: "2026-06-10T00:00:00Z".to_string(),
            last_accessed_at: "2026-06-10T00:00:00Z".to_string(),
        };

        repo.save_archive(archive.clone())
            .await
            .expect("archive should save");
        repo.save_archive(StoredSessionArchive {
            compressed_archive: "second".to_string(),
            updated_at: "2026-06-10T00:01:00Z".to_string(),
            ..archive
        })
        .await
        .expect("archive should update");

        let loaded = repo
            .load_archive("session-one")
            .await
            .expect("archive should load")
            .expect("archive should exist");

        assert_eq!(loaded.compressed_archive, "second");
        assert_eq!(loaded.title.as_deref(), Some("First"));
        assert_eq!(loaded.updated_at, "2026-06-10T00:01:00Z");
    }

    fn test_repo() -> SessionArchiveRepository {
        SessionArchiveRepository::new(std::env::temp_dir().join(format!(
            "akasa-session-archives-{}.sqlite3",
            Uuid::new_v4().simple()
        )))
    }
}

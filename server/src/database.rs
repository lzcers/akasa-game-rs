use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use rusqlite::Connection;
use tokio::sync::{Mutex, MutexGuard};

#[derive(Debug, Clone)]
pub struct AppDatabase {
    db_path: PathBuf,
    access_lock: Arc<Mutex<()>>,
}

impl AppDatabase {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
            access_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn lock(&self) -> MutexGuard<'_, ()> {
        self.access_lock.lock().await
    }

    pub fn open_connection(&self, label: &str) -> Result<Connection> {
        ensure_parent_dir(&self.db_path)?;
        let conn = Connection::open(&self.db_path).with_context(|| {
            format!(
                "failed to open {label} sqlite database `{}`",
                self.db_path.display()
            )
        })?;
        conn.busy_timeout(Duration::from_secs(5))
            .with_context(|| format!("failed to configure {label} sqlite busy timeout"))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .with_context(|| format!("failed to enable WAL for {label} sqlite database"))?;
        Ok(conn)
    }
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create sqlite database directory `{}`",
                parent.display()
            )
        })?;
    }
    Ok(())
}

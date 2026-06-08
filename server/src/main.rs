mod analytics;
mod api;
mod error;
mod state;

use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::Context;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{api::build_router, state::AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let options = ServerOptions::from_env_and_args();
    let state = AppState::new(default_analytics_events_path(), options.local_debug);

    let app = build_router(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], 3001));
    info!("akashic-server listening on {}", addr);
    if options.local_debug {
        info!("local debug enabled: stream chunk printing and context export are active");
    }

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind to {}", addr))?;

    axum::serve(listener, app)
        .await
        .context("failed to start akashic-server")?;

    Ok(())
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "akasa_server=info,axum=info,tower_http=info".into());

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .compact(),
        )
        .init();
}

fn default_analytics_events_path() -> PathBuf {
    PathBuf::from("db-data/analytics.sqlite3")
}

#[derive(Debug, Default)]
struct ServerOptions {
    local_debug: bool,
}

impl ServerOptions {
    fn from_env_and_args() -> Self {
        let mut options = Self {
            local_debug: env_flag("AKASA_LOCAL_DEBUG") || env_flag("AKASA_SERVER_LOCAL_DEBUG"),
        };

        for arg in env::args().skip(1) {
            match arg.as_str() {
                "--local-debug" => options.local_debug = true,
                "--no-local-debug" => options.local_debug = false,
                _ => {}
            }
        }

        options
    }
}

fn env_flag(key: &str) -> bool {
    env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

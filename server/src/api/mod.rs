pub mod archive;
pub mod creation;
pub mod dto;
pub mod game_sessions;
pub mod profiles;
pub mod site;

use std::time::Instant;

use axum::{
    Router,
    extract::Request,
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use tracing::info;

use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/analytics/events", post(site::record_analytics_events))
        .route("/api/feedback", post(site::submit_feedback))
        .route("/internal/analytics/data", get(site::get_analytics_summary))
        .route(
            "/api/creation/generate",
            post(creation::generate_creation_draft),
        )
        .route("/api/profiles/generate", post(profiles::generate_profiles))
        .route(
            "/api/game-sessions/create",
            post(game_sessions::create_game_session),
        )
        .route(
            "/api/game-sessions/{session_id}",
            get(game_sessions::get_game_session_world),
        )
        .route(
            "/api/game-sessions/{session_id}/clone",
            post(game_sessions::clone_game_session),
        )
        .route(
            "/api/game-sessions/{session_id}/save-export",
            post(game_sessions::save_export),
        )
        .route(
            "/api/game-sessions/{session_id}/summary",
            post(game_sessions::generate_story_summary),
        )
        .route(
            "/api/game-sessions/load-archive",
            post(game_sessions::load_archive),
        )
        .route(
            "/api/game-sessions/{session_id}/control",
            post(game_sessions::control_game_session),
        )
        .route(
            "/api/game-sessions/{session_id}/stream",
            get(game_sessions::stream_game_session),
        )
        .route_layer(middleware::from_fn(log_api_request))
        .with_state(state)
}

async fn log_api_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let started_at = Instant::now();

    info!("api request started: {} {}", method, uri);
    let response = next.run(request).await;
    let status = response.status();
    let elapsed = started_at.elapsed().as_millis();
    info!(
        "api request finished: {} {} -> {} ({} ms)",
        method, uri, status, elapsed
    );

    response
}

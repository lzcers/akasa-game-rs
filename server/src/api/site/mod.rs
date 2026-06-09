mod dto;

pub use dto::*;

use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;

use crate::{analytics::AnalyticsSummary, error::AppError, state::AppState};

use super::dto::ApiResponse;

type ApiResult<T> = Result<Json<ApiResponse<T>>, AppError>;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsSummaryQuery {
    range_hours: Option<u32>,
}

pub async fn record_analytics_events(
    State(state): State<AppState>,
    Json(request): Json<AnalyticsBatchRequest>,
) -> ApiResult<AnalyticsBatchData> {
    let accepted = state.record_analytics_events(request).await?;
    Ok(Json(ApiResponse::ok(AnalyticsBatchData { accepted })))
}

pub async fn get_analytics_summary(
    State(state): State<AppState>,
    Query(query): Query<AnalyticsSummaryQuery>,
) -> ApiResult<AnalyticsSummary> {
    let summary = state
        .analytics_summary(query.range_hours.unwrap_or(24))
        .await?;
    Ok(Json(ApiResponse::ok(summary)))
}

pub async fn submit_feedback(
    State(state): State<AppState>,
    Json(request): Json<SubmitFeedbackRequest>,
) -> ApiResult<SubmitFeedbackData> {
    let request = request.validate()?;
    let submitted = state.send_feedback(request).await?;
    Ok(Json(ApiResponse::ok(submitted)))
}

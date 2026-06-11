mod dto;

pub use dto::*;

use std::net::{IpAddr, SocketAddr};

use axum::{
    Json,
    extract::{ConnectInfo, Query, State},
    http::HeaderMap,
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
    ConnectInfo(remote_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<AnalyticsBatchRequest>,
) -> ApiResult<AnalyticsBatchData> {
    let ip_address = client_ip_address(&headers, Some(remote_addr));
    let accepted = state.record_analytics_events(request, ip_address).await?;
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

fn client_ip_address(headers: &HeaderMap, remote_addr: Option<SocketAddr>) -> Option<String> {
    [
        "cf-connecting-ip",
        "x-real-ip",
        "x-forwarded-for",
        "forwarded",
    ]
    .into_iter()
    .filter_map(|name| headers.get(name))
    .filter_map(|value| value.to_str().ok())
    .flat_map(candidate_ips_from_header)
    .find_map(normalize_ip_address)
    .or_else(|| remote_addr.map(|addr| addr.ip().to_string()))
}

fn candidate_ips_from_header(value: &str) -> Vec<&str> {
    value
        .split(',')
        .flat_map(|part| part.split(';'))
        .filter_map(|part| {
            let part = part.trim();
            let candidate = part.strip_prefix("for=").unwrap_or(part).trim_matches('"');
            (!candidate.is_empty()).then_some(candidate)
        })
        .collect()
}

fn normalize_ip_address(value: &str) -> Option<String> {
    let value = value.trim().trim_matches('"');
    let value = value
        .strip_prefix('[')
        .and_then(|value| value.split_once(']').map(|(ip, _)| ip))
        .unwrap_or(value);
    let value = if let Some((host, _port)) = value.rsplit_once(':') {
        if host.contains(':') { value } else { host }
    } else {
        value
    };
    value.parse::<IpAddr>().ok().map(|ip| ip.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn client_ip_address_prefers_proxy_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.9, 10.0.0.2"),
        );

        let ip = client_ip_address(&headers, Some(SocketAddr::from(([127, 0, 0, 1], 3001))));

        assert_eq!(ip.as_deref(), Some("203.0.113.9"));
    }

    #[test]
    fn client_ip_address_falls_back_to_remote_addr() {
        let ip = client_ip_address(
            &HeaderMap::new(),
            Some(SocketAddr::from(([127, 0, 0, 1], 3001))),
        );

        assert_eq!(ip.as_deref(), Some("127.0.0.1"));
    }
}

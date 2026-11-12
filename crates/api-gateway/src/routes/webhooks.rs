//! /api/v1/sec/webhooks：订阅管理（sec:manage）。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use core_events::WebhookRepo;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
pub struct CreateWebhookReq {
    pub url: String,
    /// 订阅的 topic 列表（如 ["doc.released"]）
    pub topics: Vec<String>,
    /// HMAC-SHA256 签名密钥
    pub secret: String,
}

#[derive(Serialize, ToSchema)]
pub struct WebhookDto {
    pub id: Uuid,
    pub url: String,
    pub topics: Vec<String>,
}

/// POST /api/v1/sec/webhooks（sec:manage）
#[utoipa::path(
    post, path = "/api/v1/sec/webhooks",
    request_body = CreateWebhookReq,
    responses((status = 201, body = WebhookDto), (status = 403))
)]
pub async fn create_webhook(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Json(req): Json<CreateWebhookReq>,
) -> Result<(StatusCode, Json<WebhookDto>), ApiError> {
    if req.url.trim().is_empty() || req.topics.is_empty() || req.secret.trim().is_empty() {
        return Err(ApiError::new(
            core_model::DomainError::FieldRequired("url/topics/secret".into()),
            &tid.0,
        ));
    }
    let repo = WebhookRepo::new(st.pool.clone());
    let id = repo
        .create(&req.url, &req.topics, &req.secret)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok((
        StatusCode::CREATED,
        Json(WebhookDto {
            id,
            url: req.url,
            topics: req.topics,
        }),
    ))
}

/// GET /api/v1/sec/webhooks（sec:manage）
#[utoipa::path(get, path = "/api/v1/sec/webhooks", responses((status = 200, body = [WebhookDto])))]
pub async fn list_webhooks(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
) -> Result<Json<Vec<WebhookDto>>, ApiError> {
    let repo = WebhookRepo::new(st.pool.clone());
    let subs = repo.list().await.map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(Json(
        subs.into_iter()
            .map(|s| WebhookDto {
                id: s.id,
                url: s.url,
                topics: s.topics,
            })
            .collect(),
    ))
}

/// DELETE /api/v1/sec/webhooks/{id}（sec:manage）
#[utoipa::path(
    delete, path = "/api/v1/sec/webhooks/{id}",
    params(("id" = Uuid, Path)),
    responses((status = 204))
)]
pub async fn delete_webhook(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let repo = WebhookRepo::new(st.pool.clone());
    repo.delete(id)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/sec/webhooks/dead — 死信事件（sec:manage）
#[utoipa::path(get, path = "/api/v1/sec/webhooks/dead", responses((status = 200)))]
pub async fn dead_events(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows: Vec<(i64, String, serde_json::Value)> = sqlx::query_as(
        "SELECT id, topic, payload FROM event_outbox WHERE status = 2 ORDER BY id DESC LIMIT 100",
    )
    .fetch_all(&st.pool)
    .await
    .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    Ok(Json(serde_json::json!({
        "items": rows.into_iter().map(|(id, topic, payload)| {
            serde_json::json!({"id": id, "topic": topic, "payload": payload})
        }).collect::<Vec<_>>()
    })))
}

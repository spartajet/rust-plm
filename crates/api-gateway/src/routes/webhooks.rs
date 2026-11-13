//! /api/v1/sec/webhooks：订阅管理（sec:manage）。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
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
    // 强校验（增量13：增量4 债清偿）——防拼写错 topic 静默零投递 / 弱 HMAC 密钥 / 非 http(s) 端点
    const KNOWN_TOPICS: [&str; 4] = [
        "doc.released",
        "doc.revised",
        "doc.submitted",
        "doc.checked",
    ];
    // 终审 I1/I2 重写：strip_prefix 取余部（不裸字节下标——http:// 仅 7 字节/多字节字符会越界 panic），
    // host 段（余部至首个 '/'）必须含 '.'——语义=域名限定（终审 I2 false-accept 关闭）
    let url_trimmed = req.url.trim();
    let secret_trimmed = req.secret.trim();
    if url_trimmed.is_empty() || req.topics.is_empty() || secret_trimmed.is_empty() {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest("url/topics/secret 必填".into()),
            &tid.0,
        ));
    }
    let host_ok = match url_trimmed
        .strip_prefix("https://")
        .or_else(|| url_trimmed.strip_prefix("http://"))
    {
        Some(rest) => {
            let host = rest.split('/').next().unwrap_or("");
            !host.is_empty() && host.contains('.')
        }
        None => false,
    };
    if !host_ok {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest("url 须为 http(s):// 且 host 为含点的域名".into()),
            &tid.0,
        ));
    }
    if secret_trimmed.chars().count() < 8 {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest("secret 须 ≥ 8 字符（HMAC 密钥强度下限）".into()),
            &tid.0,
        ));
    }
    if req
        .topics
        .iter()
        .any(|t| !KNOWN_TOPICS.contains(&t.as_str()))
    {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest(format!("topics 须属于 {:?}", KNOWN_TOPICS)),
            &tid.0,
        ));
    }
    let repo = WebhookRepo::new(st.pool.clone());
    let id = repo
        .create(url_trimmed, &req.topics, secret_trimmed)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok((
        StatusCode::CREATED,
        Json(WebhookDto {
            id,
            url: url_trimmed.to_string(),
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

/// 死信分页查询参数（增量13：增量4 债清偿）
#[derive(Deserialize)]
pub struct DeadQuery {
    page: Option<u32>,
    size: Option<u32>,
}

/// GET /api/v1/sec/webhooks/dead?page&size — 死信分页（增量13：增量4 债清偿）
#[utoipa::path(
    get, path = "/api/v1/sec/webhooks/dead",
    params(("page" = u32, Query), ("size" = u32, Query, description = "缺省 50，上限 200")),
    responses((status = 200, description = "{items,total,page,size}；items 含 attempts/error"))
)]
pub async fn dead_events(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Query(q): Query<DeadQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let page = q.page.unwrap_or(1).max(1);
    let size = q.size.unwrap_or(50).clamp(1, 200);
    let rows: Vec<(i64, String, serde_json::Value, i32, Option<String>)> = sqlx::query_as(
        "SELECT id, topic, payload, attempts, error FROM event_outbox          WHERE status = 2 ORDER BY id DESC LIMIT $1 OFFSET $2",
    )
    .bind(size as i64)
    .bind(i64::from(page - 1) * i64::from(size))
    .fetch_all(&st.pool)
    .await
    .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM event_outbox WHERE status = 2")
        .fetch_one(&st.pool)
        .await
        .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    Ok(Json(serde_json::json!({
        "items": rows.into_iter().map(|(id, topic, payload, attempts, error)| {
            serde_json::json!({"id": id, "topic": topic, "payload": payload, "attempts": attempts, "error": error})
        }).collect::<Vec<_>>(),
        "total": total,
        "page": page,
        "size": size,
    })))
}

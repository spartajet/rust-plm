//! /api/v1/audit — 审计检索（增量18 SYS-01，sec:manage；append-only 只读）。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::Extension;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct AuditQuery {
    username: Option<String>,
    action: Option<String>,
    object: Option<String>,
    from: Option<String>,
    to: Option<String>,
    page: Option<u32>,
    size: Option<u32>,
}

/// GET /api/v1/audit — 审计多维检索（按人/动作/对象/时间窗；分页信封）
#[utoipa::path(
    get, path = "/api/v1/audit",
    params(("username" = String, Query), ("action" = String, Query), ("object" = String, Query), ("from" = String, Query, description = "ISO8601 起"), ("to" = String, Query), ("page" = u32, Query), ("size" = u32, Query, description = "缺省 50，上限 200")),
    responses((status = 200, description = "{items,total,page,size}"), (status = 403))
)]
pub async fn search_audit(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let parse_ts = |s: &Option<String>| -> Result<Option<chrono::DateTime<chrono::Utc>>, ApiError> {
        match s.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(v) => chrono::DateTime::parse_from_rfc3339(v)
                .map(|d| Some(d.with_timezone(&chrono::Utc)))
                .map_err(|e| {
                    ApiError::new(
                        core_model::DomainError::BadRequest(format!("时间格式须 RFC3339: {e}")),
                        &tid.0,
                    )
                }),
            None => Ok(None),
        }
    };
    let from = parse_ts(&q.from)?;
    let to = parse_ts(&q.to)?;
    let (items, total) = st
        .audit
        .search(
            q.username.as_deref(),
            q.action.as_deref(),
            q.object.as_deref(),
            from,
            to,
            q.page.unwrap_or(1).max(1),
            q.size.unwrap_or(50).clamp(1, 200),
        )
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(Json(json!({
        "items": items,
        "total": total,
        "page": q.page.unwrap_or(1),
        "size": q.size.unwrap_or(50),
    })))
}

//! /api/v1/docs/{obj_no}/lock 与 checkout/checkin 路由（增量26 DOC-03 检入检出）。
//! 门控 docs:write（检出/检入=编辑权协调）；lock GET=docs:read；DELETE（强制回收）=sec:manage。
//! audit 显式三行：doc.checkout / doc.checkin / doc.force-unlock。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use core_sec::AuthUser;
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct LockDto {
    pub locked: bool,
    pub by: Option<String>,
    pub until: Option<String>,
}

/// GET /api/v1/docs/{obj_no}/lock — 锁状态（docs:read；ABAC 越级 404）
#[utoipa::path(
    get, path = "/api/v1/docs/{obj_no}/lock",
    params(("obj_no" = String, Path)),
    responses((status = 200, body = LockDto), (status = 404))
)]
pub async fn get_lock(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(obj_no): Path<String>,
) -> Result<Json<LockDto>, ApiError> {
    // ABAC：越级 404 不泄露存在性（与 detail 同语义）
    let d = st
        .service
        .detail(&obj_no)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    if d.object.secret_level > user.max_secret_level {
        return Err(ApiError::new(
            core_model::DomainError::ObjNotFound(format!("docs:{obj_no}")),
            &tid.0,
        ));
    }
    let info = st
        .service
        .lock_info(&obj_no)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(Json(match info {
        Some(l) => LockDto {
            locked: true,
            by: Some(l.by_name),
            until: Some(l.locked_until.to_rfc3339()),
        },
        None => LockDto {
            locked: false,
            by: None,
            until: None,
        },
    }))
}

/// POST /api/v1/docs/{obj_no}/checkout — 检出（docs:write；独占）
#[utoipa::path(
    post, path = "/api/v1/docs/{obj_no}/checkout",
    params(("obj_no" = String, Path)),
    responses((status = 200, description = "检出成功"), (status = 400, description = "非 working 态"), (status = 404), (status = 409, description = "已被他人检出"))
)]
pub async fn checkout_doc(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(obj_no): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let d = st
        .service
        .detail(&obj_no)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    if d.object.secret_level > user.max_secret_level {
        return Err(ApiError::new(
            core_model::DomainError::ObjNotFound(format!("docs:{obj_no}")),
            &tid.0,
        ));
    }
    let hours = st
        .sec
        .param_i64("checkout_hours", 24)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    let l = st
        .service
        .checkout(&obj_no, user.id, &user.username, hours)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    st.audit.record(
        user.username.clone(),
        "doc.checkout",
        Some(obj_no.clone()),
        Some(json!({"until": l.locked_until.to_rfc3339()})),
        Some(tid.0.clone()),
        None,
    );
    Ok(Json(
        json!({"by": l.by_name, "until": l.locked_until.to_rfc3339()}),
    ))
}

/// POST /api/v1/docs/{obj_no}/checkin — 检入（docs:write；仅持有者）
#[utoipa::path(
    post, path = "/api/v1/docs/{obj_no}/checkin",
    params(("obj_no" = String, Path)),
    responses((status = 204), (status = 400, description = "未被检出"), (status = 404), (status = 409, description = "他人持有"))
)]
pub async fn checkin_doc(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(obj_no): Path<String>,
) -> Result<StatusCode, ApiError> {
    st.service
        .checkin(&obj_no, user.id)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    st.audit.record(
        user.username.clone(),
        "doc.checkin",
        Some(obj_no.clone()),
        None,
        Some(tid.0.clone()),
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/docs/{obj_no}/lock — 强制回收（sec:manage）
#[utoipa::path(
    delete, path = "/api/v1/docs/{obj_no}/lock",
    params(("obj_no" = String, Path)),
    responses((status = 204), (status = 404), (status = 403))
)]
pub async fn force_unlock_doc(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(obj_no): Path<String>,
) -> Result<StatusCode, ApiError> {
    let had = st
        .service
        .force_unlock(&obj_no)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    st.audit.record(
        user.username.clone(),
        "doc.force-unlock",
        Some(obj_no.clone()),
        Some(json!({"had_lock": had})),
        Some(tid.0.clone()),
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}

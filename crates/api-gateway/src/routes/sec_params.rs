//! /api/v1/sec/params 路由（增量23 SYS-04 最小参数中心）：密码策略与锁定参数集中管理。
//! 门控 sec:manage；PUT 带 audit 显式留痕（SYS-04"修改留痕"）。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use core_sec::AuthUser;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// 参数行（updated_by 展开用户名）
#[derive(Serialize, ToSchema)]
pub struct SysParamDto {
    pub key: String,
    pub value: String,
    pub updated_by: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// 参数键值域白名单（终审基调：服务端把关，非法值 400）
const RANGES: [(&str, i64, i64); 3] = [
    ("password_min_length", 8, 64),
    ("lock_threshold", 3, 20),
    ("lock_minutes", 1, 1440),
];

#[derive(Deserialize, ToSchema)]
pub struct SetParamReq {
    pub value: i64,
}

/// GET /api/v1/sec/params — 参数全量（sec:manage）
#[utoipa::path(
    get, path = "/api/v1/sec/params",
    responses((status = 200, body = Vec<SysParamDto>), (status = 403))
)]
pub async fn list_params(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
) -> Result<Json<Vec<SysParamDto>>, ApiError> {
    let rows = st
        .sec
        .params()
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    // updated_by uuid → username（批量查）
    let ids: Vec<uuid::Uuid> = rows.iter().filter_map(|(_, _, u, _)| *u).collect();
    let mut names: std::collections::HashMap<uuid::Uuid, String> = Default::default();
    if !ids.is_empty() {
        let pairs: Vec<(uuid::Uuid, String)> =
            sqlx::query_as("SELECT id, username FROM sec_user WHERE id = ANY($1)")
                .bind(&ids)
                .fetch_all(&st.pool)
                .await
                .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
        names.extend(pairs);
    }
    Ok(Json(
        rows.into_iter()
            .map(|(key, value, ub, at)| SysParamDto {
                key,
                value,
                updated_by: ub.and_then(|u| names.get(&u).cloned()),
                updated_at: at,
            })
            .collect(),
    ))
}

/// PUT /api/v1/sec/params/{key} — 改参数（值域白名单校验；audit sec-param.change 留痕）
#[utoipa::path(
    put, path = "/api/v1/sec/params/{key}",
    params(("key" = String, Path)),
    request_body = SetParamReq,
    responses((status = 204), (status = 400, description = "未知键/值越界"), (status = 403))
)]
pub async fn set_param(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(key): Path<String>,
    Json(req): Json<SetParamReq>,
) -> Result<StatusCode, ApiError> {
    let (_, lo, hi) = RANGES.iter().find(|(k, _, _)| *k == key).ok_or_else(|| {
        ApiError::new(
            core_model::DomainError::BadRequest(format!("未知参数键: {key}")),
            &tid.0,
        )
    })?;
    if req.value < *lo || req.value > *hi {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest(format!(
                "{key} 值域 [{lo}, {hi}]，得到 {}",
                req.value
            )),
            &tid.0,
        ));
    }
    let old = st
        .sec
        .set_param(&key, req.value, Some(user.id))
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    // SYS-04 修改留痕（增量23）：旧值/新值/操作人
    st.audit.record(
        user.username.clone(),
        "sec-param.change",
        Some(key.clone()),
        Some(serde_json::json!({"from": old, "to": req.value.to_string()})),
        Some(tid.0.clone()),
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}

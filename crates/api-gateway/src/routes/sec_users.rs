//! /api/v1/sec：用户管理（建号/授权/回收）与角色查询——全部要求 sec:manage。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::Query;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
pub struct CreateUserReq {
    pub username: String,
    pub password: String,
    pub display_name: Option<String>,
    /// 缺省 ["viewer"]
    #[serde(default)]
    pub roles: Vec<String>,
    /// 0 公开 / 1 内部 / 2 秘密 / 3 机密；缺省 1
    #[serde(default = "default_lvl")]
    pub max_secret_level: i16,
}

fn default_lvl() -> i16 {
    1
}

#[derive(Serialize, ToSchema)]
pub struct UserCreated {
    pub id: Uuid,
    pub username: String,
    pub roles: Vec<String>,
}

/// POST /api/v1/sec/users（sec:manage）
#[utoipa::path(
    post, path = "/api/v1/sec/users",
    request_body = CreateUserReq,
    responses((status = 201, body = UserCreated), (status = 403), (status = 409))
)]
pub async fn create_user(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Json(req): Json<CreateUserReq>,
) -> Result<(StatusCode, Json<UserCreated>), ApiError> {
    if req.username.trim().is_empty() || req.password.len() < 8 {
        return Err(ApiError::new(
            core_model::DomainError::FieldRequired("username 与 password(≥8位)".into()),
            &tid.0,
        ));
    }
    let role_refs: Vec<&str> = req.roles.iter().map(|s| s.as_str()).collect();
    let default_roles = ["viewer"];
    let roles: &[&str] = if role_refs.is_empty() {
        &default_roles
    } else {
        &role_refs
    };
    let id = st
        .sec
        .create_user(
            &req.username,
            &req.password,
            req.display_name.as_deref(),
            roles,
            req.max_secret_level,
        )
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok((
        StatusCode::CREATED,
        Json(UserCreated {
            id,
            username: req.username,
            roles: req.roles,
        }),
    ))
}

/// POST /api/v1/sec/users/{username}/roles/{role}（sec:manage）
#[utoipa::path(
    post, path = "/api/v1/sec/users/{username}/roles/{role}",
    params(("username" = String, Path), ("role" = String, Path)),
    responses((status = 204), (status = 400), (status = 403))
)]
pub async fn grant_role(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Path((username, role)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    st.sec
        .grant(&username, &role)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/sec/users/{username}/roles/{role}（sec:manage）
#[utoipa::path(
    delete, path = "/api/v1/sec/users/{username}/roles/{role}",
    params(("username" = String, Path), ("role" = String, Path)),
    responses((status = 204))
)]
pub async fn revoke_role(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Path((username, role)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    st.sec
        .revoke(&username, &role)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct RoleDto {
    pub code: String,
    pub name: String,
}

/// GET /api/v1/sec/roles（sec:manage）
#[utoipa::path(get, path = "/api/v1/sec/roles", responses((status = 200, body = [RoleDto])))]
pub async fn list_roles(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
) -> Result<Json<Vec<RoleDto>>, ApiError> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT code, name FROM sec_role ORDER BY code")
            .fetch_all(&st.pool)
            .await
            .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    Ok(Json(
        rows.into_iter()
            .map(|(code, name)| RoleDto { code, name })
            .collect(),
    ))
}

/// 用户列表查询参数（增量13 搜索 + 增量14 分页）
#[derive(Deserialize)]
pub struct UsersQuery {
    /// username 模糊过滤（增量13）
    search: Option<String>,
    page: Option<u32>,
    size: Option<u32>,
}

/// GET /api/v1/sec/users（sec:manage）——用户列表（角色聚合；增量11 用户管理面）
#[utoipa::path(
    get, path = "/api/v1/sec/users",
    params(("search" = String, Query, description = "username 模糊过滤"), ("page" = u32, Query), ("size" = u32, Query, description = "缺省 50，上限 200")),
    responses((status = 200, description = "{items,total,page,size}；items 含 username/max_secret_level/roles/created_at"), (status = 403))
)]
pub async fn list_users(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Query(q): Query<UsersQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let page = q.page.unwrap_or(1).max(1);
    let size = q.size.unwrap_or(50).clamp(1, 200);
    let (items, total) = st
        .sec
        .list_users(q.search.as_deref(), page, size)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(Json(serde_json::json!({
        "items": items,
        "total": total,
        "page": page,
        "size": size,
    })))
}

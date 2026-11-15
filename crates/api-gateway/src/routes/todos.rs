//! /api/v1/todos 路由（增量19 WF-03 待办中心）：待办/已办/我发起三盒统一查询。
//! 门控 docs:read（与 GET /docs 同层）；数据面 ABAC 密级过滤在 DocService::todos 内。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::Extension;
use axum::Json;
use core_sec::AuthUser;
use mod_doc::TodoBox;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct TodoQ {
    /// 盒类型：todo（缺省）/ done / mine
    pub r#box: Option<String>,
    pub page: Option<u32>,
    pub size: Option<u32>,
}

/// 待办中心行 DTO（OpenAPI 面；todo/mine 盒 action/at/detail 为 null）
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct TodoItemDto {
    pub obj_no: String,
    pub name: Option<String>,
    pub state: Option<String>,
    pub major_ver: Option<i32>,
    pub minor_ver: Option<i32>,
    pub secret_level: Option<i16>,
    pub submitter: Option<String>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub action: Option<String>,
    pub at: Option<chrono::DateTime<chrono::Utc>>,
    pub detail: Option<Value>,
}

impl From<mod_doc::TodoItem> for TodoItemDto {
    fn from(i: mod_doc::TodoItem) -> Self {
        Self {
            obj_no: i.obj_no,
            name: i.name,
            state: i.state,
            major_ver: i.major_ver,
            minor_ver: i.minor_ver,
            secret_level: i.secret_level,
            submitter: i.submitter,
            updated_at: i.updated_at,
            action: i.action,
            at: i.at,
            detail: i.detail,
        }
    }
}

/// GET /api/v1/todos?box=todo|done|mine&page=1&size=20 — 待办中心三盒
#[utoipa::path(
    get, path = "/api/v1/todos",
    params(("box" = String, Query, description = "todo（缺省）/done/mine"), ("page" = u32, Query), ("size" = u32, Query)),
    responses((status = 200, description = "{items, total, page, size}"), (status = 400, description = "非法盒名"))
)]
pub async fn list_todos(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<TodoQ>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let box_name = q.r#box.as_deref().unwrap_or("todo");
    let b = TodoBox::parse(box_name).ok_or_else(|| {
        ApiError::new(
            core_model::DomainError::BadRequest(format!(
                "box 须为 todo|done|mine，得到 {box_name}"
            )),
            &tid.0,
        )
    })?;
    let page = q.page.unwrap_or(1).max(1);
    let size = q.size.unwrap_or(20).clamp(1, 100);
    let (items, total) = st
        .service
        .todos(
            b,
            user.id,
            &user.username,
            user.max_secret_level,
            page,
            size,
        )
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    let dtos: Vec<TodoItemDto> = items.into_iter().map(Into::into).collect();
    Ok(Json(json!({
        "items": dtos,
        "total": total,
        "page": page,
        "size": size,
    })))
}

//! /api/v1/parts：物料主数据（增量17，PART-01/02/05 纵切）。
//! 权限裁定：复用 docs 门控（SMB 简化——独立 parts 权限后置记账）；ABAC 密级同 docs 语义。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::Extension;
use axum::Json;
use core_sec::AuthUser;
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct CreatePartReq {
    /// 分类 code（编码规则挂在分类上）
    pub class_code: String,
    pub attrs: serde_json::Map<String, serde_json::Value>,
}

/// POST /api/v1/parts — 创建物料（分类赋码 + 查重提示，不阻断）
#[utoipa::path(
    post, path = "/api/v1/parts",
    request_body = CreatePartReq,
    responses((status = 201, description = "{obj_no,class_code,duplicates}"), (status = 400), (status = 403))
)]
pub async fn create_part(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<CreatePartReq>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let result = st
        .parts
        .create(&req.class_code, req.attrs, Some(user.id))
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "obj_no": result.obj_no,
            "class_code": result.class_code,
            "duplicates": result.duplicates,
        })),
    ))
}

#[derive(Deserialize)]
pub struct ListPartsQuery {
    /// 名称过滤（等值——模糊查重走 duplicates 端点）
    name: Option<String>,
    page: Option<u32>,
    size: Option<u32>,
}

/// GET /api/v1/parts — 物料列表（ABAC 密级）
#[utoipa::path(
    get, path = "/api/v1/parts",
    params(("name" = String, Query), ("page" = u32, Query), ("size" = u32, Query)),
    responses((status = 200, description = "{items,total,page,size}"))
)]
pub async fn list_parts(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<ListPartsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (items, total) = st
        .parts
        .list(
            q.name.as_deref(),
            q.page.unwrap_or(1).max(1),
            q.size.unwrap_or(20).clamp(1, 200),
            user.max_secret_level,
        )
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(Json(json!({
        "items": items,
        "total": total,
        "page": q.page.unwrap_or(1),
        "size": q.size.unwrap_or(20),
    })))
}

#[derive(Deserialize)]
pub struct DuplicatesQuery {
    name: String,
    material: Option<String>,
}

/// GET /api/v1/parts/duplicates — 查重（PART-05 MVP：名称/材料模糊，LIMIT 5）
#[utoipa::path(
    get, path = "/api/v1/parts/duplicates",
    params(("name" = String, Query), ("material" = String, Query)),
    responses((status = 200, description = "[{obj_no,name,material}]"))
)]
pub async fn duplicates(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<DuplicatesQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // 查重调用者按自身密级——结果内含他人物料时 ABAC 行级过滤（镜像 where-used 模式）
    let all = st
        .parts
        .duplicates(&q.name, q.material.as_deref())
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    // 逐行按 obj_no 查密级过滤
    let mut out = Vec::new();
    for d in all {
        let visible = sqlx::query_scalar::<_, i16>(
            "SELECT secret_level FROM cm_object WHERE obj_no = $1 AND deleted_at IS NULL",
        )
        .bind(&d.obj_no)
        .fetch_optional(&st.pool)
        .await
        .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?
        .map(|sl| sl <= user.max_secret_level)
            == Some(true);
        if visible {
            out.push(json!(d));
        }
    }
    Ok(Json(json!(out)))
}

/// GET /api/v1/classes — 分类树（docs:read）
#[utoipa::path(get, path = "/api/v1/classes", responses((status = 200)))]
pub async fn class_tree(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tree = st
        .parts
        .class_tree()
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(Json(json!(tree)))
}

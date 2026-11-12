//! /api/v1/boms：对象的 BOM 结构管理（读=docs:read，写=docs:write；NDJSON 流式展开）。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use axum::Json;
use core_bom::{ExpandMode, LineInput};
use core_sec::AuthUser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

/// POST /api/v1/boms/{obj_no}/lines — 全量替换 BOM 行（docs:write；循环 → 409）
#[utoipa::path(
    post, path = "/api/v1/boms/{obj_no}/lines",
    params(("obj_no" = String, Path)),
    request_body = Vec<LineInputDto>,
    responses((status = 204), (status = 400, description = "行号重复/数量非法"), (status = 404), (status = 409, description = "循环引用"))
)]
pub async fn set_lines(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(obj_no): Path<String>,
    Json(lines): Json<Vec<LineInputDto>>,
) -> Result<StatusCode, ApiError> {
    let obj = st
        .service
        .detail(&obj_no)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    // 终审 C2：密级 ABAC（越级 404 不泄露存在性；写侧同样拒绝）
    if obj.object.secret_level > user.max_secret_level {
        return Err(ApiError::new(
            core_model::DomainError::ObjNotFound(format!("docs:{obj_no}")),
            &tid.0,
        ));
    }
    let inputs: Vec<LineInput> = lines
        .into_iter()
        .map(|l| LineInput {
            line_no: l.line_no,
            child: l.child,
            qty: l.qty,
        })
        .collect();
    // 子节点必须为库内对象——expand/where-used 无需校验（FK 保证）
    st.bom
        .set_lines(obj.object.id, inputs)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ExpandQuery {
    mode: Option<String>,
    /// 发布 major（增量9 BOM 版本链）：给定则按发布快照 as-of 展开；缺省=live
    major: Option<i32>,
}

/// GET /api/v1/boms/{obj_no}/expand?mode=full|leaf|onelevel&major=N — NDJSON 流式
#[utoipa::path(
    get, path = "/api/v1/boms/{obj_no}/expand",
    params(("obj_no" = String, Path), ("mode" = String, Query, description = "full|leaf|onelevel，缺省 full"), ("major" = i32, Query, description = "发布 major：as-of 快照展开；缺省=live")),
    responses((status = 200, description = "NDJSON: {child, qty_total} 行流"))
)]
pub async fn expand(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(obj_no): Path<String>,
    Query(q): Query<ExpandQuery>,
) -> Result<Response, ApiError> {
    let obj = st
        .service
        .detail(&obj_no)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    // 终审 C2：密级 ABAC
    if obj.object.secret_level > user.max_secret_level {
        return Err(ApiError::new(
            core_model::DomainError::ObjNotFound(format!("docs:{obj_no}")),
            &tid.0,
        ));
    }
    // 终审 M2：未知 mode → 400（不再静默回退 Full）
    let mode = match q.mode.as_deref() {
        None | Some("full") => ExpandMode::Full,
        Some("leaf") => ExpandMode::LeafOnly,
        Some("onelevel") => ExpandMode::OneLevel,
        Some(other) => {
            return Err(ApiError::new(
                core_model::DomainError::BadRequest(format!(
                    "mode 须为 full|leaf|onelevel，得到 {other}"
                )),
                &tid.0,
            ));
        }
    };
    // 增量9：major=N → 发布快照 as-of 展开；缺省 → live 闭包
    let rows = match q.major {
        Some(major) => st
            .bom
            .expand_asof(obj.object.id, major, mode)
            .await
            .map_err(|e| ApiError::new(e, &tid.0))?,
        None => st
            .bom
            .expand(obj.object.id, mode)
            .await
            .map_err(|e| ApiError::new(e, &tid.0))?,
    };

    let body: Vec<String> = rows
        .iter()
        .map(|r| json!({"child": r.child, "qty_total": r.qty_total}).to_string())
        .collect();
    let ndjson = body.join("\n");
    let mut res = (StatusCode::OK, ndjson).into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/x-ndjson".parse().unwrap(),
    );
    Ok(res)
}

/// GET /api/v1/boms/{obj_no}/where-used — 反查（NDJSON 流式）
#[utoipa::path(
    get, path = "/api/v1/boms/{obj_no}/where-used",
    params(("obj_no" = String, Path)),
    responses((status = 200, description = "NDJSON: {ancestor, qty_total} 行流"))
)]
pub async fn where_used(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(obj_no): Path<String>,
) -> Result<Response, ApiError> {
    let obj = st
        .service
        .detail(&obj_no)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    // 终审 C2：密级 ABAC（反查同样不泄露越级对象存在性）
    if obj.object.secret_level > user.max_secret_level {
        return Err(ApiError::new(
            core_model::DomainError::ObjNotFound(format!("docs:{obj_no}")),
            &tid.0,
        ));
    }
    let rows = st
        .bom
        .where_used(obj.object.id)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    let body: Vec<String> = rows
        .iter()
        .map(|(a, q)| json!({"ancestor": a, "qty_total": q}).to_string())
        .collect();
    let ndjson = body.join("\n");
    let mut res = (StatusCode::OK, ndjson).into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/x-ndjson".parse().unwrap(),
    );
    Ok(res)
}

#[derive(Deserialize, Serialize, ToSchema)]
pub struct LineInputDto {
    pub line_no: i32,
    /// 子对象 id（cm_object.id）
    pub child: uuid::Uuid,
    pub qty: i64,
}

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

#[derive(Deserialize)]
pub struct WhereUsedQuery {
    /// as-of 历史反查（增量14）：按发布快照分组（release_root/major），回填 obj_no
    asof: Option<String>,
}

/// GET /api/v1/boms/{obj_no}/where-used — 反查（NDJSON 流式）
/// 缺省=live（{ancestor, qty_total}）；`?asof=1`=发布快照历史（{release_root_no, major, parent_no, qty_total}）
#[utoipa::path(
    get, path = "/api/v1/boms/{obj_no}/where-used",
    params(("obj_no" = String, Path), ("asof" = String, Query, description = "asof=1 → 发布快照历史反查")),
    responses((status = 200, description = "NDJSON 行流"))
)]
pub async fn where_used(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(obj_no): Path<String>,
    Query(q): Query<WhereUsedQuery>,
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
    let ndjson = if q.asof.as_deref() == Some("1") {
        // as-of（增量14）：store 纯聚合 → 网关 ABAC 过滤（终审 C1：root/parent 是别的对象，
        // 必须按查看者密级过滤——低密级用户不得经反查看到绝密父件的存在性/件号）→ 回填 obj_no
        let agg = st
            .bom
            .where_used_asof(obj.object.id)
            .await
            .map_err(|e| ApiError::new(e, &tid.0))?;
        let ids: Vec<uuid::Uuid> = agg
            .iter()
            .flat_map(|(root, _, parent, _)| [*root, *parent])
            .collect();
        // id → (obj_no, secret_level)
        let meta: std::collections::HashMap<uuid::Uuid, (String, i16)> = if ids.is_empty() {
            Default::default()
        } else {
            let rows: Vec<(uuid::Uuid, String, i16)> =
                sqlx::query_as("SELECT id, obj_no, secret_level FROM cm_object WHERE id = ANY($1)")
                    .bind(&ids)
                    .fetch_all(&st.pool)
                    .await
                    .map_err(|e| {
                        ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0)
                    })?;
            rows.into_iter()
                .map(|(id, no, sl)| (id, (no, sl)))
                .collect()
        };
        agg.iter()
            // 行级丢弃越密级 root/parent（存在性隐藏——child 越级已在分支前 404）
            .filter(|(root, _, parent, _)| {
                let root_ok = meta.get(root).map(|(_, sl)| *sl <= user.max_secret_level) == Some(true);
                let parent_ok =
                    meta.get(parent).map(|(_, sl)| *sl <= user.max_secret_level) == Some(true);
                root_ok && parent_ok
            })
            .filter_map(|(root, major, parent, qty)| {
                let root_no = meta.get(root).map(|(no, _)| no.clone())?;
                let parent_no = meta.get(parent).map(|(no, _)| no.clone())?;
                Some(
                    json!({"release_root_no": root_no, "major": major, "parent_no": parent_no, "qty_total": qty})
                        .to_string(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        // live（现状，字段名兼容不破坏既有消费）
        let rows = st
            .bom
            .where_used(obj.object.id)
            .await
            .map_err(|e| ApiError::new(e, &tid.0))?;
        rows.iter()
            .map(|(a, q)| json!({"ancestor": a, "qty_total": q}).to_string())
            .collect::<Vec<_>>()
            .join("\n")
    };
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

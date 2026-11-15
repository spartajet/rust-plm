//! /api/v1/meta：类型元数据读取与流程档位管理。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct TypeMeta {
    pub code: String,
    pub name: String,
    pub flow_tier: String,
}

/// GET /api/v1/meta/types/{code}（认证即可——档位是公共语义）
#[utoipa::path(
    get, path = "/api/v1/meta/types/{code}",
    params(("code" = String, Path)),
    responses((status = 200, body = TypeMeta), (status = 404))
)]
pub async fn get_type(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Path(code): Path<String>,
) -> Result<Json<TypeMeta>, ApiError> {
    let td = st
        .registry
        .type_def(&code)
        .map_err(|e| ApiError::new(e, &tid.0))?;
    let flow_tier = td
        .config
        .get("flow_tier")
        .and_then(|v| v.as_str())
        .unwrap_or("standard")
        .to_string();
    Ok(Json(TypeMeta {
        code: td.code.clone(),
        name: td.name.clone(),
        flow_tier,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct SetFlowTierReq {
    /// minimal | standard | strict（增量19 起可选；与 require_comment 至少一项）
    pub tier: Option<String>,
    /// 审批意见必填开关（增量19 WF-02；缺省不改）
    pub require_comment: Option<bool>,
}

/// PUT /api/v1/meta/types/{code}/flow-tier（docs:approve 门控；热生效 ≤3s）
#[utoipa::path(
    put, path = "/api/v1/meta/types/{code}/flow-tier",
    params(("code" = String, Path)),
    request_body = SetFlowTierReq,
    responses((status = 204), (status = 400), (status = 403))
)]
pub async fn set_flow_tier(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<core_sec::AuthUser>,
    Path(code): Path<String>,
    Json(req): Json<SetFlowTierReq>,
) -> Result<StatusCode, ApiError> {
    // 先 404（类型存在性）后 400（档位合法性）——终审 M5
    st.registry
        .type_def(&code)
        .map_err(|e| ApiError::new(e, &tid.0))?;
    if req.tier.is_none() && req.require_comment.is_none() {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest(
                "tier 与 require_comment 至少提供一项".into(),
            ),
            &tid.0,
        ));
    }
    if let Some(tier) = &req.tier {
        if core_version::FlowTier::parse(tier).is_none() {
            return Err(ApiError::new(
                core_model::DomainError::BadRequest(format!(
                    "档位须为 minimal|standard|strict，得到 {tier}"
                )),
                &tid.0,
            ));
        }
    }
    // 旧档位（审计用——UPDATE 前读取）
    let old_tier = st
        .registry
        .type_def(&code)
        .ok()
        .and_then(|td| {
            td.config
                .get("flow_tier")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| "standard".into());
    let old_rc = st
        .registry
        .type_def(&code)
        .ok()
        .and_then(|td| {
            td.config
                .get("require_comment")
                .and_then(|v| v.as_bool())
        })
        .unwrap_or(false);
    // UPDATE 触发 cm_meta_changed → TypeRegistry 热刷新（≤3s）；合并未提供的键
    let mut patch = serde_json::Map::new();
    if let Some(tier) = &req.tier {
        patch.insert("flow_tier".into(), serde_json::json!(tier));
    }
    if let Some(rc) = req.require_comment {
        patch.insert("require_comment".into(), serde_json::json!(rc));
    }
    sqlx::query(
        "UPDATE cm_type SET config = config || $2::jsonb, updated_at = now() WHERE code = $1",
    )
    .bind(&code)
    .bind(serde_json::Value::Object(patch))
    .execute(&st.pool)
    .await
    .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    // 审计埋点（增量18 SYS-01）：配置变更留痕（含新旧档位/意见开关+操作人）
    let old_rc_json = if old_rc { serde_json::json!(true) } else { serde_json::json!(false) };
    let new_rc_json = req
        .require_comment
        .map(|rc| serde_json::json!(rc))
        .unwrap_or(old_rc_json.clone());
    st.audit.record(
        user.username,
        "flow-tier.change",
        Some(code),
        Some(serde_json::json!({
            "from": old_tier.clone(), "to": req.tier.clone().unwrap_or(old_tier),
            "require_comment_from": old_rc_json, "require_comment_to": new_rc_json,
        })),
        Some(tid.0.clone()),
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct CodPreviewQuery {
    /// 预览数量（缺省 3，上限 20）
    n: Option<usize>,
}

/// GET /api/v1/meta/types/{code}/cod-preview?n=3 — 编码预览（不烧号；增量16 PRD COD-01）
#[utoipa::path(
    get, path = "/api/v1/meta/types/{code}/cod-preview",
    params(("code" = String, Path), ("n" = usize, Query, description = "预览数量，缺省 3")),
    responses((status = 200, description = "{codes: [...]}"), (status = 404))
)]
pub async fn cod_preview(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Path(code): Path<String>,
    Query(q): Query<CodPreviewQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let td = st
        .registry
        .type_def(&code)
        .map_err(|e| ApiError::new(e, &tid.0))?;
    let rule = core_model::cod::CodRule::from_config(&td.config).ok_or_else(|| {
        ApiError::new(
            core_model::DomainError::BadRequest(format!("类型 {code} 未配置编码规则")),
            &tid.0,
        )
    })?;
    let n = q.n.unwrap_or(3).clamp(1, 20);
    let codes = core_model::cod::preview(&st.pool, &code, &rule, n)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(Json(serde_json::json!({ "codes": codes })))
}

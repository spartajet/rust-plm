//! /api/v1/im — IM 卡片动作回调（增量10）：免 Bearer，HMAC 验签 + IM 用户映射 + casbin + 密级 ABAC。
//! 真实 IM 卡片按钮经企业应用回调网关转发到此端点（契约见 README IM 段）。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Extension;
use axum::Json;
use core_version::LifecycleEvent;
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct ImActionReq {
    pub obj_no: String,
    /// approve | reject
    pub action: String,
    /// IM 侧用户标识（经 im_user_map 映射 PLM username）
    pub user: String,
    #[serde(default)]
    pub comment: Option<String>,
}

/// POST /api/v1/im/actions — IM 卡片动作回调（HMAC：`X-IM-Sign` = hex(HMAC-SHA256(secret, body))）
#[utoipa::path(
    post, path = "/api/v1/im/actions",
    request_body = ImActionReq,
    responses((status = 200, description = "迁移后对象"), (status = 401, description = "签名不匹配"), (status = 403), (status = 404), (status = 409))
)]
pub async fn im_action(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    // 未配置密钥 → 端点关闭（503，fail-closed）
    if st.im_secret.is_empty() {
        return Err(ApiError::new(
            core_model::DomainError::ServiceUnavailable("IM 回调未配置(PLM_IM_SECRET)".into()),
            &tid.0,
        ));
    }
    let Some(sign) = headers
        .get("x-im-sign")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
    else {
        return Err(ApiError::new(
            core_model::DomainError::SecUnauthorized,
            &tid.0,
        ));
    };
    if !core_im::verify_hmac_hex(&st.im_secret, &body, &sign) {
        return Err(ApiError::new(
            core_model::DomainError::SecUnauthorized,
            &tid.0,
        ));
    }

    let req: ImActionReq = serde_json::from_slice(&body).map_err(|e| {
        ApiError::new(
            core_model::DomainError::BadRequest(format!("JSON 解析失败: {e}")),
            &tid.0,
        )
    })?;

    // IM 用户 → PLM 用户（未映射 403，不泄露存在性）
    let Some(username) = st.im_user_map.get(&req.user) else {
        return Err(ApiError::new(core_model::DomainError::SecForbidden, &tid.0));
    };
    let username = username.clone();

    // 权限（casbin）+ 密级 ABAC（越级 404 与 docs 路由同语义）+ 迁移
    let allowed = st
        .sec
        .enforce(&username, "docs", "approve")
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    if !allowed {
        return Err(ApiError::new(core_model::DomainError::SecForbidden, &tid.0));
    }
    let d = st
        .service
        .detail(&req.obj_no)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    let uid = st
        .sec
        .user_id(&username)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    let max_level = st
        .sec
        .max_secret_level_of(uid)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    if d.object.secret_level > max_level {
        return Err(ApiError::new(
            core_model::DomainError::ObjNotFound(format!("docs:{}", req.obj_no)),
            &tid.0,
        ));
    }
    let ev = match req.action.as_str() {
        "approve" => LifecycleEvent::Approve,
        "reject" => LifecycleEvent::Reject,
        other => {
            return Err(ApiError::new(
                core_model::DomainError::BadRequest(format!(
                    "action 须为 approve|reject，得到 {other}"
                )),
                &tid.0,
            ))
        }
    };
    let o = st
        .service
        .transition(&st.lifecycle, ev, &req.obj_no, uid)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(Json(json!({
        "obj_no": o.obj_no,
        "state": o.state,
        "major": o.major,
        "minor": o.minor,
        "by": username,
    })))
}

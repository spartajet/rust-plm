//! 鉴权中间件：Bearer → parse_access → AuthUser 注入 Extension（计划 Review Focus #1/#5）。

use crate::error::{ApiError, TraceId};
use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;
use core_sec::AuthUser;

pub async fn require_auth(
    State(st): State<crate::state::AppState>,
    tid: Option<axum::Extension<TraceId>>,
    mut req: axum::extract::Request,
    next: Next,
) -> Result<Response, ApiError> {
    let trace: String = tid
        .map(|t| t.0 .0)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let deny = |msg: &'static str| {
        ApiError::new(core_model::DomainError::SecUnauthorized, &trace).with_message(msg)
    };

    let raw = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| deny("缺少 Authorization 头"))?;
    let token =
        core_sec::strip_bearer(raw).map_err(|_| deny("Authorization 必须为 Bearer <token>"))?;
    let claims = st
        .sec
        .parse_access(token)
        .map_err(|e| ApiError::new(e, &trace))?;
    req.extensions_mut().insert(AuthUser {
        id: claims.sub,
        username: claims.username,
        max_secret_level: 0, // 由 require_perm 回填（服务层密级过滤用）
    });
    Ok(next.run(req).await)
}

/// 授权检查（纯实现，便于测试）：enforce(obj,act) → 403；通过则回填密级上限
pub async fn perm_check(
    st: &crate::state::AppState,
    trace: &str,
    mut req: axum::extract::Request,
    next: Next,
    obj: &'static str,
    act: &'static str,
) -> Result<Response, ApiError> {
    let mut user = req
        .extensions()
        .get::<AuthUser>()
        .cloned()
        .ok_or_else(|| ApiError::new(core_model::DomainError::SecUnauthorized, trace))?;
    let allowed = st
        .sec
        .enforce(&user.username, obj, act)
        .await
        .map_err(|e| ApiError::new(e, trace))?;
    if !allowed {
        return Err(ApiError::new(core_model::DomainError::SecForbidden, trace));
    }
    user.max_secret_level = st
        .sec
        .max_secret_level_of(user.id)
        .await
        .map_err(|e| ApiError::new(e, trace))?;
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

/// 工厂：捕获 ('static obj, act) 的 Clone 闭包，供 from_fn_with_state 使用
pub fn require_perm(
    obj: &'static str,
    act: &'static str,
) -> impl Clone
       + Send
       + Fn(
    State<crate::state::AppState>,
    Option<axum::Extension<TraceId>>,
    axum::extract::Request,
    Next,
) -> futures_util::future::BoxFuture<'static, Result<Response, ApiError>> {
    move |st, tid, req, next| {
        Box::pin(async move {
            let trace: String = tid
                .map(|t| t.0 .0)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            perm_check(&st, &trace, req, next, obj, act).await
        })
    }
}

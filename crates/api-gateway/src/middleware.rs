//! 授权中间件：require_perm = 认证(Bearer→401) + 授权(enforce→403) + 密级回填。
//! 豁免路由不挂本中间件（/healthz、/api/docs*、/api/v1/auth/*）。

use crate::error::{ApiError, TraceId};
use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;
use core_sec::AuthUser;

/// 授权检查（自包含）：认证 → enforce(obj,act) → 回填密级上限
pub async fn perm_check(
    st: &crate::state::AppState,
    trace: &str,
    mut req: axum::extract::Request,
    next: Next,
    obj: &'static str,
    act: &'static str,
) -> Result<Response, ApiError> {
    let deny401 = |msg: &'static str| {
        ApiError::new(core_model::DomainError::SecUnauthorized, trace).with_message(msg)
    };

    let raw = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| deny401("缺少 Authorization 头"))?;
    let token =
        core_sec::strip_bearer(raw).map_err(|_| deny401("Authorization 必须为 Bearer <token>"))?;
    let claims = st
        .sec
        .parse_access(token)
        .map_err(|e| ApiError::new(e, trace))?;
    let mut user = AuthUser {
        id: claims.sub,
        username: claims.username,
        max_secret_level: 0,
    };

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
#[allow(clippy::type_complexity)]
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

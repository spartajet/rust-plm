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
    let trace: String = tid.map(|t| t.0 .0).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let deny = |msg: &'static str| ApiError::new(core_model::DomainError::SecUnauthorized, &trace).with_message(msg);

    let raw = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| deny("缺少 Authorization 头"))?;
    let token = core_sec::strip_bearer(raw).map_err(|_| deny("Authorization 必须为 Bearer <token>"))?;
    let claims = st.sec.parse_access(token).map_err(|e| ApiError::new(e, &trace))?;
    req.extensions_mut().insert(AuthUser { id: claims.sub, username: claims.username });
    Ok(next.run(req).await)
}

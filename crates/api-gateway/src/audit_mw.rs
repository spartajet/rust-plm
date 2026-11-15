//! 审计中间件（增量18 SYS-01）：响应后兜底埋点（method+path+status+user+trace_id+ip）。
//! 显式事件（登录成败/下载/flow-tier/sec 变更等）由各 handler 直接调 `st.audit.record`（带语义 action）；
//! 中间件兜底其余 authed 请求 action=http。写入走 AuditService::record（spawn 异步不阻塞）。

use crate::state::AppState;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

pub async fn audit_mw(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let method = req.method().clone().to_string();
    let path = req.uri().path().to_string();
    let ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string());
    let tid = req
        .extensions()
        .get::<crate::error::TraceId>()
        .map(|t| t.0.clone());
    // 直接解析 JWT 得用户名（不依赖 authn 中间件顺序；解析失败=anonymous，登录失败场景本就无有效令牌）
    let user = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| core_sec::strip_bearer(raw).ok())
        .and_then(|t| st.sec.parse_access(t).ok())
        .map(|c| c.username);

    let res = next.run(req).await;

    // 2xx 全记（兜底 http）；显式事件由 handler 直写（含 4xx 安全事件）
    if res.status().is_success() {
        st.audit.record(
            user.unwrap_or_else(|| "anonymous".into()),
            "http",
            None,
            Some(
                serde_json::json!({"method": method, "path": path, "status": res.status().as_u16()}),
            ),
            tid,
            ip,
        );
    }
    res
}

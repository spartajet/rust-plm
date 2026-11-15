//! 审计中间件（增量18 SYS-01）：响应后兜底埋点（method+path+status+user+trace_id+ip）。
//! 显式事件（登录成败/下载/flow-tier/sec 变更等）由各 handler 直接调 `st.audit.record`（带语义 action）；
//! 中间件兜底其余 authed 请求 action=http。写入走 AuditService::record（spawn 异步不阻塞）。

use crate::state::AppState;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

/// 客户端 IP（audit_mw 解析后插入 extensions——显式埋点 handler 复用；终审 M2）
#[derive(Clone)]
pub struct ClientIp(pub Option<String>);

pub async fn audit_mw(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let method = req.method().clone().to_string();
    let path = req.uri().path().to_string();
    let ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string());
    req.extensions_mut().insert(ClientIp(ip.clone()));
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

    // 豁免清单（终审 M3/M4）：健康探针/OpenAPI 噪声不入库；auth 显式埋点免双行；
    // /audit 查询自身不入库（否则自增行破坏分页快照）。显式事件仍由 handler 直写。
    const SKIP: [&str; 4] = ["/healthz", "/api/docs", "/api/v1/audit", "/api/v1/auth"];
    if SKIP
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}/")))
    {
        return next.run(req).await;
    }

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

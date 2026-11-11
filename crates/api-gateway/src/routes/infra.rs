//! 基础设施路由：healthz / OpenAPI / Scalar UI。

use crate::state::AppState;
use axum::extract::State;
use axum::response::Html;
use axum::Json;
use serde::Serialize;
use utoipa::{OpenApi, ToSchema};

#[derive(Serialize, ToSchema)]
pub struct Healthz {
    pub status: String,
    pub meta_version: u64,
}

/// GET /healthz — D-AC 健康检查（DB ping + 元数据版本；ping 失败 → 503 degraded，终审 I1）
#[utoipa::path(
    get,
    path = "/healthz",
    responses((status = 200, body = Healthz), (status = 503, body = Healthz))
)]
pub async fn healthz(State(st): State<AppState>) -> axum::response::Response {
    use axum::response::IntoResponse;
    match sqlx::query("SELECT 1").execute(&st.pool).await {
        Ok(_) => (
            axum::http::StatusCode::OK,
            Json(Healthz {
                status: "ok".into(),
                meta_version: st.registry.version(),
            }),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(Healthz {
                status: "degraded".into(),
                meta_version: st.registry.version(),
            }),
        )
            .into_response(),
    }
}

/// GET /api/docs/openapi.json
pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(crate::state::ApiDocHolder::openapi())
}

/// GET /api/docs — Scalar UI（CDN；纯内网环境见 README 离线说明）
pub async fn scalar_ui() -> Html<&'static str> {
    Html(include_str!("../scalar.html"))
}

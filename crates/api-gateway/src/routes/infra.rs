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

/// GET /healthz — D-AC 健康检查（DB ping + 元数据版本）
#[utoipa::path(get, path = "/healthz", responses((status = 200, body = Healthz)))]
pub async fn healthz(State(st): State<AppState>) -> Json<Healthz> {
    // DB 可达性即健康定义的一部分；失败时 axum 返回 500（连接错误冒泡）
    let _ = sqlx::query("SELECT 1").execute(&st.pool).await;
    Json(Healthz {
        status: "ok".into(),
        meta_version: st.registry.version(),
    })
}

/// GET /api/docs/openapi.json
pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(crate::state::ApiDocHolder::openapi())
}

/// GET /api/docs — Scalar UI（CDN；纯内网环境见 README 离线说明）
pub async fn scalar_ui() -> Html<&'static str> {
    Html(include_str!("../scalar.html"))
}

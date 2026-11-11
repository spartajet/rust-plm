//! 路由装配：唯一 build 函数（R-D4）。

pub mod docs;
pub mod infra;

use crate::state::AppState;
use axum::routing::{get, post};
use axum::Router;

pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(infra::healthz))
        .route("/api/v1/docs", post(docs::upload_doc).get(docs::list_docs))
        .route("/api/v1/docs/{obj_no}", get(docs::get_doc))
        .route("/api/v1/docs/{obj_no}/file", get(docs::download_doc))
        .route("/api/docs/openapi.json", get(infra::openapi_json))
        .route("/api/docs", get(infra::scalar_ui))
        .layer(axum::middleware::from_fn(crate::error::trace_mw))
        .layer(axum::extract::DefaultBodyLimit::max(state.max_upload as usize))
        .with_state(state)
}

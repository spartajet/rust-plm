//! 路由装配：唯一 build 函数（R-D4）。

pub mod auth;
pub mod docs;
pub mod infra;

use crate::middleware::require_auth;
use crate::state::AppState;
use axum::routing::{get, post};
use axum::Router;

pub fn build(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/v1/docs", post(docs::upload_doc).get(docs::list_docs))
        .route("/api/v1/docs/{obj_no}", get(docs::get_doc))
        .route("/api/v1/docs/{obj_no}/file", get(docs::download_doc))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_auth,
        ));

    let public = Router::new()
        .route("/healthz", get(infra::healthz))
        .route("/api/v1/auth/login", post(auth::login))
        .route("/api/v1/auth/refresh", post(auth::refresh))
        .route("/api/docs/openapi.json", get(infra::openapi_json))
        .route("/api/docs", get(infra::scalar_ui));

    public
        .merge(protected)
        .layer(axum::middleware::from_fn(crate::error::trace_mw))
        .layer(axum::extract::DefaultBodyLimit::max(
            state.max_upload as usize,
        ))
        .with_state(state)
}

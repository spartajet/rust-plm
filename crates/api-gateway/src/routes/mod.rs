//! 路由装配：唯一 build 函数（R-D4）。

pub mod auth;
pub mod docs;
pub mod infra;
pub mod sec_users;
pub mod webhooks;

use crate::middleware::require_perm;
use crate::state::AppState;
use axum::routing::{delete, get, post};
use axum::Router;

pub fn build(state: AppState) -> Router {
    // 鉴权（谁）→ 授权（能做什么）两层
    let docs_rw = Router::new()
        .route("/api/v1/docs", post(docs::upload_doc))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_perm("docs", "write"),
        ));
    let docs_r = Router::new()
        .route("/api/v1/docs", get(docs::list_docs))
        .route("/api/v1/docs/{obj_no}", get(docs::get_doc))
        .route("/api/v1/docs/{obj_no}/file", get(docs::download_doc))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_perm("docs", "read"),
        ));
    let docs_lc_write = Router::new()
        .route("/api/v1/docs/{obj_no}/submit", post(docs::submit_doc))
        .route("/api/v1/docs/{obj_no}/reject", post(docs::reject_doc))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_perm("docs", "write"),
        ));
    let docs_lc_approve = Router::new()
        .route("/api/v1/docs/{obj_no}/approve", post(docs::approve_doc))
        .route("/api/v1/docs/{obj_no}/obsolete", post(docs::obsolete_doc))
        .route("/api/v1/docs/{obj_no}/release", post(docs::release_doc))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_perm("docs", "approve"),
        ));
    let sec_mgmt = Router::new()
        .route("/api/v1/sec/users", post(sec_users::create_user))
        .route(
            "/api/v1/sec/users/{username}/roles/{role}",
            post(sec_users::grant_role).delete(sec_users::revoke_role),
        )
        .route("/api/v1/sec/roles", get(sec_users::list_roles))
        .route(
            "/api/v1/sec/webhooks",
            post(webhooks::create_webhook).get(webhooks::list_webhooks),
        )
        .route(
            "/api/v1/sec/webhooks/{id}",
            delete(webhooks::delete_webhook),
        )
        .route("/api/v1/sec/webhooks/dead", get(webhooks::dead_events))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_perm("sec", "manage"),
        ))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024)); // M2：管理面不承受上传限额

    // M2（终审遗留）：auth 路由 16KB 体上限，不再承受 200MB 上传限额
    let auth = Router::new()
        .route("/api/v1/auth/login", post(auth::login))
        .route("/api/v1/auth/refresh", post(auth::refresh))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024));

    let public = Router::new()
        .route("/healthz", get(infra::healthz))
        .route("/api/docs/openapi.json", get(infra::openapi_json))
        .route("/api/docs", get(infra::scalar_ui));

    public
        .merge(auth)
        .merge(docs_rw)
        .merge(docs_r)
        .merge(docs_lc_write)
        .merge(docs_lc_approve)
        .merge(sec_mgmt)
        .layer(axum::middleware::from_fn(crate::error::trace_mw))
        .layer(axum::extract::DefaultBodyLimit::max(
            state.max_upload as usize,
        ))
        .with_state(state)
}

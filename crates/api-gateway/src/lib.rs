//! api-gateway —— 唯一持有 axum Router 的 crate（概设 R-D4）。
//! 错误体 {code,message,trace_id}（ADR-0015）；mod-doc 保持传输无关。

pub mod error;
pub mod middleware;
pub mod routes;
pub mod state;

pub use state::bom_release_hook;
pub use state::AppState;

use axum::Router;

pub fn router(state: AppState) -> Router {
    routes::build(state)
}

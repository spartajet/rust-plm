//! AppState：跨 handler 共享的装配句柄（Clone，Arc 内核）。

use mod_doc::DocService;
use sqlx::PgPool;
use std::sync::Arc;
use utoipa::OpenApi;

use core_model::TypeRegistry;

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<TypeRegistry>,
    pub service: Arc<DocService>,
    pub pool: PgPool,
    /// 上传体积上限（字节），DefaultBodyLimit 使用
    pub max_upload: u64,
}

/// OpenAPI 聚合文档（/api/docs/openapi.json）
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::routes::docs::upload_doc,
        crate::routes::docs::list_docs,
        crate::routes::docs::get_doc,
        crate::routes::docs::download_doc,
        crate::routes::infra::healthz,
    ),
    components(schemas(
        crate::routes::docs::FileInfoDto,
        crate::routes::docs::DocResponse,
        crate::routes::docs::ListResponse,
        crate::routes::infra::Healthz,
    ))
)]
pub struct ApiDocHolder;

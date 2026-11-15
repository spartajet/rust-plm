//! AppState：跨 handler 共享的装配句柄（Clone，Arc 内核）。

use core_sec::SecService;
use mod_doc::DocService;
use sqlx::PgPool;
use std::sync::Arc;
use utoipa::OpenApi;

// mod_part 经 Cargo 依赖引入（AppState 字段）

use core_model::TypeRegistry;

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<TypeRegistry>,
    pub service: Arc<DocService>,
    pub pool: PgPool,
    /// 上传体积上限（字节），DefaultBodyLimit 使用
    pub max_upload: u64,
    /// 认证服务（docs 路由要求 Bearer）
    pub sec: Arc<SecService>,
    /// 生命周期存储（增量3）
    pub lifecycle: Arc<core_version::LifecycleStore>,
    /// BOM 引擎（增量6）
    pub bom: Arc<core_bom::BomStore>,
    /// 物料服务（增量17 PART）
    pub parts: Arc<mod_part::PartService>,
    /// 审计服务（增量18 SYS-01）
    pub audit: Arc<core_events::AuditService>,
    /// 全文检索（增量20 RPT-01/03，core-search 内嵌 tantivy）
    pub search: Arc<core_search::SearchService>,
    /// IM 回调 HMAC 密钥（增量10；空串=端点 503 关闭——未配置即不可用）
    pub im_secret: String,
    /// IM id → PLM username 映射（增量10）
    pub im_user_map: std::collections::HashMap<String, String>,
}

/// 发布冻结注入（增量9 BOM 版本链）：released 同事务快照 BOM 子树。
/// 装配共用——plm-server main 与 e2e harness 同源，避免两处漂移。
pub fn bom_release_hook(bom: Arc<core_bom::BomStore>) -> Arc<dyn core_version::ReleaseHook> {
    struct Hook(Arc<core_bom::BomStore>);
    #[async_trait::async_trait]
    impl core_version::ReleaseHook for Hook {
        async fn on_released(
            &self,
            tx: &mut sqlx::PgConnection,
            _type_code: &str,
            _obj_no: &str,
            obj_id: uuid::Uuid,
            major: i32,
        ) -> core_model::Result<()> {
            self.0
                .snapshot_live_edges_tx(tx, obj_id, major)
                .await
                .map(|_| ())
        }
    }
    Arc::new(Hook(bom))
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
        crate::routes::auth::login,
        crate::routes::auth::refresh,
        crate::routes::meta::get_type,
        crate::routes::meta::set_flow_tier,
        crate::routes::meta::cod_preview,
        crate::routes::parts::create_part,
        crate::routes::parts::list_parts,
        crate::routes::parts::duplicates,
        crate::routes::parts::class_tree,
        crate::routes::audit::search_audit,
        crate::routes::todos::list_todos,
        crate::routes::docs::batch_approve,
        crate::routes::search::search,
        crate::routes::boms::lines_boms,
        crate::routes::boms::summary_boms,
        crate::routes::docs::preview_doc,
        crate::routes::sec_params::list_params,
        crate::routes::sec_params::set_param,
        crate::routes::template::export_template,
        crate::routes::template::starter_template,
        crate::routes::template::import_template,
        crate::routes::checklist::get_checklist,
        crate::routes::checklist::complete_step,
    ),
    components(schemas(
        crate::routes::docs::FileInfoDto,
        crate::routes::docs::DocResponse,
        crate::routes::docs::ListResponse,
        crate::routes::docs::LcBody,
        crate::routes::docs::BatchApproveReq,
        crate::routes::docs::BatchApproveResp,
        crate::routes::todos::TodoItemDto,
        crate::routes::search::SearchResponse,
        crate::routes::search::GroupDto,
        crate::routes::search::SearchHitDto,
        crate::routes::boms::LineInfoDto,
        crate::routes::boms::SummaryDto,
        crate::routes::sec_params::SysParamDto,
        crate::routes::sec_params::SetParamReq,
        crate::routes::template::TemplatePackage,
        crate::routes::template::ImportReq,
        crate::routes::template::ImportResult,
        crate::routes::checklist::ChecklistStep,
        crate::routes::infra::Healthz,
        crate::routes::auth::LoginReq,
        crate::routes::auth::RefreshReq,
        crate::routes::auth::TokenPairDto,
        crate::routes::webhooks::CreateWebhookReq,
        crate::routes::webhooks::WebhookDto,
        crate::routes::meta::TypeMeta,
        crate::routes::meta::SetFlowTierReq,
        crate::routes::boms::LineInputDto,
        crate::routes::parts::CreatePartReq,
    ))
)]
pub struct ApiDocHolder;

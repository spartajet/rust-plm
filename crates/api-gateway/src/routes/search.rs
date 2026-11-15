//! /api/v1/search 路由（增量20 RPT-01 全局搜索）：一处搜一切、按类型聚类；ABAC 密级过滤。
//! 门控 docs:read（与 GET /docs、GET /parts 同层）；数据面过滤在 SearchService 查询内。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::Extension;
use axum::Json;
use core_sec::AuthUser;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct SearchQ {
    /// 关键词（必填；obj_no 精确 ∪ 名称/材料分词）
    pub q: Option<String>,
    /// 钻取类型（缺省聚类返回全部）：drawing | part
    pub r#type: Option<String>,
    pub page: Option<u32>,
    pub size: Option<u32>,
}

/// 检索命中行（与 core_search::SearchHit 同构；OpenAPI 面）
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SearchHitDto {
    pub type_code: String,
    pub obj_no: String,
    pub name: Option<String>,
    pub state: String,
    pub major_ver: i32,
    pub minor_ver: i32,
    pub secret_level: i16,
}

impl From<core_search::SearchHit> for SearchHitDto {
    fn from(h: core_search::SearchHit) -> Self {
        Self {
            type_code: h.type_code,
            obj_no: h.obj_no,
            name: h.name,
            state: h.state,
            major_ver: h.major_ver,
            minor_ver: h.minor_ver,
            secret_level: h.secret_level,
        }
    }
}

/// 类型聚类组
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GroupDto {
    pub type_code: String,
    pub total: u64,
    pub items: Vec<SearchHitDto>,
}

impl From<core_search::GroupResult> for GroupDto {
    fn from(g: core_search::GroupResult) -> Self {
        Self {
            type_code: g.type_code,
            total: g.total,
            items: g.items.into_iter().map(Into::into).collect(),
        }
    }
}

/// 检索响应：聚类（无 type）或平铺分页（有 type）
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SearchResponse {
    pub q: String,
    /// 聚类模式：各类型分组（每类型至多 10 条）；钻取模式：单组分页
    pub groups: Vec<GroupDto>,
    /// 聚合总数（全部组 total 之和）
    pub total: u64,
    /// 钻取分页（有 type 时返回；聚类模式为 1）
    pub page: u32,
    pub size: u32,
}

/// GET /api/v1/search?q=轴承[&type=drawing&page=1&size=20]
#[utoipa::path(
    get, path = "/api/v1/search",
    params(("q" = String, Query, description = "关键词（必填）"), ("type" = String, Query, description = "钻取类型（缺省聚类）"), ("page" = u32, Query), ("size" = u32, Query)),
    responses((status = 200, body = SearchResponse), (status = 400, description = "q 缺失或非法 type"), (status = 401))
)]
pub async fn search(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<SearchQ>,
) -> Result<Json<SearchResponse>, ApiError> {
    let query = q.q.unwrap_or_default().trim().to_string();
    if query.is_empty() {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest("q 不能为空".into()),
            &tid.0,
        ));
    }
    let page = q.page.unwrap_or(1).max(1);
    let size = q.size.unwrap_or(20).clamp(1, 100);
    let (groups, page_out, size_out) = match q.r#type.as_deref() {
        None | Some("") => {
            let gs = st
                .search
                .search_groups(&query, user.max_secret_level, 10)
                .map_err(|e| ApiError::new(e, &tid.0))?;
            (gs, 1, 10)
        }
        Some(t) => {
            if !core_search::SEARCH_TYPES.contains(&t) {
                return Err(ApiError::new(
                    core_model::DomainError::BadRequest(format!(
                        "type 须为 {}，得到 {t}",
                        core_search::SEARCH_TYPES.join("|")
                    )),
                    &tid.0,
                ));
            }
            let (items, total) = st
                .search
                .search_page(&query, t, user.max_secret_level, page, size)
                .map_err(|e| ApiError::new(e, &tid.0))?;
            (
                vec![core_search::GroupResult {
                    type_code: t.to_string(),
                    total,
                    items,
                }],
                page,
                size,
            )
        }
    };
    let total: u64 = groups.iter().map(|g| g.total).sum();
    Ok(Json(SearchResponse {
        q: query,
        groups: groups.into_iter().map(Into::into).collect(),
        total,
        page: page_out,
        size: size_out,
    }))
}

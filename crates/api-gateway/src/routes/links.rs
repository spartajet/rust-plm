//! /api/v1/links 路由（增量27 DOC-12 对象关联）：物料↔文档双向追溯（kind="doc_part" 起步）。
//! 门控：POST/DELETE=docs:write；GET=docs:read（挂两组）。ABAC：双端任一越级 → 404 不泄露。
//! audit：doc.link / doc.unlink。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use core_sec::AuthUser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

/// kind 白名单（域扩展随增；当前仅物料↔文档）
const KINDS: [&str; 1] = ["doc_part"];

#[derive(Deserialize, ToSchema)]
pub struct CreateLinkReq {
    /// 文档编号（doc_part 语义下 from=文档）
    pub from_no: String,
    /// 物料编号（to=物料）
    pub to_no: String,
    pub kind: String,
}

#[derive(Serialize, ToSchema)]
pub struct LinkItem {
    pub link_id: String,
    pub obj_no: String,
    pub name: Option<String>,
    pub class_name: Option<String>,
}

/// POST /api/v1/links — 建关联（docs:write；双端 ABAC；重复 409）
#[utoipa::path(
    post, path = "/api/v1/links",
    request_body = CreateLinkReq,
    responses((status = 201, description = "关联 id"), (status = 400, description = "自关联/kind 非法"), (status = 404, description = "双端任一不存在或越级"), (status = 409, description = "关联已存在"))
)]
pub async fn create_link(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<CreateLinkReq>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    if !KINDS.contains(&req.kind.as_str()) {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest(format!(
                "kind 须为 {}，得到 {}",
                KINDS.join("|"),
                req.kind
            )),
            &tid.0,
        ));
    }
    if req.from_no == req.to_no {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest("from/to 不得为同一对象（自关联）".into()),
            &tid.0,
        ));
    }
    let pool = &st.pool;
    // 双端解析+ABAC（越级 404 不泄露存在性）
    let resolve = |no: &str, type_code: &str| {
        let pool = pool.clone();
        let no = no.to_string();
        let type_code = type_code.to_string();
        async move {
            let row: Option<(uuid::Uuid, i16)> = sqlx::query_as(
                "SELECT id, secret_level FROM cm_object \
                 WHERE type_id = (SELECT id FROM cm_type WHERE code = $2) \
                   AND obj_no = $1 AND deleted_at IS NULL \
                 ORDER BY major_ver DESC, minor_ver DESC, iteration DESC LIMIT 1",
            )
            .bind(&no)
            .bind(&type_code)
            .fetch_optional(&pool)
            .await
            .map_err(core_model::DomainError::from)?;
            let (id, secret) = row
                .ok_or_else(|| core_model::DomainError::ObjNotFound(format!("{type_code}:{no}")))?;
            if secret > 0 {
                // 密级 >0 需上限校验（上限由调用方闭包比较——此处返回 secret 由外层判）
                return Ok::<(uuid::Uuid, i16), core_model::DomainError>((id, secret));
            }
            Ok((id, secret))
        }
    };
    let (from_id, from_secret) = resolve(&req.from_no, "drawing")
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    let (to_id, to_secret) = resolve(&req.to_no, "part")
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    if from_secret > user.max_secret_level || to_secret > user.max_secret_level {
        return Err(ApiError::new(
            core_model::DomainError::ObjNotFound(format!(
                "docs:{}|parts:{}",
                req.from_no, req.to_no
            )),
            &tid.0,
        ));
    }
    let dup: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT id FROM obj_link WHERE from_id = $1 AND to_id = $2 AND kind = $3",
    )
    .bind(from_id)
    .bind(to_id)
    .bind(&req.kind)
    .fetch_optional(pool)
    .await
    .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    if dup.is_some() {
        return Err(ApiError::new(
            core_model::DomainError::LinkDuplicate(format!(
                "{} → {} ({})",
                req.from_no, req.to_no, req.kind
            )),
            &tid.0,
        ));
    }
    let id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO obj_link(from_id, to_id, kind, created_by) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(from_id)
    .bind(to_id)
    .bind(&req.kind)
    .bind(user.id)
    .fetch_one(pool)
    .await
    .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    st.audit.record(
        user.username.clone(),
        "doc.link",
        Some(req.from_no.clone()),
        Some(json!({"to": req.to_no, "kind": req.kind})),
        Some(tid.0.clone()),
        None,
    );
    Ok((StatusCode::CREATED, Json(json!({"id": id}))))
}

/// DELETE /api/v1/links/{id} — 解除关联（docs:write；无此关联 404）
#[utoipa::path(
    delete, path = "/api/v1/links/{id}",
    params(("id" = uuid::Uuid, Path)),
    responses((status = 204), (status = 404))
)]
pub async fn delete_link(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let row: Option<(uuid::Uuid, uuid::Uuid, String)> =
        sqlx::query_as("SELECT from_id, to_id, kind FROM obj_link WHERE id = $1")
            .bind(id)
            .fetch_optional(&st.pool)
            .await
            .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    let Some((from_id, to_id, kind)) = row else {
        return Err(ApiError::new(
            core_model::DomainError::ObjNotFound(format!("link:{id}")),
            &tid.0,
        ));
    };
    // 双端 ABAC（解除任一端越级也不得操作）
    for oid in [from_id, to_id] {
        let secret: i16 = sqlx::query_scalar("SELECT secret_level FROM cm_object WHERE id = $1")
            .bind(oid)
            .fetch_one(&st.pool)
            .await
            .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
        if secret > user.max_secret_level {
            return Err(ApiError::new(
                core_model::DomainError::ObjNotFound(format!("link:{id}")),
                &tid.0,
            ));
        }
    }
    sqlx::query("DELETE FROM obj_link WHERE id = $1")
        .bind(id)
        .execute(&st.pool)
        .await
        .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    let from_no: String = sqlx::query_scalar("SELECT obj_no FROM cm_object WHERE id = $1")
        .bind(from_id)
        .fetch_one(&st.pool)
        .await
        .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    let to_no: String = sqlx::query_scalar("SELECT obj_no FROM cm_object WHERE id = $1")
        .bind(to_id)
        .fetch_one(&st.pool)
        .await
        .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    st.audit.record(
        user.username.clone(),
        "doc.unlink",
        Some(from_no),
        Some(json!({"to": to_no, "kind": kind})),
        Some(tid.0.clone()),
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}

async fn links_for(
    st: &AppState,
    tid: &crate::error::TraceId,
    user: &AuthUser,
    anchor_no: &str,
    anchor_type: &str,
    _other_type: &str,
    anchor_side: &str,
) -> Result<Json<serde_json::Value>, ApiError> {
    // 锚端 ABAC：锚对象越级 404
    let anchor: Option<(uuid::Uuid, i16)> = sqlx::query_as(
        "SELECT id, secret_level FROM cm_object \
         WHERE type_id = (SELECT id FROM cm_type WHERE code = $2) \
           AND obj_no = $1 AND deleted_at IS NULL \
         ORDER BY major_ver DESC, minor_ver DESC, iteration DESC LIMIT 1",
    )
    .bind(anchor_no)
    .bind(anchor_type)
    .fetch_optional(&st.pool)
    .await
    .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    let Some((anchor_id, secret)) = anchor else {
        return Err(ApiError::new(
            core_model::DomainError::ObjNotFound(format!("{anchor_type}:{anchor_no}")),
            &tid.0,
        ));
    };
    if secret > user.max_secret_level {
        return Err(ApiError::new(
            core_model::DomainError::ObjNotFound(format!("{anchor_type}:{anchor_no}")),
            &tid.0,
        ));
    }
    // 对端列表（对端密级过滤行级——不泄露）
    let (join_col, side_col) = if anchor_side == "from" {
        ("to_id", "l.from_id")
    } else {
        ("from_id", "l.to_id")
    };
    let sql = format!(
        "SELECT l.id, o.obj_no, o.attrs->>'name' AS name, o.attrs->>'class_name' AS class_name \
         FROM obj_link l JOIN cm_object o ON o.id = l.{join_col} \
         WHERE {side_col} = $1 AND l.kind = 'doc_part' \
           AND o.deleted_at IS NULL AND o.secret_level <= $2 \
         ORDER BY o.obj_no"
    );
    let rows: Vec<(uuid::Uuid, String, Option<String>, Option<String>)> = sqlx::query_as(&sql)
        .bind(anchor_id)
        .bind(user.max_secret_level)
        .fetch_all(&st.pool)
        .await
        .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    Ok(Json(json!({
        "items": rows
            .into_iter()
            .map(|(id, no, name, cls)| {
                let item: LinkItem = LinkItem {
                    link_id: id.to_string(),
                    obj_no: no,
                    name,
                    class_name: cls,
                };
                item
            })
            .collect::<Vec<_>>(),
    })))
}

/// GET /api/v1/docs/{no}/links — 文档挂的物料（docs:read；对端密级过滤）
#[utoipa::path(
    get, path = "/api/v1/docs/{no}/links",
    params(("no" = String, Path)),
    responses((status = 200, description = "{items:[{link_id,obj_no,name,class_name}]}"), (status = 404))
)]
pub async fn doc_links(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(no): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    links_for(&st, &tid, &user, &no, "drawing", "part", "from").await
}

/// GET /api/v1/parts/{no}/links — 物料挂的文档（docs:read；对端密级过滤）
#[utoipa::path(
    get, path = "/api/v1/parts/{no}/links",
    params(("no" = String, Path)),
    responses((status = 200, description = "{items:[{link_id,obj_no,name,class_name}]}"), (status = 404))
)]
pub async fn part_links(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(no): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    links_for(&st, &tid, &user, &no, "part", "drawing", "to").await
}

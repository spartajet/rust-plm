//! /api/v1/meta/template 路由（增量24 AGL-05 行业模板包）：导出/Starter 内置包/导入（冲突检测+dry_run）。
//! 门控 sec:manage（配置面写操作同层）；导入 audit 显式留痕。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::Extension;
use axum::Json;
use core_sec::AuthUser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::ToSchema;

/// 内置 Starter 基础包（AGL-05"随产品分发"——include_str! 随二进制，零资产部署）
pub const STARTER_JSON: &str = include_str!("../../../../templates/starter.json");

const FORMAT: &str = "rustplm-template";

/// 模板包（AGL-05 单一文件格式；导出与导入同 schema——往返自洽）
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TemplatePackage {
    pub format: String,
    pub version: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<String>,
    pub types: Vec<TplType>,
    pub classes: Vec<TplClass>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TplType {
    pub code: String,
    pub name: String,
    pub base_type: String,
    pub config: Value,
    pub attrs: Vec<TplAttr>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TplAttr {
    pub code: String,
    pub name: String,
    pub data_type: String,
    pub required: bool,
    pub sort_no: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TplClass {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub parent_code: Option<String>,
    #[serde(default)]
    pub cod_rule: Option<Value>,
}

/// GET /api/v1/meta/template/export — 导出全量类型+属性+分类（AGL-05）
#[utoipa::path(
    get, path = "/api/v1/meta/template/export",
    responses((status = 200, body = TemplatePackage), (status = 403))
)]
pub async fn export_template(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
) -> Result<Json<Value>, ApiError> {
    let pool = &st.pool;
    let types: Vec<(String, String, String, Value)> =
        sqlx::query_as("SELECT code, name, base_type, config FROM cm_type ORDER BY code")
            .fetch_all(pool)
            .await
            .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    let mut tpl_types = Vec::new();
    for (code, name, base_type, config) in types {
        let attrs: Vec<(String, String, String, bool, i32)> = sqlx::query_as(
            "SELECT code, name, data_type, required, sort_no FROM cm_attr \
             WHERE type_id = (SELECT id FROM cm_type WHERE code = $1) ORDER BY sort_no",
        )
        .bind(&code)
        .fetch_all(pool)
        .await
        .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
        tpl_types.push(json!({
            "code": code,
            "name": name,
            "base_type": base_type,
            "config": config,
            "attrs": attrs.into_iter().map(|(c, n, d, r, s)| json!({
                "code": c, "name": n, "data_type": d, "required": r, "sort_no": s,
            })).collect::<Vec<_>>(),
        }));
    }
    let classes: Vec<(String, String, Option<String>, Option<Value>)> = sqlx::query_as(
        "SELECT c.code, c.name, p.code, c.cod_rule FROM cm_class c \
         LEFT JOIN cm_class p ON p.id = c.parent_id ORDER BY c.code",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    let tpl_classes: Vec<_> = classes
        .into_iter()
        .map(|(code, name, parent, rule)| {
            json!({
                "code": code, "name": name,
                "parent_code": parent,
                "cod_rule": rule,
            })
        })
        .collect();
    Ok(Json(json!({
        "format": FORMAT,
        "version": 1,
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "types": tpl_types,
        "classes": tpl_classes,
    })))
}

/// GET /api/v1/meta/template/starter — 内置 Starter 基础包（随二进制分发）
#[utoipa::path(
    get, path = "/api/v1/meta/template/starter",
    responses((status = 200, body = TemplatePackage), (status = 403))
)]
pub async fn starter_template() -> Result<Json<Value>, ApiError> {
    let v: Value = serde_json::from_str(STARTER_JSON).expect("内置 starter.json 编译期校验通过");
    Ok(Json(v))
}

#[derive(Deserialize, ToSchema)]
pub struct ImportReq {
    pub template: Value,
    /// dry_run=仅冲突检测不落库（导入向导预演）；merge=应用新增
    pub mode: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportResult {
    pub applied_types: Vec<String>,
    pub applied_classes: Vec<String>,
    /// 冲突：已存在/父缺失等（dry_run 与 merge 一致语义——merge 时冲突项跳过）
    pub conflicts: Vec<Value>,
}

/// POST /api/v1/meta/template/import — 导入（冲突检测；dry_run 预演不落库）
#[utoipa::path(
    post, path = "/api/v1/meta/template/import",
    request_body = ImportReq,
    responses((status = 200, body = ImportResult), (status = 400, description = "format/version 非法"), (status = 403))
)]
pub async fn import_template(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<ImportReq>,
) -> Result<Json<ImportResult>, ApiError> {
    if req.template["format"].as_str() != Some(FORMAT) {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest(format!("template.format 须为 {FORMAT}")),
            &tid.0,
        ));
    }
    if req.template["version"].as_i64() != Some(1) {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest("template.version 须为 1".into()),
            &tid.0,
        ));
    }
    if req.mode != "dry_run" && req.mode != "merge" {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest(format!(
                "mode 须为 dry_run|merge，得到 {}",
                req.mode
            )),
            &tid.0,
        ));
    }
    // 解析（serde 强类型校验字段形状）
    let pkg: TemplatePackage = serde_json::from_value(req.template.clone()).map_err(|e| {
        ApiError::new(
            core_model::DomainError::BadRequest(format!("template 结构不合法: {e}")),
            &tid.0,
        )
    })?;
    let pool = &st.pool;
    let merge = req.mode == "merge";

    let mut applied_types = Vec::new();
    let mut applied_classes = Vec::new();
    let mut conflicts: Vec<Value> = Vec::new();

    // 类型：存在即 skip（部分合并语义复杂且危险——向导如实报告，裁定见 migrations.md）
    for t in &pkg.types {
        let exists: Option<uuid::Uuid> =
            sqlx::query_scalar("SELECT id FROM cm_type WHERE code = $1")
                .bind(&t.code)
                .fetch_optional(pool)
                .await
                .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
        if exists.is_some() {
            conflicts.push(json!({"kind": "type_exists", "key": t.code}));
            continue;
        }
        if merge {
            let id: uuid::Uuid = sqlx::query_scalar(
                "INSERT INTO cm_type(code, name, base_type, config) VALUES ($1, $2, $3, $4::jsonb) RETURNING id",
            )
            .bind(&t.code)
            .bind(&t.name)
            .bind(&t.base_type)
            .bind(&t.config)
            .fetch_one(pool)
            .await
            .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
            for a in &t.attrs {
                sqlx::query(
                    "INSERT INTO cm_attr(type_id, code, name, data_type, required, sort_no) \
                     VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(id)
                .bind(&a.code)
                .bind(&a.name)
                .bind(&a.data_type)
                .bind(a.required)
                .bind(a.sort_no)
                .execute(pool)
                .await
                .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
            }
            applied_types.push(t.code.clone());
        }
    }

    // 分类：同码 → conflict（报告现值）；merge 时父码须已存在（模板内或库内）
    for c in &pkg.classes {
        let exists: Option<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object('cod_rule', cod_rule) FROM cm_class WHERE code = $1",
        )
        .bind(&c.code)
        .fetch_optional(pool)
        .await
        .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
        if exists.is_some() {
            conflicts.push(json!({"kind": "class_exists", "key": c.code}));
            continue;
        }
        let parent_id: Option<uuid::Uuid> = match &c.parent_code {
            Some(pc) => {
                let pid: Option<uuid::Uuid> =
                    sqlx::query_scalar("SELECT id FROM cm_class WHERE code = $1")
                        .bind(pc)
                        .fetch_optional(pool)
                        .await
                        .map_err(|e| {
                            ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0)
                        })?;
                if pid.is_none() {
                    conflicts.push(json!({"kind": "parent_missing", "key": c.code, "parent": pc}));
                    continue;
                }
                pid
            }
            None => None,
        };
        if merge {
            sqlx::query(
                "INSERT INTO cm_class(code, name, parent_id, cod_rule) VALUES ($1, $2, $3, $4::jsonb)",
            )
            .bind(&c.code)
            .bind(&c.name)
            .bind(parent_id)
            .bind(&c.cod_rule)
            .execute(pool)
            .await
            .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
            applied_classes.push(c.code.clone());
        }
    }

    if merge {
        // 类型注册表热刷新触发（cm_meta_changed 由触发器监听 UPDATE/INSERT——同 flow_tier 语义）
        st.audit.record(
            user.username.clone(),
            "template.import",
            None,
            Some(json!({
                "mode": req.mode,
                "types": applied_types,
                "classes": applied_classes,
                "conflicts": conflicts.len(),
            })),
            Some(tid.0.clone()),
            None,
        );
    }
    Ok(Json(ImportResult {
        applied_types,
        applied_classes,
        conflicts,
    }))
}

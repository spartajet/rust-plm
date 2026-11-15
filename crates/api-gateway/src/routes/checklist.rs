//! /api/v1/meta/impl-checklist 路由（增量24 AGL-09 实施导航）：八步 7 天上线 Checklist。
//! auto 步实时判定（数据面查询）；manual 步 sys_param 持久化（复用参数中心，无新迁移）。
//! GET 认证即可（向导全员可见）；complete 需 sec:manage 且按序。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use core_sec::AuthUser;
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;

const CHECKLIST_KEY: &str = "impl_checklist_done";
/// 八步 key（顺序即依赖序；manual 步须按序完成）
const STEPS: [&str; 8] = [
    "org", "perm", "coding", "types", "flow", "data", "training", "cutover",
];

#[derive(Serialize, ToSchema)]
pub struct ChecklistStep {
    pub key: String,
    pub title: String,
    /// 操作指引（AGL-09"每步含操作指引"）
    pub hint: String,
    pub done: bool,
    /// true=数据面自动判定；false=人工标记
    pub auto: bool,
}

/// 人工已完成步骤集合（sys_param JSON 数组；损坏兜底空集）
async fn manual_done(st: &AppState) -> Vec<String> {
    let v: Option<String> = sqlx::query_scalar("SELECT value FROM sys_param WHERE key = $1")
        .bind(CHECKLIST_KEY)
        .fetch_optional(&st.pool)
        .await
        .unwrap_or(None);
    v.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

/// auto 步实时判定（数据面查询；缺省 false）
async fn auto_done(st: &AppState, key: &str) -> bool {
    async fn q(st: &AppState, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(&st.pool)
            .await
            .unwrap_or(0)
    }
    match key {
        "org" => q(st, "SELECT count(*) FROM sec_user").await >= 2,
        "perm" => {
            q(
                st,
                "SELECT count(*) FROM sec_user_role WHERE username <> 'admin'",
            )
            .await
                >= 1
        }
        "coding" => {
            q(
                st,
                "SELECT count(*) FROM cm_class WHERE cod_rule IS NOT NULL",
            )
            .await
                >= 1
        }
        "types" => q(st, "SELECT count(*) FROM cm_attr").await >= 5,
        "flow" => {
            q(
                st,
                "SELECT count(*) FROM cm_type WHERE config ? 'flow_tier'",
            )
            .await
                >= 1
        }
        "data" => {
            q(
                st,
                "SELECT count(*) FROM cm_object WHERE deleted_at IS NULL",
            )
            .await
                >= 1
        }
        _ => false,
    }
}

fn meta(key: &str) -> (&'static str, &'static str, bool) {
    match key {
        "org" => ("组织成员", "用户管理中创建真实成员账号（≥2 人）", true),
        "perm" => ("权限配置", "为成员授予 editor/viewer 角色", true),
        "coding" => ("编码规则", "物料分类上配置编码规则（自动赋码）", true),
        "types" => ("类型建模", "为类型添加自定义属性（超出种子属性）", true),
        "flow" => (
            "流程绑定",
            "在元数据中确认审批档位（minimal/standard/strict）",
            true,
        ),
        "data" => ("数据导入", "上传首批图纸或创建物料", true),
        "training" => ("培训交付", "组织成员完成系统操作培训", false),
        "cutover" => ("切换上线", "正式切换到 RustPLM 受控（停用旧共享盘）", false),
        _ => ("", "", false),
    }
}

/// GET /api/v1/meta/impl-checklist — 八步状态（认证即可；向导全员可见）
#[utoipa::path(
    get, path = "/api/v1/meta/impl-checklist",
    responses((status = 200, body = Vec<ChecklistStep>), (status = 401))
)]
pub async fn get_checklist(
    State(st): State<AppState>,
) -> Result<Json<Vec<ChecklistStep>>, ApiError> {
    let done_set = manual_done(&st).await;
    let mut steps = Vec::new();
    for key in STEPS {
        let (title, hint, auto) = meta(key);
        let done = if auto {
            auto_done(&st, key).await
        } else {
            done_set.iter().any(|k| k == key)
        };
        steps.push(ChecklistStep {
            key: key.to_string(),
            title: title.to_string(),
            hint: hint.to_string(),
            done,
            auto,
        });
    }
    Ok(Json(steps))
}

/// POST /api/v1/meta/impl-checklist/{step}/complete — 人工步骤标记完成（sec:manage；按序）
#[utoipa::path(
    post, path = "/api/v1/meta/impl-checklist/{step}/complete",
    params(("step" = String, Path)),
    responses((status = 204), (status = 400, description = "非人工步骤/前置未完成"), (status = 403))
)]
pub async fn complete_step(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Extension(user): Extension<AuthUser>,
    Path(step): Path<String>,
) -> Result<StatusCode, ApiError> {
    let Some(idx) = STEPS.iter().position(|k| *k == step) else {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest(format!("未知步骤: {step}")),
            &tid.0,
        ));
    };
    let (_, _, auto) = meta(step.as_str());
    if auto {
        return Err(ApiError::new(
            core_model::DomainError::BadRequest(format!(
                "步骤 {step} 由系统自动判定，无需人工标记"
            )),
            &tid.0,
        ));
    }
    // 按序约束：前置 manual 步（training→cutover 链）须已完成
    let done_set = manual_done(&st).await;
    for prev in STEPS.iter().take(idx) {
        let (_, _, prev_auto) = meta(prev);
        if !prev_auto && !done_set.iter().any(|k| k == prev) {
            return Err(ApiError::new(
                core_model::DomainError::BadRequest(format!("前置步骤 {prev} 未完成")),
                &tid.0,
            ));
        }
    }
    // 持久化（sys_param JSON 数组）
    let mut next = done_set.clone();
    if !next.iter().any(|k| k == &step) {
        next.push(step.clone());
    }
    sqlx::query(
        "INSERT INTO sys_param(key, value, updated_by) VALUES ($1, $2, $3) \
         ON CONFLICT (key) DO UPDATE SET value = $2, updated_by = $3, updated_at = now()",
    )
    .bind(CHECKLIST_KEY)
    .bind(serde_json::to_string(&next).unwrap_or_default())
    .bind(user.id)
    .execute(&st.pool)
    .await
    .map_err(|e| ApiError::new(core_model::DomainError::Db(e.to_string()), &tid.0))?;
    // 留痕（SYS-04 同语义——实施进度是管理动作）
    st.audit.record(
        user.username.clone(),
        "impl-checklist.complete",
        Some(step.clone()),
        None,
        Some(tid.0.clone()),
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}

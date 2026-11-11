//! ObjectStore —— 业务对象统一存取（概设 §3.1 契约的首次实现）。
//!
//! 事务语义：
//! - create：类型校验→默认值填充→save_before 钩子→编号唯一预检→INSERT
//! - update：仅 working 态；patch merge；save_before 钩子；乐观锁（attrs_rev），
//!   冲突返回 ObjVersionConflict(409)；
//! - query：QueryBuilder 编译（热路径 100% 索引，Spike V-1 规则）；
//! - 发布态对象 update 直接拒绝（ObjStateForbidden）——升版只能经变更流程（core-version/mod-ec）。

use crate::query::{CompiledQuery, ObjectQuery, QueryBuilder};
use crate::registry::TypeRegistry;
use crate::types::{AttrMap, BizObject, TypeDef};
use crate::{DomainError, HookOutcome, HookPoint, HookRegistry, Result};
use async_trait::async_trait;
use sqlx::postgres::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// 创建命令（obj_no 由调用方赋码：COD 编码系统或显式指定）
pub struct CreateCmd {
    pub type_code: String,
    pub obj_no: String,
    pub attrs: AttrMap,
    pub owner: Option<Uuid>,
    pub secret_level: i16,
}

#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn create(&self, cmd: CreateCmd) -> Result<BizObject>;
    async fn get(&self, type_code: &str, obj_no: &str, major: i32, minor: i32) -> Result<BizObject>;
    async fn update(&self, type_code: &str, obj_no: &str, major: i32, minor: i32,
                    patch: &AttrMap, expect_rev: i32, actor: Uuid) -> Result<BizObject>;
    async fn relate(&self, rel_code: &str, from: Uuid, to: Uuid) -> Result<()>;
    async fn unrelate(&self, rel_code: &str, from: Uuid, to: Uuid) -> Result<()>;
    async fn query(&self, q: &ObjectQuery) -> Result<(Vec<BizObject>, u64)>;
    async fn soft_delete(&self, type_code: &str, obj_no: &str, actor: Uuid) -> Result<()>;
}

pub struct PgObjectStore {
    pool: PgPool,
    registry: Arc<TypeRegistry>,
    hooks: Arc<HookRegistry>,
}

const OBJ_COLS: &str = "id, type_id, obj_no, major_ver, minor_ver, iteration, state, \
                        secret_level, owner, attrs, attrs_rev, created_at, updated_at, created_by, updated_by";

impl PgObjectStore {
    pub fn new(pool: PgPool, registry: Arc<TypeRegistry>, hooks: Arc<HookRegistry>) -> Self {
        Self { pool, registry, hooks }
    }

    /// 属性校验与默认值填充（类型/必填；枚举深校验由 dict 钩子扩展）
    fn validate_and_fill(td: &TypeDef, attrs: &mut AttrMap) -> Result<()> {
        for a in &td.attrs {
            match attrs.get(&a.code) {
                None | Some(serde_json::Value::Null) => {
                    if let Some(d) = &a.default_val {
                        attrs.insert(a.code.clone(), d.clone());
                    } else if a.required {
                        return Err(DomainError::AttrRequired(a.code.clone()));
                    }
                }
                Some(v) => {
                    let ok = match a.data_type {
                        crate::DataType::Text | crate::DataType::Enum | crate::DataType::Ref => v.is_string(),
                        crate::DataType::Int => v.is_i64(),
                        crate::DataType::Decimal | crate::DataType::Unit => v.is_number(),
                        crate::DataType::Bool => v.is_boolean(),
                        crate::DataType::Date => v.is_string(),
                        crate::DataType::Json => true,
                    };
                    if !ok {
                        return Err(DomainError::AttrTypeMismatch(
                            a.code.clone(),
                            a.data_type.as_str().to_string(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    async fn row_to_obj(&self, td: &TypeDef, row: ObjRow) -> Result<BizObject> {
        Ok(BizObject {
            id: row.id,
            type_id: row.type_id,
            type_code: td.code.clone(),
            obj_no: row.obj_no,
            major: row.major_ver,
            minor: row.minor_ver,
            iteration: row.iteration,
            state: row.state,
            secret_level: row.secret_level,
            owner: row.owner,
            attrs: row.attrs.as_object().cloned().unwrap_or_default(),
            attrs_rev: row.attrs_rev,
            created_at: row.created_at,
            updated_at: row.updated_at,
            created_by: row.created_by,
            updated_by: row.updated_by,
        })
    }
}

#[derive(sqlx::FromRow)]
struct ObjRow {
    id: Uuid,
    type_id: Uuid,
    obj_no: String,
    major_ver: i32,
    minor_ver: i32,
    iteration: i32,
    state: String,
    secret_level: i16,
    owner: Option<Uuid>,
    attrs: serde_json::Value,
    attrs_rev: i32,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    updated_at: Option<chrono::DateTime<chrono::Utc>>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
}

#[async_trait]
impl ObjectStore for PgObjectStore {
    async fn create(&self, cmd: CreateCmd) -> Result<BizObject> {
        let td = self.registry.type_def(&cmd.type_code)?;
        let mut attrs = cmd.attrs;

        // 编号唯一性预检（返回语义化错误；并发兜底靠唯一索引）
        let dup: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM cm_object WHERE type_id=$1 AND obj_no=$2 \
             AND major_ver=1 AND minor_ver=0 AND deleted_at IS NULL LIMIT 1")
            .bind(td.id)
            .bind(&cmd.obj_no)
            .fetch_optional(&self.pool)
            .await?;
        if dup.is_some() {
            return Err(DomainError::ObjNoDuplicate(format!("{}:{}", cmd.type_code, cmd.obj_no)));
        }

        Self::validate_and_fill(&td, &mut attrs)?;

        // save_before 钩子（拒绝则中止；空注册表为 no-op）
        let ctx = crate::HookContext {
            type_code: td.code.clone(),
            obj_no: cmd.obj_no.clone(),
            state: "working".into(),
            attrs: attrs.clone(),
            actor: cmd.owner.unwrap_or_default(),
        };
        if let HookOutcome::Denied(by, reason) =
            self.hooks.run(&HookPoint::SaveBefore, &ctx).await?
        {
            return Err(DomainError::HookRejected(format!("[{by}] {reason}")));
        }

        let row: ObjRow = sqlx::query_as(&format!(
            "INSERT INTO cm_object(type_id, obj_no, attrs, secret_level, owner, created_by, updated_by) \
             VALUES ($1, $2, $3::jsonb, $4, $5, $5, $5) RETURNING {OBJ_COLS}"
        ))
        .bind(td.id)
        .bind(&cmd.obj_no)
        .bind(serde_json::Value::Object(attrs))
        .bind(cmd.secret_level)
        .bind(cmd.owner)
        .fetch_one(&self.pool)
        .await?;
        self.row_to_obj(&td, row).await
    }

    async fn get(&self, type_code: &str, obj_no: &str, major: i32, minor: i32) -> Result<BizObject> {
        let td = self.registry.type_def(type_code)?;
        let row: Option<ObjRow> = sqlx::query_as(&format!(
            "SELECT {OBJ_COLS} FROM cm_object \
             WHERE type_id=$1 AND obj_no=$2 AND major_ver=$3 AND minor_ver=$4 AND deleted_at IS NULL"
        ))
        .bind(td.id)
        .bind(obj_no)
        .bind(major)
        .bind(minor)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => self.row_to_obj(&td, r).await,
            None => Err(DomainError::ObjNotFound(format!("{type_code}:{obj_no}:{major}.{minor}"))),
        }
    }

    async fn update(&self, type_code: &str, obj_no: &str, major: i32, minor: i32,
                    patch: &AttrMap, expect_rev: i32, actor: Uuid) -> Result<BizObject> {
        let td = self.registry.type_def(type_code)?;
        let cur = self.get(type_code, obj_no, major, minor).await?;
        if cur.state != "working" {
            return Err(DomainError::ObjStateForbidden(format!(
                "{type_code}:{obj_no} state={}", cur.state
            )));
        }

        let mut merged = cur.attrs.clone();
        for (k, v) in patch {
            merged.insert(k.clone(), v.clone());
        }
        Self::validate_and_fill(&td, &mut merged)?;

        let ctx = crate::HookContext {
            type_code: td.code.clone(),
            obj_no: obj_no.to_string(),
            state: cur.state.clone(),
            attrs: merged.clone(),
            actor,
        };
        if let HookOutcome::Denied(by, reason) = self.hooks.run(&HookPoint::SaveBefore, &ctx).await? {
            return Err(DomainError::HookRejected(format!("[{by}] {reason}")));
        }

        let row: Option<ObjRow> = sqlx::query_as(&format!(
            "UPDATE cm_object SET attrs = $3::jsonb, attrs_rev = attrs_rev + 1, \
                    iteration = iteration + 1, updated_at = now(), updated_by = $4 \
             WHERE id = $1 AND attrs_rev = $2 AND deleted_at IS NULL \
             RETURNING {OBJ_COLS}"
        ))
        .bind(cur.id)
        .bind(expect_rev)
        .bind(serde_json::Value::Object(merged))
        .bind(actor)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => self.row_to_obj(&td, r).await,
            None => Err(DomainError::ObjVersionConflict(format!("{type_code}:{obj_no}:{major}.{minor}"))),
        }
    }

    async fn relate(&self, rel_code: &str, from: Uuid, to: Uuid) -> Result<()> {
        self.registry.rel_def(rel_code)?; // 关系定义必须存在
        let n = sqlx::query(
            "INSERT INTO cm_rel_inst(rel_code, from_id, to_id) VALUES ($1,$2,$3) \
             ON CONFLICT (rel_code, from_id, to_id) DO NOTHING")
            .bind(rel_code)
            .bind(from)
            .bind(to)
            .execute(&self.pool)
            .await?
            .rows_affected();
        let _ = n;
        Ok(())
    }

    async fn unrelate(&self, rel_code: &str, from: Uuid, to: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM cm_rel_inst WHERE rel_code=$1 AND from_id=$2 AND to_id=$3")
            .bind(rel_code)
            .bind(from)
            .bind(to)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn query(&self, q: &ObjectQuery) -> Result<(Vec<BizObject>, u64)> {
        let td = self.registry.type_def(&q.type_code)?;
        let cq: CompiledQuery = QueryBuilder::compile(td.clone(), q)?;

        // 绑定执行（值按 JSON 传递 + cast，与编译占位一一对应）
        let mut base = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT id, type_id, obj_no, major_ver, minor_ver, iteration, state, \
                    secret_level, owner, attrs, attrs_rev, created_at, updated_at, created_by, updated_by \
             FROM cm_object WHERE type_id = ");
        base.push_bind(td.id);
        base.push(" AND (").push(cq.where_sql.as_str()).push(") ORDER BY ").push(cq.order_sql.as_str());
        base.push(" LIMIT ").push(cq.limit as i64).push(" OFFSET ").push(cq.offset as i64);

        let mut sel = base.build_query_as::<ObjRow>();
        for b in &cq.binds {
            sel = sel.bind(b.value.to_string());
        }
        let rows: Vec<ObjRow> = sel.fetch_all(&self.pool).await?;

        let mut cnt = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT count(*) FROM cm_object WHERE type_id = ");
        cnt.push_bind(td.id);
        cnt.push(" AND (").push(cq.where_sql.as_str()).push(")");
        let mut qcnt = cnt.build_query_scalar::<i64>();
        for b in &cq.binds {
            qcnt = qcnt.bind(b.value.to_string());
        }
        let total: i64 = qcnt.fetch_one(&self.pool).await?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(self.row_to_obj(&td, r).await?);
        }
        Ok((out, total as u64))
    }

    async fn soft_delete(&self, type_code: &str, obj_no: &str, actor: Uuid) -> Result<()> {
        let td = self.registry.type_def(type_code)?;
        let n = sqlx::query(
            "UPDATE cm_object SET deleted_at = now(), updated_by = $3 \
             WHERE type_id=$1 AND obj_no=$2 AND deleted_at IS NULL AND state = 'working'")
            .bind(td.id)
            .bind(obj_no)
            .bind(actor)
            .execute(&self.pool)
            .await?
            .rows_affected();
        if n == 0 {
            return Err(DomainError::ObjNotFound(format!("{type_code}:{obj_no}")));
        }
        Ok(())
    }
}

/// 兼容保留：绑定以 JSON 文本传递，由占位处 PG 端 cast 落列（jsonb/numeric/uuid/timestamptz 均安全）
#[allow(dead_code)]
fn bind_json_text(v: &serde_json::Value) -> String {
    v.to_string()
}

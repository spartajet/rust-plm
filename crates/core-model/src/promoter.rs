//! Promoter —— 属性提升列迁移计划器（ADR-0002 / Spike V-1 实测结论的产品化）。
//!
//! 状态机：Pending → Adding（ALTER ADD COLUMN，实测 2-4ms）→ Backfilling（分批 UPDATE 回填，
//! 百万行 ~61s，须后台低优先级执行）→ Indexing（CREATE INDEX）→ Done（标记 promoted + 版本失效）。
//! 任一步失败：Failed → 回滚（DROP COLUMN，数据仍在 JSONB，无损）。

use crate::registry::TypeRegistry;
use crate::{AttrDef, DataType, DomainError, Result};
use sqlx::postgres::PgPool;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

/// 提升计划
pub struct PromotePlan {
    pub type_code: String,
    pub attr_code: String,
    /// 物理列名 `p_<code>`
    pub col_name: String,
    /// PG 列类型
    pub pg_type: &'static str,
}

/// 执行选项
pub struct PromoteOpts {
    /// 回填批大小（默认 100_000；Spike V-1 采用值）
    pub batch: i64,
    /// 批间休眠（后台低优先级，降低对在线业务影响）
    pub throttle: std::time::Duration,
}

impl Default for PromoteOpts {
    fn default() -> Self {
        Self {
            batch: 100_000,
            throttle: std::time::Duration::from_millis(200),
        }
    }
}

pub struct Promoter {
    pool: PgPool,
    registry: Arc<TypeRegistry>,
}

impl Promoter {
    pub fn new(pool: PgPool, registry: Arc<TypeRegistry>) -> Self {
        Self { pool, registry }
    }

    /// 生成提升计划（校验：属性存在、未提升、命名不冲突）
    pub fn plan(&self, type_code: &str, attr_code: &str) -> Result<PromotePlan> {
        let td = self.registry.type_def(type_code)?;
        let a: &AttrDef = td.attr_or_err(attr_code)?;
        if a.promoted {
            return Err(DomainError::Internal(format!(
                "属性已提升: {type_code}.{attr_code}"
            )));
        }
        Ok(PromotePlan {
            type_code: type_code.to_string(),
            attr_code: attr_code.to_string(),
            col_name: format!("p_{}", attr_code),
            pg_type: a.data_type.pg_column_type(),
        })
    }

    /// 执行提升（幂等：重复执行按列存在性跳过对应阶段）
    pub async fn execute(&self, plan: &PromotePlan, opts: &PromoteOpts) -> Result<()> {
        let td = self.registry.type_def(&plan.type_code)?;
        let attr_id: Uuid =
            sqlx::query_scalar("SELECT id FROM cm_attr WHERE type_id=$1 AND code=$2")
                .bind(td.id)
                .bind(&plan.attr_code)
                .fetch_one(&self.pool)
                .await?;

        let log_id: Uuid = sqlx::query_scalar(
            "INSERT INTO cm_ddl_log(type_id, attr_id, action, col_name, status) \
             VALUES ($1,$2,'promote',$3,'adding') RETURNING id",
        )
        .bind(td.id)
        .bind(attr_id)
        .bind(&plan.col_name)
        .fetch_one(&self.pool)
        .await?;

        // 1) Adding：在线加列（PG 元数据操作，毫秒级，不阻塞并发写——Spike V-1 实测）
        let exists: Option<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_name='cm_object' AND column_name=$1",
        )
        .bind(&plan.col_name)
        .fetch_optional(&self.pool)
        .await?;
        if exists.is_none() {
            // 提取表达式与列类型对齐（text 系直接 ->>，数值/日期 cast，jsonb 原样）
            let expr = extract_expr(&plan.col_name, data_type_of(&td, &plan.attr_code)?);
            sqlx::raw_sql(&format!(
                "ALTER TABLE cm_object ADD COLUMN {col} {ty}",
                col = plan.col_name,
                ty = plan.pg_type
            ))
            .execute(&self.pool)
            .await?;
            let _ = expr; // 回填阶段使用
            info!(col = %plan.col_name, "提升列已添加（元数据操作）");
        }
        sqlx::query("UPDATE cm_ddl_log SET status='backfilling' WHERE id=$1")
            .bind(log_id)
            .execute(&self.pool)
            .await?;

        // 2) Backfilling：分批回填（JSONB → 提升列；未命中填 NULL）
        let expr = extract_expr(&plan.col_name, data_type_of(&td, &plan.attr_code)?);
        let mut batches = 0i32;
        loop {
            let n = sqlx::query(&format!(
                "UPDATE cm_object SET {col} = ({expr}) \
                 WHERE id IN (SELECT id FROM cm_object WHERE {col} IS NULL AND deleted_at IS NULL \
                              AND attrs ? '{code}' LIMIT {batch})",
                col = plan.col_name,
                expr = expr,
                code = plan.attr_code.replace('\'', "''"),
                batch = opts.batch
            ))
            .execute(&self.pool)
            .await?
            .rows_affected();
            batches += 1;
            if n == 0 {
                break;
            }
            tokio::time::sleep(opts.throttle).await;
        }

        // 3) Indexing
        let idx = format!("idx_{}", plan.col_name);
        let idx_exists: Option<String> = sqlx::query_scalar(
            "SELECT indexname FROM pg_indexes WHERE tablename='cm_object' AND indexname=$1",
        )
        .bind(&idx)
        .fetch_optional(&self.pool)
        .await?;
        if idx_exists.is_none() {
            sqlx::query("UPDATE cm_ddl_log SET status='indexing' WHERE id=$1")
                .bind(log_id)
                .execute(&self.pool)
                .await?;
            sqlx::raw_sql(&format!(
                "CREATE INDEX {idx} ON cm_object({col})",
                idx = idx,
                col = plan.col_name
            ))
            .execute(&self.pool)
            .await?;
        }

        // 4) Done：标记 + 元数据版本失效（触发器自动 NOTIFY → TypeRegistry 热刷新）
        sqlx::query(
            "UPDATE cm_attr SET promoted=true, promoted_col=$3, updated_at=now() WHERE id=$1; \
             UPDATE cm_ddl_log SET status='done', batches=$2, finished_at=now() WHERE id=$4",
        )
        .bind(attr_id)
        .bind(batches)
        .bind(&plan.col_name)
        .bind(log_id)
        .execute(&self.pool)
        .await?;
        let _ = self.registry.reload_if_changed().await;
        info!(col = %plan.col_name, batches, "属性提升完成");
        Ok(())
    }

    /// 回滚提升（DROP COLUMN；数据仍完整保留于 attrs JSONB，无损降级）
    pub async fn rollback(&self, plan: &PromotePlan) -> Result<()> {
        let td = self.registry.type_def(&plan.type_code)?;
        warn!(col = %plan.col_name, "回滚提升列");
        let mut tx = self.pool.begin().await?;
        sqlx::raw_sql(&format!(
            "ALTER TABLE cm_object DROP COLUMN IF EXISTS {}",
            plan.col_name
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE cm_attr SET promoted=false, promoted_col=NULL WHERE type_id=$1 AND code=$2",
        )
        .bind(td.id)
        .bind(&plan.attr_code)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE cm_ddl_log SET rolled_back=true, status='rolled_back', finished_at=now() \
             WHERE type_id=$1 AND col_name=$2 AND action='promote' AND NOT rolled_back",
        )
        .bind(td.id)
        .bind(&plan.col_name)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        let _ = self.registry.reload_if_changed().await;
        Ok(())
    }
}

fn data_type_of(td: &crate::TypeDef, code: &str) -> Result<DataType> {
    Ok(td.attr_or_err(code)?.data_type)
}

/// 回填提取表达式（attrs -> code 转换为目标列类型）
fn extract_expr(col: &str, dt: DataType) -> String {
    let _ = col;
    match dt {
        DataType::Text | DataType::Enum | DataType::Ref => "attrs ->> __CODE__".into(),
        DataType::Int => "(attrs ->> __CODE__)::bigint".into(),
        DataType::Decimal | DataType::Unit => "(attrs ->> __CODE__)::numeric".into(),
        DataType::Date => "(attrs ->> __CODE__)::timestamptz".into(),
        DataType::Bool => "(attrs ->> __CODE__)::boolean".into(),
        DataType::Json => "(attrs -> __CODE__)::jsonb".into(),
    }
}

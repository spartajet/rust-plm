//! LifecycleStore：单事务迁移（状态推进 + released 快照 + doc.released 事件落 Outbox）。

use crate::machine::{self, LifecycleEvent};
use core_model::{DomainError, Result};
use sqlx::PgPool;
use uuid::Uuid;

pub struct LifecycleStore {
    pool: PgPool,
}

impl LifecycleStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 迁移对象状态（按 type_code+obj_no 定位活版本 v1.0）。
    /// released 时同事务：写快照 + 落 `doc.released` Outbox 行（增量4 外投）。
    pub async fn transition(
        &self,
        type_code: &str,
        obj_no: &str,
        ev: LifecycleEvent,
        actor: Option<Uuid>,
    ) -> Result<core_model::BizObject> {
        let mut tx = self.pool.begin().await?;

        #[derive(sqlx::FromRow)]
        #[allow(dead_code)]
        struct Row {
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
        let row: Row = sqlx::query_as(
            "SELECT id, type_id, obj_no, major_ver, minor_ver, iteration, state, \
                    secret_level, owner, attrs, attrs_rev, created_at, updated_at, created_by, updated_by \
             FROM cm_object WHERE id IN ( \
               SELECT id FROM cm_object WHERE obj_no = $2 AND deleted_at IS NULL \
                 AND type_id = (SELECT id FROM cm_type WHERE code = $1) \
               ORDER BY major_ver DESC, minor_ver DESC LIMIT 1)",
        )
        .bind(type_code)
        .bind(obj_no)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => {
                DomainError::ObjNotFound(format!("{type_code}:{obj_no}"))
            }
            other => DomainError::Db(other.to_string()),
        })?;

        // 档位读取（cm_type.config.flow_tier，缺省 standard；每迁移一次点查——档位热生效优先）
        let tier_str: Option<String> =
            sqlx::query_scalar("SELECT config->>'flow_tier' FROM cm_type WHERE id = $1")
                .bind(row.type_id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();
        let tier = match tier_str.as_deref().map(crate::FlowTier::parse) {
            Some(Some(t)) => t,
            Some(None) => {
                tracing::warn!(raw = ?tier_str, "非法 flow_tier 存量值：降级 standard（strict 强制失效，请修正 cm_type.config）");
                crate::FlowTier::Standard
            }
            None => crate::FlowTier::Standard,
        };

        let new_state = machine::allowed_for(tier, &row.state, ev).ok_or_else(|| {
            DomainError::IllegalTransition(row.state.clone(), ev.as_str().to_string())
        })?;

        // 会签校验（strict checked→released）：第二级审批须异于提交人 **且** 复核人（终审 C1）；
        // actor=None 时 fail-closed（终审 M1：公开 API 不得静默绕过）
        if row.state == "checked" && ev == LifecycleEvent::Approve {
            let Some(a) = actor else {
                return Err(DomainError::MakerCheckerViolation);
            };
            if a == row.updated_by.unwrap_or(a) || row.created_by == Some(a) {
                return Err(DomainError::MakerCheckerViolation);
            }
        }

        let updated_by = actor;
        let obj = sqlx::query_as::<
            _,
            (
                Uuid,
                Uuid,
                String,
                i32,
                i32,
                i32,
                String,
                i16,
                Option<Uuid>,
                serde_json::Value,
                i32,
                Option<chrono::DateTime<chrono::Utc>>,
                Option<chrono::DateTime<chrono::Utc>>,
                Option<Uuid>,
                Option<Uuid>,
            ),
        >(
            // 终审 C2：旧状态入 WHERE 守卫——并发迁移竞态输家得 409，而非静默覆盖
            "UPDATE cm_object SET state = $3, updated_at = now(), updated_by = $4 \
             WHERE id = $1 AND state = $2 \
             RETURNING id, type_id, obj_no, major_ver, minor_ver, iteration, state, \
                       secret_level, owner, attrs, attrs_rev, created_at, updated_at, created_by, updated_by",
        )
        .bind(row.id)
        .bind(&row.state)
        .bind(new_state)
        .bind(updated_by)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            DomainError::IllegalTransition(
                row.state.clone(),
                format!("{} (并发变更)", ev.as_str()),
            )
        })?;

        // released：同事务快照 + 事件
        if new_state == "released" {
            sqlx::query(
                "INSERT INTO cm_obj_snapshot(obj_no, type_code, major_ver, minor_ver, state, attrs, created_by) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(&row.obj_no)
            .bind(type_code)
            .bind(obj.3)
            .bind(obj.4)
            .bind(new_state)
            .bind(&obj.9)
            .bind(updated_by)
            .execute(&mut *tx)
            .await?;
            sqlx::query("INSERT INTO event_outbox(topic, payload) VALUES ('doc.released', $1)")
                .bind(serde_json::json!({
                    "type_code": type_code,
                    "obj_no": row.obj_no,
                    "major": obj.3,
                    "minor": obj.4,
                    "released_at": chrono::Utc::now(),
                }))
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;

        Ok(core_model::BizObject {
            id: obj.0,
            type_id: obj.1,
            type_code: type_code.to_string(),
            obj_no: obj.2,
            major: obj.3,
            minor: obj.4,
            iteration: obj.5,
            state: obj.6,
            secret_level: obj.7,
            owner: obj.8,
            attrs: obj.9.as_object().cloned().unwrap_or_default(),
            attrs_rev: obj.10,
            created_at: obj.11,
            updated_at: obj.12,
            created_by: obj.13,
            updated_by: obj.14,
        })
    }
}

//! AuditService（增量18 SYS-01）：审计留痕 + 检索。append-only——应用层只 INSERT/SELECT。

use serde::Serialize;
use sqlx::PgPool;

pub struct AuditService {
    pool: PgPool,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AuditRow {
    pub id: i64,
    pub at: chrono::DateTime<chrono::Utc>,
    pub username: String,
    pub action: String,
    pub object: Option<String>,
    pub detail: Option<serde_json::Value>,
    pub trace_id: Option<String>,
    pub ip: Option<String>,
}

impl AuditService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 异步留痕（spawn——不阻塞主链路；失败仅 WARN 不影响业务）
    pub fn record(
        &self,
        username: impl Into<String> + Send + 'static,
        action: impl Into<String> + Send + 'static,
        object: Option<String>,
        detail: Option<serde_json::Value>,
        trace_id: Option<String>,
        ip: Option<String>,
    ) {
        let pool = self.pool.clone();
        let username = username.into();
        let action = action.into();
        tokio::spawn(async move {
            let r = sqlx::query(
                "INSERT INTO audit_log(username, action, object, detail, trace_id, ip) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(username)
            .bind(action)
            .bind(object)
            .bind(detail)
            .bind(trace_id)
            .bind(ip)
            .execute(&pool)
            .await;
            if let Err(e) = r {
                tracing::warn!(error = %e, "审计写入失败");
            }
        });
    }

    /// 多维检索（SYS-01：按人/动作/对象/时间窗；分页）
    pub async fn search(
        &self,
        username: Option<&str>,
        action: Option<&str>,
        object: Option<&str>,
        from: Option<chrono::DateTime<chrono::Utc>>,
        to: Option<chrono::DateTime<chrono::Utc>>,
        page: u32,
        size: u32,
    ) -> Result<(Vec<AuditRow>, i64), core_model::DomainError> {
        let rows: Vec<AuditRow> = sqlx::query_as(
            "SELECT id, at, username, action, object, detail, trace_id, ip FROM audit_log \
             WHERE ($1::text IS NULL OR username = $1) \
               AND ($2::text IS NULL OR action = $2) \
               AND ($3::text IS NULL OR object = $3) \
               AND ($4::timestamptz IS NULL OR at >= $4) \
               AND ($5::timestamptz IS NULL OR at <= $5) \
             ORDER BY id DESC LIMIT $6 OFFSET $7",
        )
        .bind(username.filter(|s| !s.trim().is_empty()))
        .bind(action.filter(|s| !s.trim().is_empty()))
        .bind(object.filter(|s| !s.trim().is_empty()))
        .bind(from)
        .bind(to)
        .bind(size as i64)
        .bind(i64::from(page.saturating_sub(1)) * i64::from(size))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| core_model::DomainError::Db(e.to_string()))?;
        let total: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_log \
             WHERE ($1::text IS NULL OR username = $1) \
               AND ($2::text IS NULL OR action = $2) \
               AND ($3::text IS NULL OR object = $3) \
               AND ($4::timestamptz IS NULL OR at >= $4) \
               AND ($5::timestamptz IS NULL OR at <= $5)",
        )
        .bind(username.filter(|s| !s.trim().is_empty()))
        .bind(action.filter(|s| !s.trim().is_empty()))
        .bind(object.filter(|s| !s.trim().is_empty()))
        .bind(from)
        .bind(to)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| core_model::DomainError::Db(e.to_string()))?;
        Ok((rows, total))
    }
}

//! EventDispatcher：拉取 pending 事件 → 按订阅扇出 → HMAC 签名 POST → 退避重试 → 死信。
//! 状态机：pending(attempts=0) → 成功 dispatched(1) / 失败 attempts+1 且 next_retry_at=+退避；
//! attempts≥3 → dead(2)。成功后不再投（status CAS 0→1）。

use crate::repo::WebhookRepo;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use sqlx::PgPool;
use std::time::Duration;
use tracing::warn;

type HmacSha256 = Hmac<Sha256>;

pub struct EventDispatcher {
    pool: PgPool,
    http: reqwest::Client,
    /// 重试退避（秒）：第 n 次失败后取 v[(n-1).clamp(0, len-1)]——计划语义 1m/5m/30m
    pub retry_backoff_secs: Vec<i64>,
}

const MAX_ATTEMPTS: i32 = 4; // 首发 + 3 次重试（三档退避各用一次）

impl EventDispatcher {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            // 终审 I6：不跟随重定向（30x 会丢 body/改写方法/外泄签名 body）
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .connect_timeout(Duration::from_secs(3)) // 终审 I4：黑洞端点不占满超时
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("http client 构建不应失败"),
            retry_backoff_secs: vec![60, 300, 1800],
        }
    }

    /// 投递测试/单实例快速轮询入口
    pub fn with_backoff(mut self, secs: Vec<i64>) -> Self {
        self.retry_backoff_secs = secs;
        self
    }

    /// 单轮投递：取一批 pending（到期的）事件，逐事件扇出到命中订阅。
    /// 返回本轮投递成功数。
    pub async fn tick(&self) -> core_model::Result<u32> {
        let mut delivered = 0u32;
        let events: Vec<(i64, String, serde_json::Value)> = sqlx::query_as(
            "SELECT id, topic, payload FROM event_outbox \
             WHERE status = 0 AND (next_retry_at IS NULL OR next_retry_at <= now()) \
             ORDER BY id LIMIT 50",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| core_model::DomainError::Db(e.to_string()))?;

        let repo = WebhookRepo::new(self.pool.clone());
        for (id, topic, payload) in events {
            let subs = repo.for_topic(&topic).await?;
            if subs.is_empty() {
                // 无订阅者：直接标 dispatched（事件已被"消费"——持久留档在 outbox）
                sqlx::query("UPDATE event_outbox SET status = 1 WHERE id = $1 AND status = 0")
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                continue;
            }
            let body = serde_json::to_string(&payload)
                .map_err(|e| core_model::DomainError::Internal(e.to_string()))?;

            let mut all_ok = true;
            for sub in &subs {
                let mut mac = HmacSha256::new_from_slice(sub.secret.as_bytes())
                    .map_err(|e| core_model::DomainError::Internal(e.to_string()))?;
                mac.update(body.as_bytes());
                let sig = hex::encode(mac.finalize().into_bytes());

                let ok = match self
                    .http
                    .post(&sub.url)
                    .header("content-type", "application/json")
                    .header("x-rustplm-topic", &topic)
                    .header("x-rustplm-signature", &sig)
                    .header("x-rustplm-event-id", id.to_string())
                    .body(body.clone())
                    .send()
                    .await
                {
                    Ok(r) if r.status().is_success() => true,
                    Ok(r) => {
                        warn!(topic = %topic, status = %r.status(), "webhook 投递被拒");
                        false
                    }
                    Err(e) => {
                        warn!(topic = %topic, error = %e, "webhook 投递失败");
                        false
                    }
                };
                if !ok {
                    all_ok = false;
                }
            }

            if all_ok {
                // CAS：0→1（并发投递器安全）
                sqlx::query("UPDATE event_outbox SET status = 1 WHERE id = $1 AND status = 0")
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                delivered += 1;
            } else {
                // 终审 C2：按 attempts 索引退避档；终审 I1：死信转移带 status=0 守卫
                let attempts: i32 = sqlx::query_scalar(
                    "UPDATE event_outbox SET attempts = attempts + 1 WHERE id = $1 AND status = 0 RETURNING attempts",
                )
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| core_model::DomainError::Db(e.to_string()))?
                .ok_or_else(|| core_model::DomainError::Internal("并发下事件行已被处理".into()))?;
                let secs = self.backoff_secs_for(attempts);
                sqlx::query(
                    "UPDATE event_outbox SET next_retry_at = now() + make_interval(secs => $1) WHERE id = $2 AND status = 0",
                )
                .bind(secs)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| core_model::DomainError::Db(e.to_string()))?;
                if attempts >= MAX_ATTEMPTS {
                    sqlx::query("UPDATE event_outbox SET status = 2 WHERE id = $1 AND status = 0")
                        .bind(id)
                        .execute(&self.pool)
                        .await?;
                    warn!(topic = %topic, id = id, "webhook 重试超限：事件进入死信");
                }
            }
        }
        Ok(delivered)
    }

    fn backoff_secs_for(&self, attempts: i32) -> i64 {
        let idx = (attempts - 1).clamp(0, self.retry_backoff_secs.len() as i32 - 1) as usize;
        self.retry_backoff_secs[idx]
    }

    /// 后台循环（plm-server 装配）
    pub fn spawn(self: &Arc<Self>, interval_ms: u64) -> tokio::task::JoinHandle<()> {
        let me = std::sync::Arc::clone(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                if let Err(e) = me.tick().await {
                    tracing::warn!(error = %e, "事件投递轮失败");
                }
            }
        })
    }
}

use std::sync::Arc;

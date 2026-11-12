//! 订阅仓储。

use core_model::Result;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WebhookSub {
    pub id: Uuid,
    pub url: String,
    pub topics: Vec<String>,
    #[serde(skip_serializing)]
    pub secret: String,
    pub enabled: bool,
}

pub struct WebhookRepo {
    pool: PgPool,
}

impl WebhookRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, url: &str, topics: &[String], secret: &str) -> Result<Uuid> {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO webhook_subscription(url, topics, secret) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(url)
        .bind(topics)
        .bind(secret)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn list(&self) -> Result<Vec<WebhookSub>> {
        Ok(sqlx::query_as(
            "SELECT id, url, topics, secret, enabled FROM webhook_subscription WHERE enabled ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn delete(&self, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM webhook_subscription WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 启用的订阅中，命中 topic 的全部行
    pub async fn for_topic(&self, topic: &str) -> Result<Vec<WebhookSub>> {
        Ok(sqlx::query_as(
            "SELECT id, url, topics, secret, enabled FROM webhook_subscription \
             WHERE enabled AND $1 = ANY(topics)",
        )
        .bind(topic)
        .fetch_all(&self.pool)
        .await?)
    }
}

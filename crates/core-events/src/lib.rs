//! core-events —— 事件外投（ADR-0007）：Outbox → Webhook（HMAC 签名/退避/死信）。

pub mod dispatch;
pub mod repo;

pub use dispatch::EventDispatcher;
pub use repo::{WebhookRepo, WebhookSub};

/// 多 crate 迁移链
pub fn migrator() -> sqlx::migrate::Migrator {
    let mut m = sqlx::migrate!("./migrations");
    m.set_ignore_missing(true);
    m
}

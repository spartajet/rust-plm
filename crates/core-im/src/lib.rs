//! core-im —— IM 卡片审批集成（增量10）：评审事件→机器人卡片（钉钉加签/企微/飞书），
//! 统一 HMAC 回调契约（`/api/v1/im/actions`，装配在 api-gateway）。
//!
//! 双消费者解耦：webhook dispatcher 认领 event_outbox 置位后，CardPusher 仍按
//! im_card_log(event_id UNIQUE) 独立订阅同源事件。

pub mod pusher;
pub mod render;
pub mod sender;

pub use pusher::CardPusher;
pub use render::{dingtalk_card, dingtalk_sign, dingtalk_signed_url, verify_hmac_hex, CardInput};
pub use sender::{sender_for, CardSender, DingTalkSender, FeishuSender, NoopSender, WeComSender};

/// 多 crate 迁移链（ignore_missing，同前例）
pub fn migrator() -> sqlx::migrate::Migrator {
    let mut m = sqlx::migrate!("./migrations");
    m.set_ignore_missing(true);
    m
}

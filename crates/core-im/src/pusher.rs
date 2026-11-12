//! CardPusher：event_outbox → im_card_log 订阅 → 发卡（退避/死信镜像 webhook dispatcher 语义）。
//! 双消费者解耦（增量10 Review Focus #5）：不依赖 outbox status（dispatcher 会认领置位），
//  以 im_card_log.event_id 唯一订阅。

use crate::render::CardInput;
use crate::sender::CardSender;
use core_model::Result;
use sqlx::PgPool;
use std::sync::Arc;

const MAX_ATTEMPTS: i32 = 4; // 首发 + 3 次重试（与 dispatcher 同语义）
const RETRY_BACKOFF_SECS: [i64; 3] = [60, 300, 1800];

pub struct CardPusher {
    pool: PgPool,
    sender: Arc<dyn CardSender>,
    /// 详情页链接模板（{obj_no} 占位；空则不带链接）
    doc_url_template: String,
}

impl CardPusher {
    pub fn new(pool: PgPool, sender: Arc<dyn CardSender>, doc_url_template: String) -> Self {
        Self {
            pool,
            sender,
            doc_url_template,
        }
    }

    pub fn spawn(self: Arc<Self>, interval_ms: u64) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
            // tick 慢时按实际完成时间推进（不突发补帧——镜像 dispatcher spawn 语义）
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                if let Err(e) = self.tick().await {
                    tracing::warn!(error = %e, "IM 卡片推送轮询失败");
                }
            }
        })
    }

    /// 单轮：订阅新事件 → 认领到期 pending → 发卡 → 更新状态
    pub async fn tick(&self) -> Result<()> {
        // 1) 订阅：反连接（NOT EXISTS）——终审 I1：bigserial id 于 INSERT 分配、提交时可见，
        //    并发事务提交序倒挂下「id > max 水位线」会永久跳过后提交的低 id 行（审批卡静默丢失）。
        //    幂等仍以 im_card_log.event_id UNIQUE 锚定（ON CONFLICT 兜底并发 tick 重叠）。
        sqlx::query(
            "INSERT INTO im_card_log(event_id, topic, obj_no, major, minor, actor) \
             SELECT o.id, o.topic, o.payload->>'obj_no', \
                    COALESCE((o.payload->>'major')::int, 1), \
                    COALESCE((o.payload->>'minor')::int, 0), \
                    o.payload->>'actor' \
             FROM event_outbox o \
             WHERE o.topic IN ('doc.submitted', 'doc.checked') \
               AND NOT EXISTS (SELECT 1 FROM im_card_log l WHERE l.event_id = o.id) \
             ON CONFLICT (event_id) DO NOTHING",
        )
        .execute(&self.pool)
        .await?;

        // 2) 认领到期 pending（单实例推送：顺序处理，FOR UPDATE 免——ledger 记账）
        #[derive(sqlx::FromRow)]
        struct Row {
            id: i64,
            topic: String,
            obj_no: String,
            major: i32,
            minor: i32,
            actor: Option<String>,
            attempts: i32,
        }
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, topic, obj_no, major, minor, actor, attempts FROM im_card_log \
             WHERE status = 0 AND (next_retry_at IS NULL OR next_retry_at <= now()) \
             ORDER BY id LIMIT 20",
        )
        .fetch_all(&self.pool)
        .await?;

        for r in rows {
            let state = if r.topic == "doc.checked" {
                "checked"
            } else {
                "review"
            };
            let input = CardInput {
                obj_no: &r.obj_no,
                major: r.major,
                minor: r.minor,
                state,
                actor: r.actor.as_deref(),
                url: if self.doc_url_template.is_empty() {
                    None
                } else {
                    Some(self.doc_url_template.replace("{obj_no}", &r.obj_no))
                },
            };
            match self.sender.send_card(&input).await {
                Ok(()) => {
                    sqlx::query("UPDATE im_card_log SET status = 1, sent_at = now() WHERE id = $1")
                        .bind(r.id)
                        .execute(&self.pool)
                        .await?;
                }
                Err(e) => {
                    let attempts = r.attempts + 1;
                    if attempts >= MAX_ATTEMPTS {
                        sqlx::query(
                            "UPDATE im_card_log SET status = 2, attempts = $2, error = $3 WHERE id = $1",
                        )
                        .bind(r.id)
                        .bind(attempts)
                        .bind(e.to_string())
                        .execute(&self.pool)
                        .await?;
                    } else {
                        let backoff = RETRY_BACKOFF_SECS
                            [(attempts - 1).clamp(0, RETRY_BACKOFF_SECS.len() as i32 - 1) as usize];
                        sqlx::query(
                            "UPDATE im_card_log SET attempts = $2, next_retry_at = now() + make_interval(secs => $3), \
                             error = $4 WHERE id = $1",
                        )
                        .bind(r.id)
                        .bind(attempts)
                        .bind(backoff)
                        .bind(e.to_string())
                        .execute(&self.pool)
                        .await?;
                    }
                }
            }
        }
        Ok(())
    }
}

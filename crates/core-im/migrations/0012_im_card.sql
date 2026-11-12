-- 0012_im_card.sql — 增量10 IM 卡片推送：订阅日志（不依赖 event_outbox status——webhook dispatcher
-- 会认领置位，双消费者必须解耦；以 event_id 唯一订阅，镜像 dispatcher 退避/死信语义）。

CREATE TABLE IF NOT EXISTS im_card_log (
    id            bigserial PRIMARY KEY,
    event_id      bigint NOT NULL UNIQUE,
    topic         text NOT NULL,
    obj_no        text NOT NULL,
    major         integer NOT NULL DEFAULT 1,
    minor         integer NOT NULL DEFAULT 0,
    actor         text,
    status        smallint NOT NULL DEFAULT 0,      -- 0 pending / 1 sent / 2 dead
    attempts      integer NOT NULL DEFAULT 0,
    next_retry_at timestamptz,
    error         text,
    created_at    timestamptz NOT NULL DEFAULT now(),
    sent_at       timestamptz
);
CREATE INDEX IF NOT EXISTS im_card_log_pending_idx ON im_card_log(status, id);

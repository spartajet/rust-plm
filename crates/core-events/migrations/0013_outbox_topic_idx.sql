-- 0013_outbox_topic_idx.sql — 增量13：IM/订阅反连接扫描走索引 + 死信错误信息落库。

-- IM CardPusher/webhook dispatcher 高频按 topic 过滤扫描（终审复审备忘：部分索引）
CREATE INDEX IF NOT EXISTS event_outbox_topic_idx
    ON event_outbox(topic, id)
    WHERE topic IN ('doc.released', 'doc.revised', 'doc.submitted', 'doc.checked');

-- 死信最后错误（排查需要；增量13 债清偿——此前死信无错误痕迹）
ALTER TABLE event_outbox ADD COLUMN IF NOT EXISTS error text;

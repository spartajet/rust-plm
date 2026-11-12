-- 0008_webhooks.sql — Webhook 订阅与死信（ADR-0007 / INT-02）

CREATE TABLE IF NOT EXISTS webhook_subscription (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    url        text NOT NULL,
    topics     text[] NOT NULL,
    secret     text NOT NULL,
    enabled    boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now()
);

-- Outbox 行状态扩展：pending(0)/dispatched(1)/dead(2)
ALTER TABLE event_outbox ADD COLUMN IF NOT EXISTS attempts integer NOT NULL DEFAULT 0;
ALTER TABLE event_outbox ADD COLUMN IF NOT EXISTS next_retry_at timestamptz;
CREATE INDEX IF NOT EXISTS event_outbox_retry_idx ON event_outbox(status, next_retry_at);

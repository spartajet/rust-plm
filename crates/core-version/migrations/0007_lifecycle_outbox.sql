-- 0007_lifecycle_outbox.sql — 生命周期快照 + 事件 Outbox（增量4 外投）

-- 发布/评审时点快照（不可变：无任何 UPDATE/DELETE 路径）
CREATE TABLE IF NOT EXISTS cm_obj_snapshot (
    id         bigserial PRIMARY KEY,
    obj_no     text NOT NULL,
    type_code  text NOT NULL,
    major_ver  integer NOT NULL,
    minor_ver  integer NOT NULL,
    state      text NOT NULL,
    attrs      jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    created_by uuid
);
CREATE INDEX IF NOT EXISTS cm_obj_snapshot_no_idx ON cm_obj_snapshot(type_code, obj_no);
-- 终审 C2 兜底：同版本仅一份 released 快照（并发 approve 竞态输家 DB 层拒绝）
CREATE UNIQUE INDEX IF NOT EXISTS cm_obj_snapshot_ver_uk
    ON cm_obj_snapshot(type_code, obj_no, major_ver, minor_ver);

-- 事务性事件 Outbox（ADR-0007：业务同事务写入；外投为增量4）
CREATE TABLE IF NOT EXISTS event_outbox (
    id         bigserial PRIMARY KEY,
    topic      text NOT NULL,
    payload    jsonb NOT NULL,
    status     smallint NOT NULL DEFAULT 0,      -- 0 pending / 1 dispatched
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS event_outbox_pending_idx ON event_outbox(status, id);

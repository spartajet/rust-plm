-- 0018_doc_lock.sql — 增量26 DOC-03 检入/检出：独占检出锁（一人检出全站锁定）。

-- 独占锁：object_id 主键即独占；到期惰性回收（查询时过期行视为无锁并清理）
-- 注意：checkout_hours 参数种子在 core-sec 0019（sys_param 表归 core-sec 域——
-- 本迁移执行序早于 core-sec，跨域 INSERT 会 42P01）
CREATE TABLE IF NOT EXISTS doc_lock (
    object_id    uuid PRIMARY KEY REFERENCES cm_object(id),
    by_user      uuid NOT NULL,
    by_name      text NOT NULL,
    locked_at    timestamptz NOT NULL DEFAULT now(),
    locked_until timestamptz NOT NULL
);
CREATE INDEX IF NOT EXISTS doc_lock_until_idx ON doc_lock (locked_until);

-- 0016_audit_log.sql — 增量18 SYS-01：审计日志（append-only）。
-- 应用层零 UPDATE/DELETE 路径；多角色部署（应用/DBA 分离）时对表 REVOKE UPDATE,DELETE（同 migrations.md 裁定）。

CREATE TABLE IF NOT EXISTS audit_log (
    id       bigserial PRIMARY KEY,
    at       timestamptz NOT NULL DEFAULT now(),
    username text NOT NULL,
    action   text NOT NULL,
    object   text,
    detail   jsonb,
    trace_id text,
    ip       text
);
CREATE INDEX IF NOT EXISTS audit_log_user_idx ON audit_log(username, at);
CREATE INDEX IF NOT EXISTS audit_log_action_idx ON audit_log(action, at);
CREATE INDEX IF NOT EXISTS audit_log_object_idx ON audit_log(object);

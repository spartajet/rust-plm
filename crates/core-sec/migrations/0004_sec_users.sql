-- 0004_sec_users.sql — 安全域用户表（迁移链续接：版本 0004 全局唯一）

CREATE TABLE IF NOT EXISTS sec_user (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    username      text NOT NULL,
    password_hash text NOT NULL,                  -- PHC 串（argon2id）
    display_name  text,
    enabled       boolean NOT NULL DEFAULT true,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    last_login_at timestamptz
);
CREATE UNIQUE INDEX IF NOT EXISTS sec_user_username_uk ON sec_user (lower(username));

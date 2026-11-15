-- 0017_lockout_params.sql — 增量23：防爆破锁定列（sec_user）+ SYS-04 最小参数中心（sys_param）。

-- 登录失败计数与锁定（防爆破：连续失败 ≥ lock_threshold → locked_until = now()+lock_minutes）
ALTER TABLE sec_user
    ADD COLUMN IF NOT EXISTS failed_attempts integer NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS locked_until timestamptz;

-- SYS-04 最小参数中心：系统级参数集中管理（修改走 PUT /sec/params，audit 留痕）
CREATE TABLE IF NOT EXISTS sys_param (
    key        text PRIMARY KEY,
    value      text NOT NULL,
    updated_by uuid,
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- 密码策略与锁定参数种子（幂等；改值走端点——留痕）
INSERT INTO sys_param(key, value) VALUES
    ('password_min_length', '8'),
    ('lock_threshold', '5'),
    ('lock_minutes', '15')
ON CONFLICT (key) DO NOTHING;

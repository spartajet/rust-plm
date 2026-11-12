-- 0005_sec_rbac.sql — 授权域：角色/权限/用户角色 + 用户密级上限

CREATE TABLE IF NOT EXISTS sec_role (
    code text PRIMARY KEY,
    name text NOT NULL
);

CREATE TABLE IF NOT EXISTS sec_role_perm (
    role_code text NOT NULL REFERENCES sec_role(code),
    obj      text NOT NULL,
    act      text NOT NULL,
    PRIMARY KEY (role_code, obj, act)
);

CREATE TABLE IF NOT EXISTS sec_user_role (
    username  text NOT NULL,
    role_code text NOT NULL REFERENCES sec_role(code),
    PRIMARY KEY (username, role_code)
);

INSERT INTO sec_role(code, name) VALUES
    ('admin',  '管理员'),
    ('editor', '工程师（可编辑）'),
    ('viewer', '查阅（只读）')
ON CONFLICT DO NOTHING;

INSERT INTO sec_role_perm(role_code, obj, act) VALUES
    ('admin',  'docs', 'read'),
    ('admin',  'docs', 'write'),
    ('admin',  'sec',  'manage'),
    ('editor', 'docs', 'read'),
    ('editor', 'docs', 'write'),
    ('viewer', 'docs', 'read')
ON CONFLICT DO NOTHING;

ALTER TABLE sec_user ADD COLUMN IF NOT EXISTS max_secret_level smallint NOT NULL DEFAULT 3;

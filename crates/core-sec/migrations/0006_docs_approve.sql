-- 0006: docs:approve 权限（生命周期审批，admin 专属；editor 提交、viewer 只读）
INSERT INTO sec_role_perm(role_code, obj, act) VALUES ('admin', 'docs', 'approve')
ON CONFLICT DO NOTHING;

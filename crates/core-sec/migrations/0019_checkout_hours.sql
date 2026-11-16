-- 0019_checkout_hours.sql — 增量26：检出时长参数种子（DOC-03；sys_param 归本域）。

INSERT INTO sys_param(key, value) VALUES ('checkout_hours', '24')
ON CONFLICT (key) DO NOTHING;

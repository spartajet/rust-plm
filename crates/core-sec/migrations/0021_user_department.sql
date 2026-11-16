-- 0021_user_department.sql — 增量28：员工部门（demo 数据包/组织展示；系统无部门实体——
-- ORG-01 部门表后置，demo 用 department text 承载，UserAdmin 列+筛选）。

ALTER TABLE sec_user ADD COLUMN IF NOT EXISTS department text NOT NULL DEFAULT '';
CREATE INDEX IF NOT EXISTS sec_user_department_idx ON sec_user (department);

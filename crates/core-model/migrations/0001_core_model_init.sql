-- 0001_core_model_init.sql — core-model 元数据域初始结构
-- 依据：《详细设计-core-model》、ADR-0002（JSONB + 提升列 + 在线 DDL）

-- ============ 元数据域 ============

-- 对象类型定义（版本化：任何变更 bump cm_meta_version，TypeRegistry 据此热刷新）
CREATE TABLE IF NOT EXISTS cm_type (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    code           text NOT NULL,
    name           text NOT NULL,
    base_type      text NOT NULL,                    -- part/document/change/project/bom/generic
    table_name     text NOT NULL DEFAULT 'cm_object',-- 预留分表，当前统一大表
    numbering_rule text,                             -- 编码规则代码（COD 域）
    title_attr     text,                             -- 列表主显示属性
    config         jsonb NOT NULL DEFAULT '{}',
    config_version integer NOT NULL DEFAULT 1,
    is_system      boolean NOT NULL DEFAULT false,
    enabled        boolean NOT NULL DEFAULT true,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    created_by     uuid,
    updated_by     uuid,
    CONSTRAINT cm_type_code_uk UNIQUE (code)
);

-- 属性定义
CREATE TABLE IF NOT EXISTS cm_attr (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    type_id      uuid NOT NULL REFERENCES cm_type(id),
    code         text NOT NULL,
    name         text NOT NULL,
    data_type    text NOT NULL,                      -- text/int/decimal/date/bool/enum/ref/unit/json
    required     boolean NOT NULL DEFAULT false,
    unique_flag  boolean NOT NULL DEFAULT false,
    default_val  jsonb,
    ref_type     text,                               -- ref 型属性指向的类型 code
    formula      text,                               -- 派生公式（rhai，core-hooks 执行）
    promoted     boolean NOT NULL DEFAULT false,     -- 是否已提升为物理列
    promoted_col text,                               -- 提升后的物理列名（cm_object.p_<code>）
    config       jsonb NOT NULL DEFAULT '{}',        -- 校验/显隐/宽度等
    sort_no      integer NOT NULL DEFAULT 0,
    enabled      boolean NOT NULL DEFAULT true,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    created_by   uuid,
    updated_by   uuid,
    CONSTRAINT cm_attr_type_code_uk UNIQUE (type_id, code)
);
CREATE INDEX IF NOT EXISTS cm_attr_type_idx ON cm_attr(type_id);

-- 枚举字典
CREATE TABLE IF NOT EXISTS cm_dict (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    code       text NOT NULL,
    name       text NOT NULL,
    items      jsonb NOT NULL,                       -- [{"v":"..","label":".."}]
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT cm_dict_code_uk UNIQUE (code)
);

-- 关系定义（装配关系由 core-bom 的 bom_edge 承载，不在此表）
CREATE TABLE IF NOT EXISTS cm_rel_def (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    code         text NOT NULL,
    name         text NOT NULL,
    src_type     text NOT NULL,
    dst_type     text NOT NULL,
    cardinality  text NOT NULL DEFAULT 'n:n',        -- 1:1 / 1:n / n:n
    config       jsonb NOT NULL DEFAULT '{}',
    enabled      boolean NOT NULL DEFAULT true,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT cm_rel_code_uk UNIQUE (code)
);

-- ============ 对象域 ============

-- 业务对象统一表：系统属性物理列 + 自定义属性 attrs JSONB + 动态提升列 p_<code>
CREATE TABLE IF NOT EXISTS cm_object (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    type_id      uuid NOT NULL REFERENCES cm_type(id),
    obj_no       text NOT NULL,
    major_ver    integer NOT NULL DEFAULT 1,
    minor_ver    integer NOT NULL DEFAULT 0,
    iteration    integer NOT NULL DEFAULT 0,         -- 工作版次（working 态累加）
    state        text NOT NULL DEFAULT 'working',    -- 状态机由 core-version 驱动
    secret_level smallint NOT NULL DEFAULT 0,
    owner        uuid,
    attrs        jsonb NOT NULL DEFAULT '{}',
    attrs_rev    integer NOT NULL DEFAULT 1,         -- 乐观锁
    deleted_at   timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    created_by   uuid,
    updated_at   timestamptz NOT NULL DEFAULT now(),
    updated_by   uuid
);
-- 版本唯一（软删排除）
CREATE UNIQUE INDEX IF NOT EXISTS cm_object_ver_uk
    ON cm_object(type_id, obj_no, major_ver, minor_ver) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS cm_object_gin_attrs ON cm_object USING gin(attrs);
CREATE INDEX IF NOT EXISTS cm_object_state_idx
    ON cm_object(type_id, state) WHERE deleted_at IS NULL;

-- 关系实例
CREATE TABLE IF NOT EXISTS cm_rel_inst (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    rel_code   text NOT NULL REFERENCES cm_rel_def(code),
    from_id    uuid NOT NULL REFERENCES cm_object(id),
    to_id      uuid NOT NULL REFERENCES cm_object(id),
    attrs      jsonb NOT NULL DEFAULT '{}',
    sort_no    integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now(),
    created_by uuid,
    CONSTRAINT cm_rel_inst_uk UNIQUE (rel_code, from_id, to_id)
);
CREATE INDEX IF NOT EXISTS cm_rel_inst_to_idx ON cm_rel_inst(rel_code, to_id);

-- ============ 配置热生效基础设施 ============

-- 全局元数据版本（单行表：任何元数据变更事务内 version+1，触发 NOTIFY cm_meta_changed）
CREATE TABLE IF NOT EXISTS cm_meta_version (
    id         integer PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    version    bigint NOT NULL DEFAULT 1,
    updated_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO cm_meta_version(id, version) VALUES (1, 1) ON CONFLICT DO NOTHING;

-- 在线提升列变更日志（Promoter 状态机落库，可审计可回滚）
CREATE TABLE IF NOT EXISTS cm_ddl_log (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    type_id     uuid NOT NULL,
    attr_id     uuid NOT NULL,
    action      text NOT NULL CHECK (action IN ('promote', 'demote')),
    col_name    text NOT NULL,
    status      text NOT NULL DEFAULT 'pending',     -- pending/adding/backfilling/indexing/done/failed/rolled_back
    batches     integer NOT NULL DEFAULT 0,
    error       text,
    rolled_back boolean NOT NULL DEFAULT false,
    started_at  timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz
);

-- 元数据变更触发器：cm_type/cm_attr/cm_dict/cm_rel_def 的写操作 → bump 版本 + NOTIFY
CREATE OR REPLACE FUNCTION cm_bump_meta() RETURNS trigger AS $$
BEGIN
    UPDATE cm_meta_version SET version = version + 1, updated_at = now() WHERE id = 1;
    PERFORM pg_notify('cm_meta_changed', (SELECT version::text FROM cm_meta_version WHERE id = 1));
    RETURN NULL;
END; $$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS cm_type_meta_trg ON cm_type;
CREATE TRIGGER cm_type_meta_trg AFTER INSERT OR UPDATE OR DELETE ON cm_type
    FOR EACH STATEMENT EXECUTE FUNCTION cm_bump_meta();
DROP TRIGGER IF EXISTS cm_attr_meta_trg ON cm_attr;
CREATE TRIGGER cm_attr_meta_trg AFTER INSERT OR UPDATE OR DELETE ON cm_attr
    FOR EACH STATEMENT EXECUTE FUNCTION cm_bump_meta();
DROP TRIGGER IF EXISTS cm_dict_meta_trg ON cm_dict;
CREATE TRIGGER cm_dict_meta_trg AFTER INSERT OR UPDATE OR DELETE ON cm_dict
    FOR EACH STATEMENT EXECUTE FUNCTION cm_bump_meta();
DROP TRIGGER IF EXISTS cm_rel_def_meta_trg ON cm_rel_def;
CREATE TRIGGER cm_rel_def_meta_trg AFTER INSERT OR UPDATE OR DELETE ON cm_rel_def
    FOR EACH STATEMENT EXECUTE FUNCTION cm_bump_meta();

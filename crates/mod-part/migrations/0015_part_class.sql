-- 0015_part_class.sql — 增量17 PART 物料主数据：分类树（PART-02）+ part 类型种子（PART-01 纵切）。

-- 分类树：多级（parent_id 自引用）；分类挂编码规则（COD-03 分类维度）
CREATE TABLE IF NOT EXISTS cm_class (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    parent_id  uuid REFERENCES cm_class(id),
    code       text NOT NULL,
    name       text NOT NULL,
    cod_rule   jsonb,
    enabled    boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS cm_class_code_uk ON cm_class (code);

-- part 对象类型（PART-01 纵切：单类型+分类树；类型模板差异化后置）
INSERT INTO cm_type(code, name, base_type, config)
VALUES ('part', '物料', 'part', '{"title_attr":"name"}'::jsonb)
ON CONFLICT (code) DO NOTHING;

INSERT INTO cm_attr(type_id, code, name, data_type, required, sort_no)
SELECT t.id, a.code, a.name, a.dtype, a.req, a.sort_no
FROM cm_type t
JOIN (VALUES
    ('name',    '名称', 'text',   true,  1),
    ('material','材料', 'text',   false, 2),
    ('spec',    '规格', 'text',   false, 3),
    ('model',   '型号', 'text',   false, 4)
) AS a(code, name, dtype, req, sort_no) ON true
WHERE t.code = 'part'
  AND NOT EXISTS (SELECT 1 FROM cm_attr e WHERE e.type_id = t.id AND e.code = a.code);

-- 示例分类种子（二层；cod_rule 各异——COD-03 分类独立流水）
INSERT INTO cm_class(code, name, parent_id, cod_rule)
SELECT * FROM (VALUES
    ('CLS-ZZ', '自制件', NULL::uuid, '{"cod_rule":{"prefix":"WL-ZZ-","date_fmt":"none","seq_digits":5}}'::jsonb),
    ('CLS-WG', '外购件', NULL::uuid, '{"cod_rule":{"prefix":"WL-WG-","date_fmt":"none","seq_digits":5}}'::jsonb)
) AS v(code, name, parent_id, cod_rule)
WHERE NOT EXISTS (SELECT 1 FROM cm_class e WHERE e.code = v.code);

INSERT INTO cm_class(code, name, parent_id, cod_rule)
SELECT v.code, v.name, p.id, v.cod_rule
FROM (VALUES
    ('CLS-ZZ-JGJ', '紧固件', 'CLS-ZZ', '{"cod_rule":{"prefix":"WL-ZZ-JGJ-","date_fmt":"none","seq_digits":5}}'::jsonb),
    ('CLS-ZZ-BJJ', '钣金件', 'CLS-ZZ', '{"cod_rule":{"prefix":"WL-ZZ-BJJ-","date_fmt":"none","seq_digits":5}}'::jsonb)
) AS v(code, name, parent_code, cod_rule)
JOIN cm_class p ON p.code = v.parent_code
WHERE NOT EXISTS (SELECT 1 FROM cm_class e WHERE e.code = v.code);

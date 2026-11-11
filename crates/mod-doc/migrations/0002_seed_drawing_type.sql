-- 0002_seed_drawing_type.sql — 骨架期种子：drawing 类型与属性（AGL 配置中心的后置替代）

INSERT INTO cm_type(code, name, base_type, numbering_rule, title_attr)
VALUES ('drawing', '图纸', 'document', NULL, 'name')
ON CONFLICT (code) DO NOTHING;

INSERT INTO cm_attr(type_id, code, name, data_type, required, sort_no)
SELECT t.id, v.code, v.name, v.data_type, v.required, v.sort_no
FROM cm_type t
JOIN (VALUES
    ('name',           '图样名称', 'text', true,  1),
    ('material',       '材料',     'text', false, 2),
    ('scale',          '比例',     'text', false, 3),
    ('spec',           '规格',     'text', false, 4),
    ('classification', '分类',     'text', false, 5),
    ('designer',       '设计',     'text', false, 6),
    ('remark',         '备注',     'text', false, 7)
) AS v(code, name, data_type, required, sort_no) ON TRUE
WHERE t.code = 'drawing'
ON CONFLICT (type_id, code) DO NOTHING;

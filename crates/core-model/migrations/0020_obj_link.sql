-- 0020_obj_link.sql — 增量27 DOC-12 对象关联：泛化关联表（kind 区分域——doc_part 起步；
-- 未来 BOM/变更/项目关联同表扩展）。方向语义：from=文档、to=物料（doc_part）。

CREATE TABLE IF NOT EXISTS obj_link (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    from_id    uuid NOT NULL REFERENCES cm_object(id),
    to_id      uuid NOT NULL REFERENCES cm_object(id),
    kind       text NOT NULL,
    created_by uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (from_id, to_id, kind)
);
CREATE INDEX IF NOT EXISTS obj_link_from_idx ON obj_link (from_id, kind);
CREATE INDEX IF NOT EXISTS obj_link_to_idx ON obj_link (to_id, kind);

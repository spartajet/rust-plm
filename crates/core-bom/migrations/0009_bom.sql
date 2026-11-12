-- 0009_bom.sql — BOM 边表 + 闭包表（ADR-0003 / V-2 已验证）
-- 边挂 cm_object（版本=对象版本；独立 BOM 对象+版本链为后续增量）

CREATE TABLE IF NOT EXISTS bom_edge (
    parent   uuid NOT NULL REFERENCES cm_object(id),
    line_no  integer NOT NULL,
    child    uuid NOT NULL REFERENCES cm_object(id),
    qty      bigint NOT NULL CHECK (qty > 0),
    -- line_no ≥ 1 由应用校验（终审 M10）；DB CHECK 随下个迁移补
    PRIMARY KEY (parent, line_no)
);
CREATE INDEX IF NOT EXISTS bom_edge_child_idx ON bom_edge(child);

CREATE TABLE IF NOT EXISTS bom_path (
    ancestor   uuid NOT NULL,
    descendant uuid NOT NULL,
    depth      integer NOT NULL,
    qty_path   bigint NOT NULL,
    PRIMARY KEY (ancestor, descendant, depth)
);
CREATE INDEX IF NOT EXISTS bom_path_desc_idx ON bom_path(descendant);
-- 终审 C2 兜底（生命周期同模式）：并发竞态输家 DB 层拒绝——闭包行无唯一业务键，
-- 以 (ancestor,descendant,depth) 主键天然去重（ON CONFLICT 累加）

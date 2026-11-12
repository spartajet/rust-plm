-- 0011_bom_version_chain.sql — 增量9 BOM 版本链：发布冻结快照 + 发布记录。
-- live bom_edge 语义 = 最新 major 工作区；历史形态在本表（approve 同事务经 ReleaseHook 落入）。

-- 发布记录（as-of 守卫依据；无 BOM 边的对象发布也落行）
CREATE TABLE IF NOT EXISTS bom_release (
    parent      uuid NOT NULL REFERENCES cm_object(id),
    major       integer NOT NULL,
    released_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (parent, major)
);

-- 发布时点边集快照（不可变：无 UPDATE/DELETE 路径）
CREATE TABLE IF NOT EXISTS bom_edge_snapshot (
    parent  uuid NOT NULL REFERENCES cm_object(id),
    major   integer NOT NULL,
    line_no integer NOT NULL,
    child   uuid NOT NULL REFERENCES cm_object(id),
    qty     bigint NOT NULL CHECK (qty > 0),
    PRIMARY KEY (parent, major, line_no)
);
CREATE INDEX IF NOT EXISTS bom_edge_snapshot_child_idx ON bom_edge_snapshot(child);

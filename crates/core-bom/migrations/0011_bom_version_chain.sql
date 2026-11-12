-- 0011_bom_version_chain.sql — 增量9 BOM 版本链：发布冻结快照 + 发布记录。
-- live bom_edge 语义 = 最新 major 工作区；历史形态在本表（approve 同事务经 ReleaseHook 落入）。
-- 终审 C1：快照行按「发布根」键控 (release_root, major, parent, line_no)——
-- 若按 (parent, major) 键控，子装配件独立发布与祖先发布共享键空间，祖先冻结被 DO NOTHING
-- 静默吞掉 → as-of 静默错值（真库实证 9 倍数量偏差/嵌合体形态）。

-- 发布记录（as-of 守卫依据；无 BOM 边的对象发布也落行）
CREATE TABLE IF NOT EXISTS bom_release (
    parent      uuid NOT NULL REFERENCES cm_object(id),
    major       integer NOT NULL,
    released_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (parent, major)
);

-- 发布时点子树边集快照（不可变：无 UPDATE/DELETE 路径；键=发布根事件）
CREATE TABLE IF NOT EXISTS bom_edge_snapshot (
    release_root uuid NOT NULL REFERENCES cm_object(id),
    major        integer NOT NULL,
    parent       uuid NOT NULL REFERENCES cm_object(id),
    line_no      integer NOT NULL,
    child        uuid NOT NULL REFERENCES cm_object(id),
    qty          bigint NOT NULL CHECK (qty > 0),
    PRIMARY KEY (release_root, major, parent, line_no)
);
CREATE INDEX IF NOT EXISTS bom_edge_snapshot_child_idx ON bom_edge_snapshot(child);

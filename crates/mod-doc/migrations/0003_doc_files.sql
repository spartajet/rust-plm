-- 0003_doc_files.sql — 文件两表（内容寻址元数据 + 对象引用）
-- builder-check 增补：file_ref.name 保存原始文件名（下载 Content-Disposition / CAD 检入回显）

CREATE TABLE IF NOT EXISTS file_object (
    hash         text PRIMARY KEY,
    size         bigint NOT NULL,
    mime         text,
    virus_status text NOT NULL DEFAULT 'skip',   -- 骨架期默认 skip；ClamAV 接入后 pending/clean/infected
    created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS file_ref (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    object_id  uuid NOT NULL REFERENCES cm_object(id),
    role       text NOT NULL DEFAULT 'original',
    hash       text NOT NULL REFERENCES file_object(hash),
    name       text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT file_ref_role_uk UNIQUE (object_id, role)
);
CREATE INDEX IF NOT EXISTS file_ref_hash_idx ON file_ref(hash);

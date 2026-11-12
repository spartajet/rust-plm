-- 0010_snapshot_file_hash.sql — 增量8 Revise：发布快照记录发布件内容哈希。
-- 数据先存（approve 时点绑定不可重建）；旧版文件下载端点属后续"版本链"增量。

ALTER TABLE cm_obj_snapshot ADD COLUMN IF NOT EXISTS file_hash text;

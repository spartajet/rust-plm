//! 跨层数据传输对象（serde DTO；不泄漏数据库行）。

use core_model::BizObject;
use serde::Serialize;

/// 附件信息（骨架期：每对象至多 1 个 `role='original'`）
#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub name: String,
    pub hash: String,
    pub size: i64,
    pub mime: Option<String>,
}

/// 图纸对象 + 附件聚合视图
#[derive(Debug, Clone, Serialize)]
pub struct DocDetail {
    pub object: BizObject,
    pub file: Option<FileInfo>,
}

/// 版本历史快照行（增量8：released 时点不可变链——升版出口的数据面）
#[derive(Debug, Clone, Serialize)]
pub struct VersionSnapshot {
    pub major: i32,
    pub minor: i32,
    pub state: String,
    pub file_hash: Option<String>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

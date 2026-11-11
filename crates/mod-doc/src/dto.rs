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

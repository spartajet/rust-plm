//! mod-doc —— 图文档模块（MVP 最小集）：上传/列表/详情/下载。
//! 详设见概设 §4；upload 事务窗口与去重语义见 service.rs 模块注释。

pub mod dto;
pub mod service;

/// 本 crate 迁移集。`ignore_missing=true`：多 crate 迁移链共享 `_sqlx_migrations` 表，
/// 各 Migrator 跳过非自己管理的版本号（Task2 Ruling，见 SDD 台账）。
pub fn migrator() -> sqlx::migrate::Migrator {
    let mut m = sqlx::migrate!("./migrations");
    m.set_ignore_missing(true);
    m
}

pub use dto::{DocDetail, FileInfo};
pub use service::DocService;

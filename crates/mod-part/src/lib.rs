//! mod-part —— 物料主数据编排（增量17，PRD §6.3 PART-01/02/05 纵切）。
//! 编排链：分类校验 → COD 按分类规则取号（key=class code，独立流水）→ ObjectStore.create("part")。

pub mod service;

pub use service::{ClassNode, CreateResult, Duplicate, PartService};

/// 多 crate 迁移链（ignore_missing，同前例）
pub fn migrator() -> sqlx::migrate::Migrator {
    let mut m = sqlx::migrate!("./migrations");
    m.set_ignore_missing(true);
    m
}

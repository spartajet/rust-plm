//! core-bom —— BOM 引擎（概设 §3.3 / ADR-0003）。
//! 读路径全部走 bom_path 闭包表（V-2：10 万行展开 42ms）；写路径单事务增量维护。

pub mod store;

pub use store::{BomStore, ExpandMode, ExpandRow, LineInput};

/// 多 crate 迁移链
pub fn migrator() -> sqlx::migrate::Migrator {
    let mut m = sqlx::migrate!("./migrations");
    m.set_ignore_missing(true);
    m
}

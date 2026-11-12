//! core-version —— 版本与生命周期（概设 §3.2 / ADR-0004 语义子集）。

pub mod machine;
pub mod store;

pub use machine::LifecycleEvent;
pub use store::LifecycleStore;

/// 多 crate 迁移链（ignore_missing，同前例）
pub fn migrator() -> sqlx::migrate::Migrator {
    let mut m = sqlx::migrate!("./migrations");
    m.set_ignore_missing(true);
    m
}

//! mod-doc —— 图文档模块（MVP 最小集）。
//! Task2 实现 DocService；本文件先占位使 workspace 可编译。

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

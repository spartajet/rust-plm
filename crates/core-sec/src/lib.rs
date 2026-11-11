//! core-sec —— 认证域（ADR-0011）：登录(JWT/argon2id) + 用户仓储 + 内置 admin 引导。
//! 授权（RBAC/ABAC）后续增量；骨架期所有认证用户权限等价。

pub mod claims;
pub mod service;

pub use claims::Claims;
pub use service::{AdminBootInfo, SecService, TokenPair};

/// 多 crate 迁移链（ignore_missing，同 core-model/mod-doc 模式）
pub fn migrator() -> sqlx::migrate::Migrator {
    let mut m = sqlx::migrate!("./migrations");
    m.set_ignore_missing(true);
    m
}

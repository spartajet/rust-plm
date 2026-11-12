//! core-version —— 版本与生命周期（概设 §3.2 / ADR-0004 语义子集）。

pub mod machine;
pub mod store;

pub use machine::{FlowTier, LifecycleEvent};
pub use store::LifecycleStore;

/// 发布扩展点（增量9 BOM 版本链）：released 迁移**同事务**内回调。
/// core-version 不依赖任何业务 crate；实现方（plm-server 装配层）持 BomStore 等
/// 在回调里做发布冻结。Err → 整个迁移回滚（发布不落、事件不发）。
#[async_trait::async_trait]
pub trait ReleaseHook: Send + Sync {
    async fn on_released(
        &self,
        tx: &mut sqlx::PgConnection,
        type_code: &str,
        obj_no: &str,
        obj_id: uuid::Uuid,
        major: i32,
    ) -> core_model::Result<()>;
}

/// 多 crate 迁移链（ignore_missing，同前例）
pub fn migrator() -> sqlx::migrate::Migrator {
    let mut m = sqlx::migrate!("./migrations");
    m.set_ignore_missing(true);
    m
}

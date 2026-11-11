//! core-model —— RustPLM 元数据引擎（详细设计见 docs/PLM-详细设计-core-model.md）
//!
//! 职责（概设 §3.1 / ADR-0002）：
//! - 类型/属性/关系元数据的运行期建模与热缓存（TypeRegistry，arc-swap，改配置即时生效）
//! - 业务对象统一存取（ObjectStore：cm_object 单表 + attrs JSONB + 提升列）
//! - 动态查询编译（QueryBuilder：等值强制 `@>` containment，慢路径架构级禁止——Spike V-1 结论）
//! - 在线提升列迁移（Promoter：ALTER→分批回填→索引→版本失效，Spike V-1 实测 2-4ms DDL）
//! - 业务钩子总线（HookRegistry：save_before/submit_before/release_before）

pub mod error;
pub mod hooks;
pub mod promoter;
pub mod query;
pub mod registry;
pub mod store;
pub mod types;

pub use error::{DomainError, Result};
pub use hooks::{Hook, HookContext, HookOutcome, HookPoint, HookRegistry, HookResult};
pub use promoter::{PromotePlan, Promoter};
pub use query::{ObjectQuery, Op, Page, Sort, CompiledQuery, QueryBuilder};
pub use registry::{MetaSnapshot, TypeRegistry};
pub use store::{CreateCmd, ObjectStore, PgObjectStore};
pub use types::{AttrDef, AttrMap, BaseType, BizObject, BizObjectId, DataType, RelDef, TypeDef};

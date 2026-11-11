//! TypeRegistry —— 元数据热缓存（ADR-0002 / 概设 §3.1）。
//!
//! - 启动全量加载 → `ArcSwap<MetaSnapshot>` 供并发读（无锁热路径）；
//! - 变更感知：LISTEN `cm_meta_changed`（触发器即时通知）+ 周期兜底轮询 cm_meta_version；
//!   权限/缓存失效目标 3s 内生效（PRD ORG 验收）。

use crate::{DomainError, RelDef, Result, TypeDef};
use arc_swap::ArcSwap;
use sqlx::postgres::{PgListener, PgPool};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

/// 元数据快照（不可变整体替换，arc-swap 原子切换）
pub struct MetaSnapshot {
    pub version: u64,
    pub types: HashMap<String, Arc<TypeDef>>,
    pub rels: HashMap<String, Arc<RelDef>>,
}

impl MetaSnapshot {
    pub fn empty() -> Self {
        Self {
            version: 0,
            types: HashMap::new(),
            rels: HashMap::new(),
        }
    }
}

pub struct TypeRegistry {
    inner: ArcSwap<MetaSnapshot>,
    pool: PgPool,
}

impl TypeRegistry {
    /// 启动加载（执行迁移由 plm-server 启动器负责，这里只读）
    pub async fn load(pool: PgPool) -> Result<Arc<Self>> {
        let reg = Arc::new(Self {
            inner: ArcSwap::from_pointee(MetaSnapshot::empty()),
            pool,
        });
        let v = reg.reload().await?;
        info!(version = v, "TypeRegistry 初始加载完成");
        Ok(reg)
    }

    /// 当前元数据版本
    pub fn version(&self) -> u64 {
        self.inner.load().version
    }

    /// 取类型定义（热路径：Arc 无锁读）
    pub fn type_def(&self, code: &str) -> Result<Arc<TypeDef>> {
        self.inner
            .load()
            .types
            .get(code)
            .cloned()
            .ok_or_else(|| DomainError::TypeNotFound(code.to_string()))
    }

    pub fn rel_def(&self, code: &str) -> Result<Arc<RelDef>> {
        self.inner
            .load()
            .rels
            .get(code)
            .cloned()
            .ok_or_else(|| DomainError::Internal(format!("关系定义不存在: {code}")))
    }

    pub fn all_types(&self) -> Vec<Arc<TypeDef>> {
        self.inner.load().types.values().cloned().collect()
    }

    /// 重新加载全部元数据并原子切换（幂等；供 NOTIFY/轮询/手动触发）
    pub async fn reload(&self) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        let version: i64 = sqlx::query_scalar("SELECT version FROM cm_meta_version WHERE id = 1")
            .fetch_one(&mut *tx)
            .await?;
        let types = Self::load_types(&mut tx).await?;
        let rels = Self::load_rels(&mut tx).await?;
        tx.commit().await?;

        self.inner.store(Arc::new(MetaSnapshot {
            version: version as u64,
            types,
            rels,
        }));
        Ok(version as u64)
    }

    /// 变更监听：LISTEN cm_meta_changed（即时）+ interval 兜底轮询（防漏通知）。
    /// 返回后台 JoinHandle，由调用方持有到进程生命周期。
    pub async fn spawn_watcher(
        self: &Arc<Self>,
        fallback_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        let reg = Arc::clone(self);
        tokio::spawn(async move {
            let mut listener = match PgListener::connect_with(&reg.pool).await {
                Ok(l) => l,
                Err(e) => {
                    warn!(error = %e, "元数据 LISTEN 连接失败，退化为纯轮询");
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(fallback_secs)).await;
                        let _ = reg.reload_if_changed().await;
                    }
                }
            };
            if let Err(e) = listener.listen("cm_meta_changed").await {
                warn!(error = %e, "LISTEN cm_meta_changed 失败");
            }
            loop {
                // timeout 与 notify 双源：任一到达即检查版本
                let notify = async {
                    if listener.recv().await.is_ok() {
                        // 收到通知后稍等合并同批变更
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    }
                };
                tokio::select! {
                    _ = notify => {}
                    _ = tokio::time::sleep(std::time::Duration::from_secs(fallback_secs)) => {}
                }
                if let Err(e) = reg.reload_if_changed().await {
                    warn!(error = %e, "元数据热刷新失败（保留旧快照）");
                }
            }
        })
    }

    /// 仅当库内版本号更新时才 reload（避免无效全量加载）
    pub async fn reload_if_changed(&self) -> Result<u64> {
        let db_ver: i64 = sqlx::query_scalar("SELECT version FROM cm_meta_version WHERE id = 1")
            .fetch_one(&self.pool)
            .await?;
        if db_ver as u64 > self.version() {
            self.reload().await
        } else {
            Ok(self.version())
        }
    }

    async fn load_types(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<HashMap<String, Arc<TypeDef>>> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: uuid::Uuid,
            code: String,
            name: String,
            base_type: String,
            numbering_rule: Option<String>,
            title_attr: Option<String>,
            config: serde_json::Value,
            config_version: i32,
        }
        #[derive(sqlx::FromRow)]
        struct AttrRow {
            type_id: uuid::Uuid,
            code: String,
            name: String,
            data_type: String,
            required: bool,
            unique_flag: bool,
            default_val: Option<serde_json::Value>,
            ref_type: Option<String>,
            formula: Option<String>,
            promoted: bool,
            promoted_col: Option<String>,
            config: serde_json::Value,
            /// 仅用于 SQL ORDER BY 稳定排序，不参与映射
            #[allow(dead_code)]
            sort_no: i32,
        }

        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, code, name, base_type, numbering_rule, title_attr, config, config_version \
             FROM cm_type WHERE enabled ORDER BY code",
        )
        .fetch_all(&mut **tx)
        .await?;
        let attrs: Vec<AttrRow> = sqlx::query_as(
            "SELECT type_id, code, name, data_type, required, unique_flag, default_val, ref_type, \
                    formula, promoted, promoted_col, config, sort_no \
             FROM cm_attr WHERE enabled ORDER BY type_id, sort_no, code",
        )
        .fetch_all(&mut **tx)
        .await?;

        // 按 type_id 分组属性
        let mut attrs_by_type: HashMap<uuid::Uuid, Vec<crate::AttrDef>> = HashMap::new();
        for a in attrs {
            let data_type = crate::DataType::parse(&a.data_type).ok_or_else(|| {
                DomainError::Internal(format!("非法 data_type: {} {}", a.code, a.data_type))
            })?;
            attrs_by_type
                .entry(a.type_id)
                .or_default()
                .push(crate::AttrDef {
                    code: a.code,
                    name: a.name,
                    data_type,
                    required: a.required,
                    unique_flag: a.unique_flag,
                    default_val: a.default_val,
                    ref_type: a.ref_type,
                    formula: a.formula,
                    promoted: a.promoted,
                    promoted_col: a.promoted_col,
                    config: a.config,
                });
        }

        let mut map = HashMap::new();
        for r in rows {
            let base_type = crate::BaseType::parse(&r.base_type).ok_or_else(|| {
                DomainError::Internal(format!("非法 base_type: {} {}", r.code, r.base_type))
            })?;
            let attrs = attrs_by_type.remove(&r.id).unwrap_or_default();
            let attr_index = attrs
                .iter()
                .enumerate()
                .map(|(i, a)| (a.code.clone(), i))
                .collect();
            let td = TypeDef {
                id: r.id,
                code: r.code.clone(),
                name: r.name,
                base_type,
                numbering_rule: r.numbering_rule,
                title_attr: r.title_attr,
                config: r.config,
                config_version: r.config_version,
                attrs,
                attr_index,
            };
            map.insert(r.code, Arc::new(td));
        }
        Ok(map)
    }

    async fn load_rels(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<HashMap<String, Arc<RelDef>>> {
        let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT code, name, src_type, dst_type, cardinality FROM cm_rel_def WHERE enabled",
        )
        .fetch_all(&mut **tx)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(code, name, src_type, dst_type, cardinality)| {
                (
                    code.clone(),
                    Arc::new(RelDef {
                        code,
                        name,
                        src_type,
                        dst_type,
                        cardinality,
                    }),
                )
            })
            .collect())
    }
}

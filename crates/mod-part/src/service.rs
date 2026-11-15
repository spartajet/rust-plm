//! PartService：物料创建（分类赋码）/查重/列表（PRD PART-01/02/05 纵切）。

use core_model::cod::{next_code, CodRule};
use core_model::query::{Cond, ObjectQuery, Op, Page};
use core_model::types::AttrMap;
use core_model::{CreateCmd, ObjectStore};
use serde::Serialize;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// 分类节点（树形）
#[derive(Debug, Clone, Serialize)]
pub struct ClassNode {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub has_rule: bool,
    pub children: Vec<ClassNode>,
}

pub struct PartService {
    pool: PgPool,
    objects: Arc<dyn ObjectStore>,
}

#[derive(Debug, Serialize)]
pub struct Duplicate {
    pub obj_no: String,
    pub name: String,
    pub material: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateResult {
    pub obj_no: String,
    pub class_code: String,
    /// 相似件（不阻断创建——强制门禁后置，PART-05 完整版）
    pub duplicates: Vec<Duplicate>,
}

impl PartService {
    pub const TYPE: &str = "part";

    pub fn new(pool: PgPool, objects: Arc<dyn ObjectStore>) -> Self {
        Self { pool, objects }
    }

    /// 分类树（父子嵌套；增量17 Review Focus #5）
    pub async fn class_tree(&self) -> Result<Vec<ClassNode>, core_model::DomainError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            parent_id: Option<Uuid>,
            code: String,
            name: String,
            has_rule: bool,
        }
        // WITH RECURSIVE 计算深度（根 0）；ORDER BY depth DESC——深的先遍历（挂子算法依赖）
        let rows: Vec<Row> = sqlx::query_as(
            "WITH RECURSIVE t AS (                  SELECT id, parent_id, code, name, (cod_rule IS NOT NULL) AS has_rule, 0 AS depth                  FROM cm_class WHERE parent_id IS NULL AND enabled                  UNION ALL                  SELECT c.id, c.parent_id, c.code, c.name, (c.cod_rule IS NOT NULL) AS has_rule, t.depth + 1                  FROM cm_class c JOIN t ON c.parent_id = t.id WHERE c.enabled              ) SELECT id, parent_id, code, name, has_rule FROM t ORDER BY depth DESC, code",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| core_model::DomainError::Db(e.to_string()))?;
        // 终审 I2 重写：顺序无关（HashMap+挂子——父码字典序前提不再需要；
        // 孤儿（父缺失/禁用/环）不进根列表=不可达分类不出现在树中）
        let parent_of: std::collections::HashMap<Uuid, Uuid> = rows
            .iter()
            .filter_map(|r| r.parent_id.map(|p| (r.id, p)))
            .collect();
        let ordered: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
        let mut nodes: std::collections::HashMap<Uuid, ClassNode> = rows
            .into_iter()
            .map(|r| {
                (
                    r.id,
                    ClassNode {
                        id: r.id,
                        code: r.code,
                        name: r.name,
                        has_rule: r.has_rule,
                        children: Vec::new(),
                    },
                )
            })
            .collect();
        // 挂子：SQL 已按 depth DESC 排序——正序遍历=深的先 remove 挂入父（任意树形正确；兄弟按 code 稳定）
        let mut roots: Vec<ClassNode> = Vec::new();
        for id in &ordered {
            if let Some(node) = nodes.remove(id) {
                match parent_of.get(id).and_then(|p| nodes.get_mut(p)) {
                    // 有可见父 → 挂入（node 为最新——已含此前挂入的孙辈）
                    Some(pn) => pn.children.push(node),
                    // 无父（根）或父不可见（孤儿不上树）→ 进根列表
                    None => roots.push(node),
                }
            }
        }
        // 环节点残留 nodes（不进树，防死循环）
        let _ = nodes;
        Ok(roots)
    }
}

impl PartService {
    /// 查重（PART-05 MVP）：part 类型内 name/material ILIKE 模糊，LIMIT 5（低频交互调用——
    /// JSONB ILIKE 慢路径可接受，语义检索后置记账）
    pub async fn duplicates(
        &self,
        name: &str,
        material: Option<&str>,
    ) -> Result<Vec<Duplicate>, core_model::DomainError> {
        let rows: Vec<(String, serde_json::Value)> = sqlx::query_as(
            "SELECT obj_no, attrs FROM cm_object \
             WHERE deleted_at IS NULL \
               AND type_id = (SELECT id FROM cm_type WHERE code = 'part') \
               AND (attrs->>'name' ILIKE '%' || $1 || '%' \
                    OR ($2::text IS NOT NULL AND attrs->>'material' ILIKE '%' || $2 || '%')) \
             ORDER BY updated_at DESC LIMIT 5",
        )
        .bind(name)
        .bind(material.filter(|m| !m.trim().is_empty()))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| core_model::DomainError::Db(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|(obj_no, attrs)| Duplicate {
                obj_no,
                name: attrs
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                material: attrs
                    .get("material")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            })
            .collect())
    }

    /// 创建物料（分类赋码 + 查重提示；强制门禁后置）
    pub async fn create(
        &self,
        class_code: &str,
        attrs: AttrMap,
        actor: Option<Uuid>,
    ) -> Result<CreateResult, core_model::DomainError> {
        #[derive(sqlx::FromRow)]
        struct ClassRow {
            id: Uuid,
            code: String,
            name: String,
            cod_rule: Option<serde_json::Value>,
        }
        let class: ClassRow = sqlx::query_as(
            "SELECT id, code, name, cod_rule FROM cm_class WHERE code = $1 AND enabled",
        )
        .bind(class_code)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| core_model::DomainError::Db(e.to_string()))?
        .ok_or_else(|| core_model::DomainError::BadRequest(format!("分类不存在: {class_code}")))?;

        let rule = class
            .cod_rule
            .as_ref()
            .and_then(CodRule::from_config)
            .ok_or_else(|| {
                core_model::DomainError::BadRequest(format!(
                    "分类 {class_code} 未配置编码规则（cod_rule）"
                ))
            })?;
        let key = rule.date_key_now();
        // COD-02：计数器 key=class code（按规则独立流水）
        let obj_no = next_code(&self.pool, &class.code, &rule, &key).await?;

        // attrs 回填分类上下文
        let mut attrs = attrs;
        attrs.insert(
            "class_id".into(),
            serde_json::Value::String(class.id.to_string()),
        );
        attrs.insert(
            "class_code".into(),
            serde_json::Value::String(class.code.clone()),
        );
        attrs.insert(
            "class_name".into(),
            serde_json::Value::String(class.name.clone()),
        );

        let cmd = CreateCmd {
            type_code: Self::TYPE.into(),
            obj_no,
            attrs,
            owner: actor,
            secret_level: 0,
        };
        let obj = self.objects.create(cmd).await?;

        let duplicates = self
            .duplicates(
                obj.attrs
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
                obj.attrs.get("material").and_then(|v| v.as_str()),
            )
            .await?
            .into_iter()
            .filter(|d| d.obj_no != obj.obj_no)
            .collect();

        Ok(CreateResult {
            obj_no: obj.obj_no,
            class_code: class.code,
            duplicates,
        })
    }

    /// 列表（ObjectQuery 复用；ABAC 由网关注入密级条件）
    pub async fn list(
        &self,
        name_filter: Option<&str>,
        page: u32,
        size: u32,
        max_secret: i16,
    ) -> Result<(Vec<core_model::types::BizObject>, u64), core_model::DomainError> {
        let mut conds = vec![Cond {
            attr: "secret_level".into(),
            op: Op::Le,
            value: serde_json::json!(max_secret),
        }];
        if let Some(n) = name_filter.filter(|n| !n.trim().is_empty()) {
            conds.push(Cond {
                attr: "name".into(),
                op: Op::Eq,
                value: serde_json::json!(n),
            });
        }
        let q = ObjectQuery {
            type_code: Self::TYPE.into(),
            conds,
            sorts: vec![],
            page: Some(Page { page, size }),
        };
        self.objects.query(&q).await
    }
}

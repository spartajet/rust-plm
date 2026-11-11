//! QueryBuilder —— 动态查询编译器（Spike V-1 结论的产品化落地）。
//!
//! 编译规则（ADR-0002 + spikes/REPORT.md 结论 3）：
//! | 操作 | 提升列 | 未提升（JSONB） |
//! |------|--------|------------------|
//! | eq / in | 物理列 `=` / `= ANY` | **强制 `attrs @> $n::jsonb`**（GIN，0.9ms@百万行） |
//! | isnull | `p_x IS NULL` | `NOT (attrs ? code)`（GIN 支持） |
//! | like / gt / ge / lt / le / 排序 | 物理列 | **拒绝（QRY_PROMOTE_REQUIRED）**——架构级禁止 `->>` 慢路径（202ms 反例） |
//!
//! 系统列（obj_no/state/owner/created_at/updated_at/attrs_rev 等）直接走物理列白名单。

use crate::{DomainError, Result, TypeDef};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 过滤操作符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Eq,
    Ne,
    In,
    Like,
    Gt,
    Ge,
    Lt,
    Le,
    IsNull,
}

impl Op {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Ne => "ne",
            Self::In => "in",
            Self::Like => "like",
            Self::Gt => "gt",
            Self::Ge => "ge",
            Self::Lt => "lt",
            Self::Le => "le",
            Self::IsNull => "isnull",
        }
    }
}

/// 单个过滤条件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cond {
    pub attr: String,
    pub op: Op,
    pub value: Value,
}

/// 排序项（attr 为系统列或提升列）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sort {
    pub attr: String,
    #[serde(default)]
    pub desc: bool,
}

/// 分页（size 上限 500，ADR-0015）
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Page {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_size")]
    pub size: u32,
}

fn default_page() -> u32 {
    1
}
fn default_size() -> u32 {
    50
}

impl Default for Page {
    fn default() -> Self {
        Self { page: 1, size: 50 }
    }
}

pub const MAX_PAGE_SIZE: u32 = 500;
/// 未提升属性 IN 条件的最大值数（超过引导提升列）
pub const MAX_IN_VALUES: usize = 32;

/// 对象查询（V1：条件间 AND；OR 组 V1.1 扩展 `or` 字段）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObjectQuery {
    pub type_code: String,
    #[serde(default)]
    pub conds: Vec<Cond>,
    #[serde(default)]
    pub sorts: Vec<Sort>,
    #[serde(default)]
    pub page: Option<Page>,
}

/// 绑定值：JSON 值 + 目标 PG 类型提示
#[derive(Debug, Clone)]
pub struct Bind {
    pub value: Value,
    pub cast: Option<&'static str>,
}

/// 编译产物：WHERE 片段（`$1..$n` 占位）+ 顺序绑定点
#[derive(Debug, Clone)]
pub struct CompiledQuery {
    pub type_def: std::sync::Arc<TypeDef>,
    pub where_sql: String,
    pub binds: Vec<Bind>,
    pub order_sql: String,
    pub limit: u32,
    pub offset: u32,
}

/// 系统列白名单（可直接用于过滤/排序的物理列）
const SYSTEM_COLS: &[(&str, &str)] = &[
    ("obj_no", "text"),
    ("major_ver", "int"),
    ("minor_ver", "int"),
    ("state", "text"),
    ("secret_level", "smallint"),
    ("owner", "uuid"),
    ("created_at", "timestamptz"),
    ("updated_at", "timestamptz"),
    ("created_by", "uuid"),
    ("attrs_rev", "int"),
];

fn system_col(name: &str) -> Option<&'static str> {
    SYSTEM_COLS.iter().find(|(c, _)| *c == name).map(|(_, t)| *t)
}

pub struct QueryBuilder;

impl QueryBuilder {
    /// 编译查询（纯函数，无 IO；可单测穷举矩阵——TQ-01）
    pub fn compile(td: std::sync::Arc<TypeDef>, q: &ObjectQuery) -> Result<CompiledQuery> {
        let page = q.page.unwrap_or_default();
        if page.size == 0 || page.size > MAX_PAGE_SIZE {
            return Err(DomainError::QryPageTooLarge(MAX_PAGE_SIZE));
        }

        let mut wheres: Vec<String> = vec!["deleted_at IS NULL".to_string()];
        let mut binds: Vec<Bind> = Vec::new();

        for c in &q.conds {
            wheres.push(Self::compile_cond(&td, c, &mut binds)?);
        }

        // 排序：系统列或提升列，否则拒绝
        let mut orders: Vec<String> = Vec::new();
        for s in &q.sorts {
            if let Some(_) = system_col(&s.attr) {
                orders.push(format!("{} {}", s.attr, if s.desc { "DESC" } else { "ASC" }));
            } else {
                let col = td
                    .promoted_col(&s.attr)
                    .ok_or_else(|| DomainError::QryPromoteRequired("sort".into(), s.attr.clone()))?;
                orders.push(format!("{col} {}", if s.desc { "DESC" } else { "ASC" }));
            }
        }
        if orders.is_empty() {
            orders.push("updated_at DESC".to_string());
        }

        Ok(CompiledQuery {
            type_def: td,
            where_sql: wheres.join(" AND "),
            binds,
            order_sql: orders.join(", "),
            limit: page.size,
            offset: (page.page.saturating_sub(1)) * page.size,
        })
    }

    fn compile_cond(td: &TypeDef, c: &Cond, binds: &mut Vec<Bind>) -> Result<String> {
        // 1) 系统列直通
        if let Some(pg_type) = system_col(&c.attr) {
            return Self::binded_op(&c.attr, Some(pg_type), c, binds, true);
        }
        // 2) 属性列
        let attr = td.attr_or_err(&c.attr)?;
        if attr.promoted {
            if let Some(col) = attr.promoted_col.as_deref() {
                return Self::binded_op(col, Some(attr.data_type.pg_column_type()), c, binds, true);
            }
        }
        // 3) 未提升：eq/in/isnull 走 GIN 语义；其余拒绝
        match c.op {
            Op::Eq => {
                let i = binds.len() as i32 + 1;
                let mut m = serde_json::Map::new();
                m.insert(c.attr.clone(), c.value.clone());
                binds.push(Bind { value: Value::Object(m), cast: Some("jsonb") });
                Ok(format!("attrs @> ${i}::jsonb"))
            }
            Op::In => {
                let values = c
                    .value
                    .as_array()
                    .ok_or_else(|| DomainError::AttrTypeMismatch(c.attr.clone(), "array".into()))?;
                if values.len() > MAX_IN_VALUES {
                    return Err(DomainError::QryInTooMany(MAX_IN_VALUES));
                }
                // 多个 @> 以 OR 组合（GIN BitmapOr），保持 100% 走索引
                let parts: Vec<String> = values
                    .iter()
                    .map(|v| {
                        let i = binds.len() as i32 + 1;
                        let mut m = serde_json::Map::new();
                        m.insert(c.attr.clone(), v.clone());
                        binds.push(Bind { value: Value::Object(m), cast: Some("jsonb") });
                        format!("attrs @> ${i}::jsonb")
                    })
                    .collect();
                Ok(format!("({})", parts.join(" OR ")))
            }
            Op::IsNull => Ok(format!("NOT (attrs ? '{}')", c.attr.replace('\'', "''"))),
            op => Err(DomainError::QryPromoteRequired(op.as_str().into(), c.attr.clone())),
        }
    }

    /// 物理列（系统列/提升列）的操作编译
    fn binded_op(
        col: &str,
        pg_type: Option<&'static str>,
        c: &Cond,
        binds: &mut Vec<Bind>,
        _physical: bool,
    ) -> Result<String> {
        let i = binds.len() as i32 + 1;
        let cast = pg_type.map(|t| match t {
            "numeric(24,6)" => "numeric",
            other => other,
        });
        let suffix = cast.map(|t| format!("::{}", t)).unwrap_or_default();
        Ok(match c.op {
            Op::Eq => {
                binds.push(Bind { value: c.value.clone(), cast });
                format!("{col} = ${i}{suffix}")
            }
            Op::Ne => {
                binds.push(Bind { value: c.value.clone(), cast });
                format!("{col} <> ${i}{suffix}")
            }
            Op::Gt => {
                binds.push(Bind { value: c.value.clone(), cast });
                format!("{col} > ${i}{suffix}")
            }
            Op::Ge => {
                binds.push(Bind { value: c.value.clone(), cast });
                format!("{col} >= ${i}{suffix}")
            }
            Op::Lt => {
                binds.push(Bind { value: c.value.clone(), cast });
                format!("{col} < ${i}{suffix}")
            }
            Op::Le => {
                binds.push(Bind { value: c.value.clone(), cast });
                format!("{col} <= ${i}{suffix}")
            }
            Op::Like => {
                binds.push(Bind { value: c.value.clone(), cast: Some("text") });
                format!("{col} LIKE ${i}::text")
            }
            Op::In => {
                let values = c
                    .value
                    .as_array()
                    .ok_or_else(|| DomainError::AttrTypeMismatch(c.attr.clone(), "array".into()))?;
                binds.push(Bind { value: Value::Array(values.clone()), cast: Some("jsonb") });
                format!("{col}::text = ANY(SELECT jsonb_array_elements_text(${i}::jsonb))")
            }
            Op::IsNull => format!("{col} IS NULL"),
        })
    }
}

//! 核心领域类型：对象标识、属性模型、类型定义、业务对象。
//! 这些类型是跨 crate 的契约（serde DTO），不泄漏数据库行结构。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use uuid::Uuid;

/// 业务对象标识（类型 + 编号 + 版本，全局唯一定位一个对象版本）
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BizObjectId {
    pub type_code: String,
    pub obj_no: String,
    pub major: i32,
    pub minor: i32,
}

impl fmt::Display for BizObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}.{}",
            self.type_code, self.obj_no, self.major, self.minor
        )
    }
}

/// 自定义属性集合（JSONB 值域）
pub type AttrMap = serde_json::Map<String, serde_json::Value>;

/// 属性数据类型（与 cm_attr.data_type 一一对应）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataType {
    Text,
    Int,
    Decimal,
    Date,
    Bool,
    Enum,
    Ref,
    Unit,
    Json,
}

impl DataType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text | Self::Enum | Self::Ref => "text",
            Self::Int => "int",
            Self::Decimal | Self::Unit => "decimal",
            Self::Date => "date",
            Self::Bool => "bool",
            Self::Json => "json",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "text" => Self::Text,
            "int" => Self::Int,
            "decimal" => Self::Decimal,
            "date" => Self::Date,
            "bool" => Self::Bool,
            "enum" => Self::Enum,
            "ref" => Self::Ref,
            "unit" => Self::Unit,
            "json" => Self::Json,
            _ => return None,
        })
    }

    /// 提升为物理列时的 PG 列类型（ADR-0002 类型映射）
    pub fn pg_column_type(&self) -> &'static str {
        match self {
            Self::Text | Self::Enum | Self::Ref => "text",
            Self::Int => "bigint",
            Self::Decimal | Self::Unit => "numeric(24,6)",
            Self::Date => "timestamptz",
            Self::Bool => "boolean",
            Self::Json => "jsonb",
        }
    }
}

/// 类型基类（决定模块挂接与默认行为，概设 §4）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BaseType {
    Part,
    Document,
    Change,
    Project,
    Bom,
    Generic,
}

impl BaseType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Part => "part",
            Self::Document => "document",
            Self::Change => "change",
            Self::Project => "project",
            Self::Bom => "bom",
            Self::Generic => "generic",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "part" => Self::Part,
            "document" => Self::Document,
            "change" => Self::Change,
            "project" => Self::Project,
            "bom" => Self::Bom,
            "generic" => Self::Generic,
            _ => return None,
        })
    }
}

/// 属性定义（cm_attr 的内存映像）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttrDef {
    pub code: String,
    pub name: String,
    pub data_type: DataType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub unique_flag: bool,
    #[serde(default)]
    pub default_val: Option<serde_json::Value>,
    pub ref_type: Option<String>,
    pub formula: Option<String>,
    /// 是否已提升为物理列（cm_object.p_<code>）
    #[serde(default)]
    pub promoted: bool,
    pub promoted_col: Option<String>,
    #[serde(default = "default_config")]
    pub config: serde_json::Value,
}

fn default_config() -> serde_json::Value {
    serde_json::json!({})
}

/// 对象类型定义（cm_type + 其属性集的聚合，TypeRegistry 缓存单元）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDef {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub base_type: BaseType,
    pub numbering_rule: Option<String>,
    pub title_attr: Option<String>,
    pub config: serde_json::Value,
    pub config_version: i32,
    pub attrs: Vec<AttrDef>,
    /// code -> attrs 下标（构建时生成，加速查询编译）
    #[serde(skip)]
    pub attr_index: HashMap<String, usize>,
}

impl TypeDef {
    pub fn attr(&self, code: &str) -> Option<&AttrDef> {
        self.attr_index.get(code).map(|&i| &self.attrs[i])
    }

    pub fn attr_or_err(&self, code: &str) -> crate::Result<&AttrDef> {
        self.attr(code)
            .ok_or_else(|| crate::DomainError::AttrNotFound(self.code.clone(), code.to_string()))
    }

    /// 提升列物理名
    pub fn promoted_col(&self, attr_code: &str) -> Option<&str> {
        self.attr(attr_code)
            .filter(|a| a.promoted)
            .and_then(|a| a.promoted_col.as_deref())
    }
}

/// 关系定义（cm_rel_def 的内存映像）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelDef {
    pub code: String,
    pub name: String,
    pub src_type: String,
    pub dst_type: String,
    pub cardinality: String,
}

/// 业务对象（cm_object 行的领域映像）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BizObject {
    pub id: Uuid,
    pub type_id: Uuid,
    pub type_code: String,
    pub obj_no: String,
    pub major: i32,
    pub minor: i32,
    pub iteration: i32,
    /// 生命周期状态（working/review/released/obsolete + 扩展态），迁移由 core-version 驱动
    pub state: String,
    pub secret_level: i16,
    pub owner: Option<Uuid>,
    pub attrs: AttrMap,
    /// 乐观锁版本
    pub attrs_rev: i32,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
}

impl BizObject {
    pub fn biz_id(&self) -> BizObjectId {
        BizObjectId {
            type_code: self.type_code.clone(),
            obj_no: self.obj_no.clone(),
            major: self.major,
            minor: self.minor,
        }
    }
}

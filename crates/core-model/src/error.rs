//! 领域错误：core 层强类型错误（thiserror，ADR 选型）；`code()` 为稳定错误码，
//! api-gateway 直接映射 HTTP 状态与前端 i18n key（ADR-0015）。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    // ---- 类型/属性 ----
    #[error("对象类型不存在或已停用: {0}")]
    TypeNotFound(String),
    #[error("属性不存在: {0}.{1}")]
    AttrNotFound(String, String),
    #[error("必填属性缺失: {0}")]
    AttrRequired(String),
    #[error("属性值类型不合法: {0} 期望 {1}")]
    AttrTypeMismatch(String, String),
    #[error("属性值超出枚举范围: {0}")]
    AttrEnumInvalid(String),
    #[error("属性唯一性冲突: {0}={1}")]
    AttrUniqueViolation(String, String),

    // ---- 对象 ----
    #[error("对象不存在: {0}")]
    ObjNotFound(String),
    #[error("对象版本冲突（乐观锁）: {0}")]
    ObjVersionConflict(String),
    #[error("对象编号重复: {0}")]
    ObjNoDuplicate(String),
    #[error("对象非工作态，禁止直接修改（须走变更流程）: {0}")]
    ObjStateForbidden(String),

    // ---- 查询 ----
    #[error("操作符 {0} 需要属性提升为列（或使用系统列）: {1}")]
    QryPromoteRequired(String, String),
    #[error("IN 值过多（>{0}），请提升为列或收窄条件")]
    QryInTooMany(usize),
    #[error("页大小超限（上限 {0}）")]
    QryPageTooLarge(u32),

    // ---- 钩子/基础设施 ----
    #[error("钩子拒绝: {0}")]
    HookRejected(String),
    #[error("钩子执行超时: {0}")]
    HookTimeout(String),
    #[error("数据库错误: {0}")]
    Db(String),
    #[error("内部错误: {0}")]
    Internal(String),

    // ---- 文件/请求字段（mod-doc 骨架期） ----
    #[error("请求字段缺失: {0}")]
    FieldRequired(String),
    #[error("文件错误: {0}")]
    File(String),
    #[error("上传包含多个文件（当前单文件契约）")]
    FileMulti,
    #[error("文件内容为空")]
    FileEmpty,
    #[error("文件过大（上限 {0} 字节）")]
    FileTooLarge(u64),
    #[error("请求不合法: {0}")]
    BadRequest(String),
}

impl DomainError {
    /// 稳定错误码（API 错误体 code 字段；一旦发布不可变更）
    pub fn code(&self) -> &'static str {
        match self {
            Self::TypeNotFound(_) => "MDL-TYPE-NOT-FOUND",
            Self::AttrNotFound(_, _) => "MDL-ATTR-NOT-FOUND",
            Self::AttrRequired(_) => "MDL-ATTR-REQUIRED",
            Self::AttrTypeMismatch(_, _) => "MDL-ATTR-TYPE",
            Self::AttrEnumInvalid(_) => "MDL-ATTR-ENUM",
            Self::AttrUniqueViolation(_, _) => "MDL-ATTR-UNIQUE",
            Self::ObjNotFound(_) => "MDL-OBJ-NOT-FOUND",
            Self::ObjVersionConflict(_) => "MDL-OBJ-VERSION-CONFLICT",
            Self::ObjNoDuplicate(_) => "MDL-OBJ-NO-DUPLICATE",
            Self::ObjStateForbidden(_) => "MDL-OBJ-STATE-FORBIDDEN",
            Self::QryPromoteRequired(_, _) => "MDL-QRY-PROMOTE-REQUIRED",
            Self::QryInTooMany(_) => "MDL-QRY-IN-TOO-MANY",
            Self::QryPageTooLarge(_) => "MDL-QRY-PAGE-TOO-LARGE",
            Self::HookRejected(_) => "MDL-HOOK-REJECTED",
            Self::HookTimeout(_) => "MDL-HOOK-TIMEOUT",
            Self::Db(_) => "MDL-DB",
            Self::Internal(_) => "MDL-INTERNAL",
            Self::FieldRequired(_) => "MDL-FIELD-REQUIRED",
            Self::File(_) => "MDL-FILE",
            Self::FileMulti => "MDL-FILE-MULTI",
            Self::FileEmpty => "MDL-FILE-EMPTY",
            Self::FileTooLarge(_) => "MDL-FILE-TOO-LARGE",
            Self::BadRequest(_) => "MDL-BAD-REQUEST",
        }
    }

    /// 建议 HTTP 状态（api-gateway 参考）
    pub fn http_status(&self) -> u16 {
        match self {
            Self::TypeNotFound(_) | Self::AttrNotFound(_, _) | Self::ObjNotFound(_) => 404,
            Self::ObjVersionConflict(_)
            | Self::ObjNoDuplicate(_)
            | Self::AttrUniqueViolation(..) => 409,
            Self::ObjStateForbidden(_) => 423,
            Self::QryPromoteRequired(..)
            | Self::QryInTooMany(_)
            | Self::QryPageTooLarge(_)
            | Self::AttrRequired(_)
            | Self::AttrTypeMismatch(..)
            | Self::AttrEnumInvalid(_) => 400,
            Self::HookRejected(_) => 422,
            Self::HookTimeout(_) => 504,
            Self::FieldRequired(_) | Self::FileMulti | Self::FileEmpty | Self::BadRequest(_) => 400,
            Self::FileTooLarge(_) => 413,
            _ => 500,
        }
    }
}

impl From<sqlx::Error> for DomainError {
    fn from(e: sqlx::Error) -> Self {
        DomainError::Db(e.to_string())
    }
}

pub type Result<T, E = DomainError> = std::result::Result<T, E>;

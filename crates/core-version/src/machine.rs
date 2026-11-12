//! 表驱动状态机（纯函数，无 IO——TQ-01 可穷举单测；后续扩展状态经 TypeDef.config 注入）。

use core_model::{DomainError, Result};

/// 生命周期事件（serde 小写；网关路径参数直传）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleEvent {
    Submit,
    Approve,
    Reject,
    Release,
    Obsolete,
}

impl LifecycleEvent {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "submit" => Self::Submit,
            "approve" => Self::Approve,
            "reject" => Self::Reject,
            "release" => Self::Release,
            "obsolete" => Self::Obsolete,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Submit => "submit",
            Self::Approve => "approve",
            Self::Reject => "reject",
            Self::Release => "release",
            Self::Obsolete => "obsolete",
        }
    }
}

/// 默认迁移表：working→(submit)→review→(approve)→released→(obsolete)→obsolete；
/// review→(reject)→working；released→(submit)→review（升版审批，经变更流程时使用）。
pub fn allowed(state: &str, ev: LifecycleEvent) -> Option<&'static str> {
    match (state, ev) {
        ("working", LifecycleEvent::Submit) => Some("review"),
        ("review", LifecycleEvent::Approve) => Some("released"),
        ("review", LifecycleEvent::Reject) => Some("working"),
        ("released", LifecycleEvent::Obsolete) => Some("obsolete"),
        _ => None,
    }
}

/// 带领域错误的便捷包装
pub fn transition(state: &str, ev: LifecycleEvent) -> Result<&'static str> {
    allowed(state, ev)
        .ok_or_else(|| DomainError::Internal(format!("非法迁移 {state} --{}-->", ev.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_chain_legal() {
        assert_eq!(
            transition("working", LifecycleEvent::Submit).unwrap(),
            "review"
        );
        assert_eq!(
            transition("review", LifecycleEvent::Approve).unwrap(),
            "released"
        );
        assert_eq!(
            transition("released", LifecycleEvent::Obsolete).unwrap(),
            "obsolete"
        );
    }

    #[test]
    fn reject_back_to_working() {
        assert_eq!(
            transition("review", LifecycleEvent::Reject).unwrap(),
            "working"
        );
    }

    #[test]
    fn illegal_paths_denied() {
        assert!(transition("working", LifecycleEvent::Release).is_err()); // 跳过审批
        assert!(transition("released", LifecycleEvent::Submit).is_err()); // V1 无升版提交
        assert!(transition("obsolete", LifecycleEvent::Submit).is_err());
        assert!(transition("review", LifecycleEvent::Release).is_err());
    }
}

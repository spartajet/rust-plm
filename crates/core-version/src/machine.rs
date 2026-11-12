//! 表驱动状态机（纯函数，无 IO——TQ-01 可穷举单测）。
//! 档位参数化（增量5 / PRD SMB-2）：minimal 极简 / standard 标准 / strict 严格两级+会签。

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

/// 流程档位（PRD SMB-2）：存于 cm_type.config.flow_tier，经 TypeRegistry 热生效；缺省 standard。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowTier {
    Minimal,
    Standard,
    Strict,
}

impl FlowTier {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "minimal" => Self::Minimal,
            "standard" => Self::Standard,
            "strict" => Self::Strict,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Standard => "standard",
            Self::Strict => "strict",
        }
    }
}

/// standard 档迁移表（兼容包装=allowed_for(Standard)）
pub fn allowed(state: &str, ev: LifecycleEvent) -> Option<&'static str> {
    allowed_for(FlowTier::Standard, state, ev)
}

/// 档位化迁移表（增量5 / SMB-2）。
/// 兜底语义：切档遗留的 review/checked 中间态在任意档位均允许 Approve→released / Reject→working（防卡死）。
pub fn allowed_for(tier: FlowTier, state: &str, ev: LifecycleEvent) -> Option<&'static str> {
    // 跨档兜底：中间态永不卡死（Reject 任意档回 working；Approve 目标态由档位决定）
    match (state, ev) {
        ("review", LifecycleEvent::Reject) => return Some("working"),
        ("checked", LifecycleEvent::Reject) => return Some("working"),
        _ => {}
    }
    match (state, ev) {
        ("working", LifecycleEvent::Submit) => match tier {
            FlowTier::Minimal => Some("released"), // 一键发布（SMB-2 留痕直达）
            _ => Some("review"),
        },
        ("review", LifecycleEvent::Approve) => match tier {
            FlowTier::Strict => Some("checked"), // 两级审批第一级
            _ => Some("released"),
        },
        ("checked", LifecycleEvent::Approve) if tier == FlowTier::Strict => Some("released"),
        ("released", LifecycleEvent::Obsolete) => Some("obsolete"),
        _ => None,
    }
}

/// 带领域错误的便捷包装（非法迁移 → 409 IllegalTransition；终审 I2 修复）
pub fn transition(state: &str, ev: LifecycleEvent) -> Result<&'static str> {
    allowed(state, ev)
        .ok_or_else(|| DomainError::IllegalTransition(state.to_string(), ev.as_str().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_full_chain_legal() {
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
        assert!(transition("working", LifecycleEvent::Release).is_err());
        assert!(transition("released", LifecycleEvent::Submit).is_err());
        assert!(transition("obsolete", LifecycleEvent::Submit).is_err());
        assert!(transition("review", LifecycleEvent::Release).is_err());
    }

    #[test]
    fn illegal_paths_are_409_not_500() {
        // 终审 I2 回归钉：非法迁移必须是 IllegalTransition（409）
        let e = transition("working", LifecycleEvent::Release).unwrap_err();
        assert_eq!(e.code(), "MDL-LCS-TRANSITION");
    }

    // ---- 档位矩阵（增量5） ----

    #[test]
    fn minimal_submit_reaches_released_directly() {
        assert_eq!(
            allowed_for(FlowTier::Minimal, "working", LifecycleEvent::Submit),
            Some("released")
        );
        // minimal 无中间态：review/checked 上无独特迁移（走兜底）
        assert_eq!(
            allowed_for(FlowTier::Minimal, "review", LifecycleEvent::Approve),
            Some("released")
        );
    }

    #[test]
    fn strict_requires_two_level_approvals() {
        assert_eq!(
            allowed_for(FlowTier::Strict, "working", LifecycleEvent::Submit),
            Some("review")
        );
        assert_eq!(
            allowed_for(FlowTier::Strict, "review", LifecycleEvent::Approve),
            Some("checked")
        );
        assert_eq!(
            allowed_for(FlowTier::Strict, "checked", LifecycleEvent::Approve),
            Some("released")
        );
        // strict 不得跳过 checked
        assert_eq!(
            allowed_for(FlowTier::Strict, "review", LifecycleEvent::Release),
            None
        );
    }

    #[test]
    fn checked_reject_back_to_working_all_tiers() {
        assert_eq!(
            allowed_for(FlowTier::Strict, "checked", LifecycleEvent::Reject),
            Some("working")
        );
    }

    #[test]
    fn obsolete_common_across_tiers() {
        for t in [FlowTier::Minimal, FlowTier::Standard, FlowTier::Strict] {
            assert_eq!(
                allowed_for(t, "released", LifecycleEvent::Obsolete),
                Some("obsolete")
            );
        }
    }
}

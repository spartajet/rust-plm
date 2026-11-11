//! HookRegistry —— 业务钩子总线（AGL-07 的执行载体，概设 §3.1）。
//!
//! 挂接点固定三类核心 + 自定义；执行顺序：内置规则 → rhai 脚本 → 插件（core-plugins 挂载）。
//! 任一 Reject 中止事务；单个处理器默认 50ms 超时熔断（配置可调）。

use crate::{AttrMap, DomainError, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// 钩子挂接点
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HookPoint {
    /// 对象创建前（校验/默认值/编号前置）
    SaveBefore,
    /// 提交流程前（门禁：必填/引用完整性/BOM 齐套等）
    SubmitBefore,
    /// 发布前（发布级强校验）
    ReleaseBefore,
    /// 自定义挂点（mod-*/插件声明）
    Custom(String),
}

/// 钩子执行上下文
pub struct HookContext {
    pub type_code: String,
    pub obj_no: String,
    pub state: String,
    pub attrs: AttrMap,
    /// 触发动作的执行者
    pub actor: Uuid,
}

/// 单个钩子处理器的裁决
pub enum HookResult {
    Pass,
    /// 修改属性后继续（顺序应用，后者可见前者的 patch）
    Patch(AttrMap),
    /// 拒绝（中止事务，携带用户可读原因）
    Reject(String),
}

/// 钩子处理器接口（内置规则、rhai 包装、插件包装统一实现此 trait）
#[async_trait]
pub trait Hook: Send + Sync {
    fn point(&self) -> &HookPoint;
    fn name(&self) -> &str;
    async fn run(&self, ctx: &HookContext) -> Result<HookResult>;
}

pub struct HookRegistry {
    hooks: Vec<Arc<dyn Hook>>,
    /// 单处理器超时（默认 50ms）
    pub timeout: Duration,
}

/// 一轮挂点的聚合结果
pub enum HookOutcome {
    /// 全部通过（含依次应用的 patch）
    Applied(AttrMap),
    /// 被拒绝（处理器名 + 原因）
    Denied(String, String),
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            hooks: Vec::new(),
            timeout: Duration::from_millis(50),
        }
    }

    pub fn register(&mut self, hook: Arc<dyn Hook>) {
        self.hooks.push(hook);
    }

    /// 执行某挂点的全部钩子（同挂点按注册序；超时按 HookTimeout 熔断）
    pub async fn run(&self, point: &HookPoint, ctx: &HookContext) -> Result<HookOutcome> {
        let mut merged: AttrMap = AttrMap::new();
        for h in self.hooks.iter().filter(|h| h.point() == point) {
            let fut = h.run(ctx);
            let r = match tokio::time::timeout(self.timeout, fut).await {
                Ok(r) => r?,
                Err(_) => {
                    return Err(DomainError::HookTimeout(format!(
                        "{}:{}",
                        point_name(point),
                        h.name()
                    )))
                }
            };
            match r {
                HookResult::Pass => {}
                HookResult::Patch(p) => {
                    for (k, v) in p {
                        merged.insert(k, v);
                    }
                }
                HookResult::Reject(reason) => {
                    return Ok(HookOutcome::Denied(h.name().to_string(), reason))
                }
            }
        }
        Ok(HookOutcome::Applied(merged))
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn point_name(p: &HookPoint) -> &str {
    match p {
        HookPoint::SaveBefore => "save_before",
        HookPoint::SubmitBefore => "submit_before",
        HookPoint::ReleaseBefore => "release_before",
        HookPoint::Custom(c) => c,
    }
}

/// 内置钩子示例：必填校验（提交/发布档强校验，草稿档宽松——SMB-1）
pub struct RequiredAttrsHook {
    pub point: HookPoint,
    pub attrs: Vec<String>,
}

#[async_trait]
impl Hook for RequiredAttrsHook {
    fn point(&self) -> &HookPoint {
        &self.point
    }
    fn name(&self) -> &str {
        "builtin:required-attrs"
    }
    async fn run(&self, ctx: &HookContext) -> Result<HookResult> {
        for a in &self.attrs {
            let empty = matches!(ctx.attrs.get(a), None | Some(Value::Null));
            if empty {
                return Ok(HookResult::Reject(format!("必填属性缺失: {a}")));
            }
        }
        Ok(HookResult::Pass)
    }
}

# ADR-0016 LLM 接入：rig 框架 + 中间件包装层

- 状态：已接受（2026-11-11，用户选型批次 3）
- 关联需求：AI-01~04、ADR-0010（Provider 抽象）

## 背景

svc-ai 网关需要在多家 LLM（DeepSeek/Qwen/Kimi/OpenAI 兼容系 + Ollama 本地）之上实现场景路由、脱敏、审计、计量与降级。自研 reqwest 实现可控但样板多；rig 是 Rust 生态最活跃的 LLM 框架，内置多 Provider 抽象与工具调用。

## 决策

采用 **rig** 作为 LLM 接入层：svc-ai 的 `LlmProvider` trait 由 rig 适配实现（OpenAI 兼容/Ollama）；**必须实现的包装层**：脱敏（上云前词表替换）、全量调用审计、Token 计量配额、超时熔断与备选链路（主→备→降级人工），包装层实现为 rig Provider 外层的组合装饰器，保持可替换性。

## 后果

- ✅ Provider 生态与工具调用免费获得；自研量减少；
- ⚠️ rig 迭代快、API 偶有破坏性变更——锁版本 + 适配层隔离（业务只见自家 `LlmProvider` trait）；
- ⚠️ 脱敏/审计必须先于 rig 调用执行（不能依赖 rig 钩子）——在网关入口统一拦截，测试用例覆盖"上云 payload 无敏感词"断言。

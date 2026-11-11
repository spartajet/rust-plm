# ADR-0009 服务端底座：axum + tokio

- 状态：已接受（2026-11-11）
- 关联需求：INT-01（OpenAPI）、F2（并发/延迟）、INT-04（IM 回调）

## 背景

备选：actix-web（性能标杆但自有 Actor 运行时、生态略割裂）、poem（API 友好但社区小）、axum（tokio 官方系、tower 中间件生态、类型安全提取器、社区最活跃）。

## 决策

axum 0.8 + tokio：tower 中间件链承载鉴权/限流/审计/trace（core-sec 统一注入）；OpenAPI 用 utoipa 注解自动生成（INT-01）；大结果集（BOM 展开/导出）走流式 Response（NDJSON）；IM 平台回调（钉钉/企微/飞书/微信）统一验签中间件后进 conn-im。

## 后果

- ✅ 与 tonic/wasmtime/tantivy 同属 tokio 生态，运行时统一；
- ✅ 性能满足 F2（axum 与 actix 在本系统瓶颈即数据库，差异可忽略）；
- ⚠️ axum 大版本升级历史上破坏性较强——锁次要版本 + 升级评估入 ADR。

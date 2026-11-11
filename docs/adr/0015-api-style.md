# ADR-0015 API 风格：REST + OpenAPI 3.1（utoipa 自动生成）

- 状态：已接受（2026-11-11）
- 关联需求：INT-01（API 优先）、AGL、PLUGIN

## 决策

- REST 资源命名 `/api/v1/{domain}/{resource}`（domain=模块）；
- OpenAPI 3.1 由 utoipa 注解生成，`/api/docs`（Scalar UI）随版本发布；CI 用 oasdiff 校验破坏性变更；
- 统一错误体 `{ code, message, trace_id, details? }`（code 为稳定枚举，与 DomainError 一一对应——见选型：thiserror）；
- 分页 `?page=&size=`（≤500）、过滤 `?filter[attr]=op:value`、排序 `?sort=`；
- 大结果集（BOM 展开/导出/审计）统一 `application/x-ndjson` 流式；
- 鉴权：Bearer JWT（Web/小程序）+ OAuth2 client_credentials（集成方）+ PAT（CI/脚本）。

## 后果

- ✅ API 即文档，插件 SDK 与小程序共享 OpenAPI 生成客户端；
- ⚠️ utoipa 注解侵入 handler——通过自定义 extractor/响应包装宏减少样板。

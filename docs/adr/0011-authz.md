# ADR-0011 认证与授权：JWT 会话 + casbin RBAC + 自研 ABAC

- 状态：已接受（2026-11-11）
- 关联需求：ORG-03/04/05、PRD ORG 验收（权限判断 <5ms、变更 3s 生效）

## 背景

功能权限（菜单/按钮/API）与数据权限（组织/项目/密级/分类/状态五维）是两类模型；纯 casbin 表达 ABAC 复杂维度性能不可控，纯自研 RBAC 又重复造轮子。

## 决策

- 认证：jsonwebtoken 短效 access（15min）+ 滚动 refresh（7d）；argon2id 密码；ldap3 对接 AD；钉钉/企微/飞书 OAuth 与微信 code2session 由 conn-im 换发统一 JWT。
- 授权：casbin-rs 承载 RBAC（role/perm/menu，策略持久化 PG，策略热加载）；**数据权限自研求值器**——五维规则编译为内存位图/谓词缓存，P95 <5ms；权限变更经 `config.changed` 事件 3s 内失效重算。
- 审计：AuditSink 追加式 + 哈希链，独立审计卷。

## 后果

- ✅ 性能可控（ABAC 求值纯内存）；casbin 只干它擅长的；
- ⚠️ casbin 模型与自研规则的边界要写清（功能=casbin，数据=自研），防止实施团队混用。

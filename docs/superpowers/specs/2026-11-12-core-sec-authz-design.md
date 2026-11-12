# core-sec 授权（RBAC + 密级）增量设计（短设计）

- 日期：2026-11-12；状态：用户"依次推进"指令下按既定顺序执行（授权 → 生命周期）
- 依据：ADR-0011（casbin RBAC + 自研 ABAC）、需求 ORG-01~05、终审遗留 M2/M5

## Intent

骨架已有认证（谁能登录），本增量补**授权**（能做什么）：角色-权限矩阵（casbin 求值）、用户管理（admin 建号/授权）、数据密级过滤（ABAC 第一维），并顺手清 M2/M5。

## Goals / Non-Goals

**Goals**
1. 三个内置角色：`admin`（全部+用户管理）、`editor`（docs:read+write）、`viewer`（docs:read）；docs 上传/写 = `docs:write`，读 = `docs:read`
2. admin API：建用户、授权/回收角色、列出角色（仅 `sec:manage`）
3. 密级：`sec_user.max_secret_level`（0 公开/1 内部/2 秘密/3 机密）；列表按 `secret_level <= max` 过滤（系统列 Le 路径），详情越级 → 404（不泄露存在性）
4. 清 M2（auth 路由 16KB 体上限）与 M5（ensure_admin ON CONFLICT）

**Non-Goals**：项目/分类两维数据权限、菜单级权限、密级位图优化（P95 <5ms 是规模化目标，当前量级 HashSet 足够）、审计报表、用户禁用 API（本轮仅建号/授权）。

**Anti-Goals**：绕过 casbin 自写策略求值（ADR-0011 已定 casbin）；在网关做数据权限（必须收口在服务层查询条件）。

## Approaches

- **A（选定）casbin 作求值器 + 自有 PG 表作策略源**：sec_role/sec_role_perm/sec_user_role 三表，启动与变更时重建 Enformer（arc-swap 整体替换，与 TypeRegistry 同模式）。符合 ADR-0011；策略可审计。
- B casbin-sqlx-adapter 直连托管：少写仓储但依赖 casbin 适配器版本兼容，且与"策略变更 3s 生效"的自建路径耦合差。
- C 纯 HashSet 自写：最快但违背 ADR-0011。

## 关键语义

- 权限串 `obj:act`；docs 路由映射 read=GET 系列、write=POST
- 403 = 已认证但无权限（MDL-SEC-FORBIDDEN）；404 = 对象存在但越密级（防存在性泄露）
- 角色变更 → 重建 enforcer（进程内即时生效，满足"3s"）

## 留给实现者

- casbin 模型串与 Enforcer 装载细节；测试矩阵（角色×操作×期望）

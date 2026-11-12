# 授权（RBAC + 密级）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，用户既定）。checkbox 步骤。

**Goal:** docs 读写按角色授权（admin/editor/viewer），密级数据过滤，admin 用户管理 API；清 M2/M5。

**Architecture:** 策略存自有 PG 三表 → 启动/变更重建 casbin Enforcer（arc-swap）→ 服务层 `enforce` + 查询条件注入密级过滤。

**Tech Stack:** casbin 2（runtime-tokio）+ 既有栈。

**Spec:** docs/superpowers/specs/2026-11-12-core-sec-authz-design.md

## Global Constraints

- 新错误码：`MDL-SEC-FORBIDDEN`(403)、`MDL-SEC-USER-EXISTS`(409)
- 密级 0..=3（公开/内部/秘密/机密）；越级 detail → 404；list → 条件过滤
- casbin 模型：`g(r.sub,p.sub) && r.obj==p.obj && r.act==p.act`；策略行 `p, role, docs, read|write|manage(sec)`
- 请求体上限：auth 路由 16KB（M2）；docs 维持 max_upload

## Review Focus

1. **viewer 写入 → 403**（非 401）→ e2e 钉
2. **越密级 detail → 404 且 list 不出现** → 集成钉
3. **回收角色后立即失效**（下次请求 403，无需重启）→ e2e 钉
4. **非 admin 调用户管理 → 403** → e2e 钉
5. **建重名用户 → 409 MDL-SEC-USER-EXISTS** → 集成钉

---

### Task 1: core-sec 授权域（表/casbin/服务方法）

**Files:** Create `crates/core-sec/migrations/0005_sec_rbac.sql`、`src/rbac.rs`；Modify `src/service.rs`（enforce/create_user/grant/revoke/list_roles/max_secret_level 注入）、`crates/core-model/src/error.rs`（FORBIDDEN/USER-EXISTS）

**Interfaces:**
- `SecService::enforce(&self, user: Uuid, obj: &str, act: &str) -> Result<bool>`
- `SecService::create_user(&self, username, password, display_name, roles: &[&str], max_secret_level: i16) -> Result<Uuid>`
- `SecService::grant(&self, username, role: &str) / revoke(...)` → 内部重建 enforcer
- `SecService::max_secret_level_of(&self, user: Uuid) -> Result<i16>`
- `pub fn reload_policies(&self, pool) -> Result<()>`（arc-swap Enforcer）
- 种子（迁移）：admin/editor/viewer 三角色 + 权限行；admin 角色绑定到 sec_user.username='admin' 在 ensure_admin 内完成

- [ ] Step1 失败测试 `tests/rbac_it.rs`：enforce 矩阵（admin: docs read/write+sec manage；editor: r+w；viewer: r）；create_user 重复 → MDL-SEC-USER-EXISTS；grant/revoke 后 enforce 立即翻转；max_secret_level 注入查询过滤（配合 mod-doc list 用 Op::Le secret_level）
- [ ] Step2 RED；Step3 实现；Step4 GREEN；Step5 Commit

### Task 2: api-gateway 权限层 + sec 管理路由 + M2

**Files:** Modify `src/middleware.rs`（+`require_perm(obj,act)` 工厂，403 语义）、`src/routes/auth.rs` 旁新增 `src/routes/sec_users.rs`、`src/routes/mod.rs`（docs 组权限分层 + sec 组 + auth 16KB 限）、`src/state.rs`（schemas）

**Interfaces:**
- `require_perm(obj,&'static act)`：authn → enforce → 403 MDL-SEC-FORBIDDEN
- `POST /api/v1/sec/users` {username,password,display_name?,roles,max_secret_level}（sec:manage）
- `POST/DELETE /api/v1/sec/users/{username}/roles/{role}`（sec:manage）
- `GET /api/v1/sec/roles`（sec:manage）
- docs POST 组套 `require_perm("docs","write")`，GET 组套 `("docs","read")`
- auth 路由 `DefaultBodyLimit::max(16*1024)`（M2）

- [ ] Step1 实现（行为钉在 Task3 e2e/Task1 集成）
- [ ] Step2 既有测试不回归；Commit

### Task 3: e2e + 清 M5

**Files:** Modify `crates/plm-server/tests/{common,e2e}`；`crates/core-sec/src/service.rs`（M5 ON CONFLICT）

**Interfaces:**
- common：`create_user_as_admin(token, username, password, roles)` 助手；viewer/editor 登录助手
- 新 e2e：`viewer_cannot_write_403`（Review Focus #1）、`revoke_role_immediate_403`（#3）、`non_admin_cannot_manage_403`（#4）、`secret_level_filtered_and_404`（#2，配 admin 建密级 2 对象 + max_secret_level=0 的 viewer）

- [ ] Step1 新增+改造（RED 确认 403/过滤缺失）；Step2 GREEN；Step3 M5：ensure_admin 改 `INSERT ... ON CONFLICT DO NOTHING` + 回读 created；Step4 Commit

### Task 4: 门禁 + README + smoke + 终审 + 合并

- [ ] Step1 fmt/clippy/全测试；Step2 README 授权段；Step3 真机 smoke（admin 建号→editor 登录→上传 201→viewer 上传 403）；Step4 Commit；Step5 终审→修复轮→合并 main

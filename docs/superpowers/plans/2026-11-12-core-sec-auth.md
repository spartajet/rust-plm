# core-sec 最小鉴权 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 骨架加上"谁能用"——登录（argon2id）→ JWT（access 15min + refresh 7d）→ `/api/v1/docs*` 要求 Bearer；只做认证，RBAC/ABAC 下一增量。

**Architecture:** 新 crate `core-sec`（用户仓储 + JWT + 密码哈希，独立于 core-model）；api-gateway 加 `require_auth` 中间件与 `/api/v1/auth/*` 路由；plm-server 装配 + 首启 ensure_admin。

**Tech Stack:** jsonwebtoken(HS256) / argon2 / sqlx / PG18。复用既有：TEST_DATABASE_URL 唯一库方案、统一错误体、迁移链模式。

**Spec:** 短设计已获用户批准（会话 2026-11-12"继续"）；约束来源 ADR-0011、需求 §7.3。

## Global Constraints

- 错误码新增：`MDL-SEC-UNAUTHORIZED`(401)/`MDL-SEC-BAD-CREDENTIALS`(401)；既有错误体结构不变
- JWT：HS256；access exp 15min、refresh exp 7d；claims 含 `typ` 字段区分（refresh 冒充 access 必须拒绝）
- JWT 密钥：`PLM_JWT_SECRET` 优先；缺省读/建 `data_dir/jwt.key`（重启不失效）；都不行则随机生成并 WARN
- 内置 admin：启动时 `ensure_admin`——**仅当 sec_user 表无 admin 时创建**；密码取 `PLM_ADMIN_PASSWORD`，缺省随机 24 位并打印一次到日志
- 鉴权豁免路由：`/healthz`、`/api/docs`、`/api/docs/openapi.json`、`/api/v1/auth/*`
- 迁移链续接：0004（core-sec，版本号全局唯一约定）
- 测试隔离：沿用 TEST_DATABASE_URL 唯一库（库名前缀 sec_/cst_）

## Review Focus

1. **refresh 冒充 access**：typ=refresh 的 token 打 docs API 必须 401 → Task2 e2e 钉
2. **过期 access**：exp 已过 → 401 → Task1 单测钉（构造 exp=-1 的 token）
3. **JWT 密钥跨重启稳定性**：无 PLM_JWT_SECRET 时 jwt.key 文件复用 → Task1 单测钉（同 dir 两次实例化密钥一致）
4. **admin 不可被登录覆盖**：ensure_admin 在 admin 已存在时不得重置密码 → Task1 集成钉
5. **Authorization 头畸形**（缺 Bearer 前缀/空）→ 401 → Task2 e2e 钉

## File Structure

```
crates/core-sec/
  migrations/0004_sec_users.sql
  src/{lib,claims,service,error?→复用core-model}.rs
  tests/{auth_it.rs}
crates/api-gateway/src/routes/{auth.rs(新)}  routes/mod.rs(挂载)  middleware.rs(新)
crates/plm-server/src/main.rs(装配)  tests/{common,e2e}(改造)
crates/core-model/src/error.rs(SEC 变体)
```

---

### Task 1: core-sec crate（claims/jwt/argon2/用户仓储 + admin 种子）

**Files:** Create `crates/core-sec/{Cargo.toml, migrations/0004_sec_users.sql, src/lib.rs, src/claims.rs, src/service.rs, tests/auth_it.rs}`；Modify `crates/core-model/src/error.rs`（SEC 错误码）；根 Cargo.toml 加 jsonwebtoken/argon2/base64? （jsonwebtoken 自带 base64）

**Interfaces:**
- Produces（Task2/3 依赖，精确签名）:
  - `core_sec::migrator() -> sqlx::migrate::Migrator`（ignore_missing，同前例）
  - `core_sec::Claims { sub: Uuid, username: String, typ: String, exp: i64 }`（serde Serialize/Deserialize）
  - `core_sec::SecService::new(pool: PgPool, jwt_secret: String) -> Self`（Arc 可克隆语义由调用方包）
  - `async ensure_admin(&self, password: Option<String>) -> Result<AdminBootInfo>`（AdminBootInfo { created: bool, printed_password: Option<String> }）
  - `async login(&self, username: &str, password: &str) -> Result<TokenPair>`；`async refresh(&self, refresh: &str) -> Result<TokenPair>`
  - `fn parse_access(&self, token: &str) -> Result<Claims>`（typ!=access 或过期 → MDL-SEC-UNAUTHORIZED）
  - `TokenPair { access_token: String, refresh_token: String, expires_in: i64 }`
- 复用：`core_model::DomainError` 新增 `SecUnauthorized`/`SecBadCredentials`（MDL-SEC-UNAUTHORIZED/MDL-SEC-BAD-CREDENTIALS，均 401）

- [ ] Step1 失败测试先行：`tests/auth_it.rs`——① jwt_roundtrip_and_expiry：login→parse_access 命中 sub/username；手工签 exp=-1 token → 401 码；② refresh_token_cannot_become_access：parse_access(refresh) → 401；refresh(refresh) → 新对且旧 refresh 仍可用（骨架不吊销）；③ ensure_admin_idempotent：首建 created=true + 指定密码可登录；二次调用 created=false 且旧密码仍可登录（Review Focus #4）；④ wrong_password_rejected → MDL-SEC-BAD-CREDENTIALS；⑤ jwt_key_file_persist：无 env 时同 data_dir 两次 SecService 密钥一致（签出的 access 互通）（Review Focus #3）
- [ ] Step2 运行验证 RED（编译失败：core-sec 不存在）
- [ ] Step3 实现：0004 建表（username unique/citext? 用普通 text + lower 索引即可）；argon2id 哈希（`PasswordHasher`）；jsonwebtoken encode/decode + validation(exp)；密钥策略（env → jwt.key → 随机+WARN）；ensure_admin 查无才插
- [ ] Step4 运行至 GREEN `TEST_DATABASE_URL=… cargo test -p core-sec`
- [ ] Step5 Commit `feat(core-sec): 登录/JWT/argon2 + admin 种子 + 5 测试`

### Task 2: api-gateway 鉴权中间件 + auth 路由

**Files:** Create `src/middleware.rs`、`src/routes/auth.rs`；Modify `src/state.rs`（AppState + `sec: Arc<SecService>`）、`src/routes/mod.rs`（豁免/保护分组）、`src/lib.rs`

**Interfaces:**
- Consumes: Task1 全部
- Produces（Task3/4 依赖）:
  - `AppState { …, sec: Arc<SecService> }`（新字段，Task4 common 同步）
  - `routes::build(state)`：`/api/v1/auth/login`(POST)/`/api/v1/auth/refresh`(POST) 豁免；`/api/v1/docs*` 套 `middleware::require_auth`
  - `require_auth`：解析 `Authorization: Bearer x` → `parse_access` → Extension 插入 `AuthUser { id: Uuid, username: String }`；失败/缺失/畸形 → 401 MDL-SEC-UNAUTHORIZED（Review Focus #1/#5）
  - 登录请求/响应 DTO（ToSchema+Serialize）：`LoginReq{username,password}` / `RefreshReq{refresh_token}` / `TokenPairDto{access_token,refresh_token,expires_in}`

- [ ] Step1 失败测试：单测 auth 路由的 401 映射逻辑放 Task4 e2e（此处仅编译级）；本任务测试为 Task4 的 e2e 先行版——直接跳到 Task4 统一打（计划允许：中间件行为由 e2e 钉，见 Task4）
- [ ] Step2 实现
- [ ] Step3 `cargo test -p api-gateway`（既有 6 单测不回归）
- [ ] Step4 Commit `feat(api-gateway): Bearer 鉴权中间件 + auth 路由`

### Task 3: plm-server 装配（ensure_admin + 密钥策略）

**Files:** Modify `crates/plm-server/src/main.rs`、`tests/common/mod.rs`（AppState 新字段 + `login_helper`）

**Interfaces:**
- Consumes: core_sec::{SecService, migrator}
- Produces: 启动序列 迁移链(+core-sec) → SecService::new(密钥策略) → ensure_admin(cfg.admin_password) → AppState{sec}; Config 新字段 `admin_password: Option<String>`、`jwt_secret: Option<String>`

- [ ] Step1 实现（无独立测试：行为被 Task4 e2e 与 Task1 单测夹住——与骨架 main.rs 同策略，已在台账有先例裁定）
- [ ] Step2 `cargo check -p plm-server` + 既有 e2e 改造后全绿（见 Task4）
- [ ] Step3 Commit（与 Task4 合并提交）

### Task 4: e2e 改造 + 鉴权钉子

**Files:** Modify `crates/plm-server/tests/common/mod.rs`、`crates/plm-server/tests/e2e.rs`

**Interfaces:**
- Consumes: AppState 新字段；TestCtx::setup 内 ensure_admin("test-admin-pw") + `ctx.login() -> String`(access token)
- Produces: 全部既有 e2e 用例改走登录；新增三钉：
  1. `docs_require_bearer`：无 token → 401 MDL-SEC-UNAUTHORIZED（含畸形头：`Authorization: xxx` 无 Bearer）
  2. `refresh_token_cannot_access_docs`：用 refresh 当 access → 401（Review Focus #1）
  3. 既有 full_flow/duplicate/oversize/healthz/degraded 全部经登录后照旧绿

- [ ] Step1 改造+新增（改造本身即 RED：旧用例直打 docs → 401）
- [ ] Step2 运行 `TEST_DATABASE_URL=… cargo test -p plm-server` → 全绿
- [ ] Step3 Commit `test(e2e): 鉴权钉子 + 全链改造`

### Task 5: 门禁 + 文档 + 合并

- [ ] Step1 `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && TEST_DATABASE_URL=… cargo test --workspace`（全绿才续）
- [ ] Step2 README：鉴权段（登录示例 + PLM_ADMIN_PASSWORD/jwt.key 说明）
- [ ] Step3 真机 smoke：起服务 → 无 token 401 → 登录 → 带 token 上传/下载绿
- [ ] Step4 Commit `chore(delivery): 鉴权文档 + smoke`；终审(fresh reviewer) → 修复轮 → 合并 main（流程同骨架增量）

## Self-Review

- 覆盖：短设计五要素（登录/JWT/中间件/装配/admin 种子）↔ Task1-4；Review Focus 5 条 ↔ Task1②③④⑤ + Task4 钉1/2 ✓
- 类型一致：TokenPair/Claims/AppState.sec 在 Task1→2→4 签名一致 ✓
- 比例：计划 ~120 行 vs 预计 ~600 行代码 ✓

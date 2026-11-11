# MVP Walking Skeleton 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 端到端纵切骨架：Docker 一条命令起服务，HTTP 走通"建图纸对象→存文件→列表/详情/下载"，测试全绿。

**Architecture:** 单体模块化（ADR-0001）。`plm-server` bin 组装 → `api-gateway`（唯一持 Router）→ `mod-doc`（传输无关服务层）→ `core-model`（已有）/ `core-files`（内容寻址）。错误统一 `{code,message,trace_id}`（ADR-0015）。

**Tech Stack:** Rust stable / axum 0.8（`{param}` 路由）/ sqlx 0.8 + PostgreSQL 18 / tokio / sha2 / figment / utoipa 5 / GitHub Actions。

**Spec:** `docs/superpowers/specs/2026-11-11-mvp-walking-skeleton-design.md`（含 builder-check 修订：6 零件、异步 FileStore、单文件契约、obj_no 独立字段、CI 进 DoD）

## Global Constraints

- PG **18**（选型定案）；路由参数用 `{obj_no}`（axum 0.8 语法）
- 错误体 `{"code","message","trace_id"}`；错误码沿用 core-model MDL-* 表（新增 MDL-FIELD-REQUIRED / MDL-FILE / MDL-FILE-MULTI / MDL-FILE-EMPTY / MDL-FILE-TOO-LARGE）
- 页大小 ≤500（`MAX_PAGE_SIZE`）；未提升属性等值强制 `attrs @>`（QueryBuilder 既有规则，禁止 `->>`）
- 文件哈希：SHA-256 64-hex；落盘 `vol/{hash前2位}/{hash}`；**MVP 单文件契约**（每对象恰 1 个 `role='original'` 文件）
- 上传请求：`obj_no`（独立表单字段，必填）+ `attrs`（JSON 字符串，可选）+ `file`（单文件，必填）
- 集成测试数据隔离：**`TEST_DATABASE_URL` + 每测试唯一库名**（对 spec "testcontainers" 的已记录偏离：本地无 Docker daemon；CI 用 postgres:18 service container，隔离语义等价）
- 仓库单 main 分支直推（骨架期），每任务一提交

## Review Focus（spec 暗示但测试易漏的输入）

1. **建对象失败的孤儿文件**：先存文件后建对象；同 obj_no 二次上传 → 409 且文件库不新增卷文件（内容寻址去重兜底）→ Task 3 测试 2 钉住
2. **非 ASCII 文件名下载**：Content-Disposition 需 `filename*=UTF-8''` 百分号编码 → Task 5 e2e 钉住
3. **空文件 / 多文件 / 缺 obj_no**：分别 400（MDL-FILE-EMPTY / MDL-FILE-MULTI / MDL-FIELD-REQUIRED）→ Task 3 测试 3、Task 5 e2e 钉住
4. **list 携带未知属性过滤**：返回 MDL-ATTR-NOT-FOUND（测试钉 code 而非状态码；状态码归 gateway 统一映射）→ Task 4 钉住
5. **超大上传**：`DefaultBodyLimit(max_upload)` 生效，multipart 解析失败返回 4xx（不 panic、不落盘半文件）→ Task 5 e2e 钉住（1KB 限额实测）

## File Structure

```
crates/core-model/    （已有；本计划仅加 tests/query_compile.rs）
crates/core-files/    （已有，已提交：lib/error/local + tests/local.rs）
crates/mod-doc/
  migrations/0002_seed_drawing_type.sql   （已有，已提交）
  migrations/0003_doc_files.sql           （已有，已提交）
  src/{lib,dto,service}.rs
  tests/{common/mod.rs, service_it.rs}
crates/api-gateway/
  src/{lib,state,error,routes/mod,routes/docs,routes/infra}.rs
  src/scalar.html
crates/plm-server/
  src/main.rs
  tests/{common/mod.rs, e2e.rs}
Dockerfile  .dockerignore  docker-compose.yaml  .env.example
.github/workflows/ci.yml
README.md（追加快速开始节）
```

---

### Task 1: QueryBuilder 编译矩阵单测（core-model，纯函数）

**Files:**
- Create: `crates/core-model/tests/query_compile.rs`

**Interfaces:**
- Consumes: `core_model::{QueryBuilder, ObjectQuery, Cond, Op, Sort, Page, DomainError, TypeDef, AttrDef, DataType, BaseType}`（全部已存在，字段 pub）
- Produces: 矩阵回归网（后续任何 QueryBuilder 改动被此测试钉住）

- [ ] **Step 1: 写测试**（纯函数无 DB；手工构造 TypeDef：material=promoted/text、remark=非提升/text、weight=promoted/decimal；断言编译 SQL 片段与错误码）

用例清单：eq 提升列 → `p_material = $1::text`；eq 非提升 → `attrs @> $1::jsonb` 且 bind 为 `{"remark":…}`；in 非提升 → `@>` OR 串接；in 非提升 33 值 → `MDL-QRY-IN-TOO-MANY`；like 非提升 → `MDL-QRY-PROMOTE-REQUIRED`；isnull 非提升 → `NOT (attrs ? 'remark')`；系统列 state eq → `state = $1::text`；range 提升列 → `p_weight >= $1::numeric`；sort 非提升 → `MDL-QRY-PROMOTE-REQUIRED`；page.size=501 → `MDL-QRY-PAGE-TOO-LARGE`；默认排序含 `updated_at DESC`。

- [ ] **Step 2: 运行** `cargo test -p core-model --test query_compile`
  预期：**全 PASS**（对既有实现的特征化测试；若失败即暴露 QueryBuilder 真缺陷，先修再续）

- [ ] **Step 3: Commit** `git commit -m "test(core-model): QueryBuilder 编译矩阵特征化测试"`

### Task 2: mod-doc 服务层 + 集成测试

**Files:**
- Create: `crates/mod-doc/Cargo.toml`、`src/lib.rs`、`src/dto.rs`、`src/service.rs`
- Create: `crates/mod-doc/tests/common/mod.rs`、`crates/mod-doc/tests/service_it.rs`
- Modify: 根 `Cargo.toml`（如需 workspace 依赖补齐——当前应已齐）

**Interfaces:**
- Consumes: `core_model::{TypeRegistry, PgObjectStore, ObjectStore, CreateCmd, ObjectQuery, AttrMap, BizObject, DomainError, MIGRATOR}`、`core_files::{FileStore, LocalFileStore, FileError}`（签名见 core-model 详设 / core-files lib.rs）
- Produces（Task 3/5 依赖）:
  - `pub static MIGRATOR: sqlx::migrate::Migrator`（0002+0003）
  - `mod_doc::DocService::new(pool: PgPool, registry: Arc<TypeRegistry>, objects: Arc<PgObjectStore>, files: Arc<dyn FileStore>) -> Self`
  - `DocService::TYPE: &str = "drawing"`
  - `async upload(&self, obj_no: String, attrs: AttrMap, file_name: String, mime: Option<String>, reader: &mut (dyn AsyncRead + Send + Unpin)) -> Result<DocDetail>`
  - `async list(&self, q: &ObjectQuery) -> Result<(Vec<BizObject>, u64)>`
  - `async detail(&self, obj_no: &str) -> Result<DocDetail>`（默认 v1.0 + original 文件）
  - `async open_original(&self, obj_no: &str) -> Result<(String, Box<dyn AsyncRead + Send + Unpin>, i64)>`（name, stream, size）
  - `FileInfo { name: String, hash: String, size: i64, mime: Option<String> }`；`DocDetail { object: BizObject, file: Option<FileInfo> }`
  - 测试设施：`tests/common::TestCtx::setup(hint: &str)`（TEST_DATABASE_URL 建唯一库 `mdt_{hint}_{nanos}` → 跑双 MIGRATOR → TypeRegistry::load → LocalFileStore(临时目录) → DocService；`cleanup()` 删库删目录）

- [ ] **Step 1: 写集成测试**（`service_it.rs`，用例与断言）
  1. `upload_detail_download_roundtrip`：上传（obj_no=DWG-<sfx>，attrs={"name":"Bracket","material":"Q235"}，file bytes=b"%DWG-fake"）→ detail：object.obj_no 命中、file.name=="part.dwg"、file.size；open_original 读回 == bytes
  2. `duplicate_obj_no_conflicts_and_no_orphan`：同 obj_no 同内容二次上传 → `MDL-OBJ-NO-DUPLICATE`；断言卷目录文件数不变（Review Focus #1）
  3. `empty_file_rejected`：0 字节 → `MDL-FILE-EMPTY`
  4. `missing_required_attr`：缺 name → `MDL-ATTR-REQUIRED`
  5. `list_filters_by_material`：两对象 material=Q235 / AL6061，`filter[material]=Q235` → total==1 且命中
- [ ] **Step 2: 运行验证失败** `TEST_DATABASE_URL=postgres://postgres:root@localhost:5432/postgres cargo test -p mod-doc`
  预期：编译失败（DocService 不存在）
- [ ] **Step 3: 实现** dto/service/lib（upload 序：put→size==0 判空→create→file_object ON CONFLICT DO NOTHING→file_ref INSERT；detail 序：get v1.0→查 file_ref JOIN file_object）
- [ ] **Step 4: 运行至全 PASS**（同 Step 2 命令）
- [ ] **Step 5: Commit** `git commit -m "feat(mod-doc): DocService 上传/列表/详情/下载 + 5 集成测试"`

### Task 3: api-gateway（错误体/trace/路由/OpenAPI）

**Files:**
- Create: `crates/api-gateway/Cargo.toml`、`src/lib.rs`、`src/state.rs`、`src/error.rs`、`src/routes/mod.rs`、`src/routes/docs.rs`、`src/routes/infra.rs`、`src/scalar.html`
- Modify: 根 `Cargo.toml`（axum/utoipa/tokio-util/futures-util 已在 workspace deps）

**Interfaces:**
- Consumes: Task 2 全部；`axum::{Router, extract::{State, Extension, Query, Multipart, DefaultBodyLimit}, response::IntoResponse}`；`tokio_util::io::ReaderStream`
- Produces（Task 4 依赖）:
  - `AppState { registry: Arc<TypeRegistry>, service: Arc<DocService>, pool: PgPool }`（Clone）
  - `api_gateway::router(state: AppState) -> Router`
  - 路由：`GET /healthz`；`POST /api/v1/docs`（multipart→201）；`GET /api/v1/docs`（列表）；`GET /api/v1/docs/{obj_no}`；`GET /api/v1/docs/{obj_no}/file`（流式 + Content-Disposition 含 `filename*=UTF-8''`）；`GET /api/docs/openapi.json`（utoipa）；`GET /api/docs`（Scalar HTML，CDN）
  - `TraceId(String)` 中间件：每请求生成 uuid → Extension + 响应头 `X-Trace-Id`
  - `ApiError { code, message, status, trace_id } : IntoResponse`；`new(err: DomainError, trace: &str)`；错误码/状态码取自 DomainError
  - `DefaultBodyLimit::max(state.max_upload)`；上限入 AppState（`max_upload: u64`）
  - ListParams 解析：`filter[k]=v`→Cond Eq；`sort=-attr`→Sort desc；`page`/`size`
  - DTO（utoipa ToSchema）：`FileInfoDto{name,hash,size,mime}`、`DocResponse{object: Value, file: Option<FileInfoDto>}`、`ListResponse{items: Vec<Value>, total, page, size}`

- [ ] **Step 1: 写单测**（`src/error.rs` 内 `#[cfg(test)]`）：DomainError::ObjNoDuplicate → ApiError code=="MDL-OBJ-NO-DUPLICATE" && status==409；FileMulti → 400/"MDL-FILE-MULTI"
- [ ] **Step 2: 运行失败** `cargo test -p api-gateway`（编译失败）
- [ ] **Step 3: 实现**（multipart 流式：`StreamReader::new(field.map_err(io::Error::other))` → `Box::pin` 后按 `&mut (dyn AsyncRead+Send+Unpin)` 传 DocService；第二个 file 字段 → `DomainError::FileMulti`）
- [ ] **Step 4: 运行 PASS** `cargo test -p api-gateway`
- [ ] **Step 5: Commit** `git commit -m "feat(api-gateway): docs 路由/统一错误体/trace/OpenAPI"`

### Task 4: plm-server 装配 + e2e（oneshot 走真 DB + 真 router）

**Files:**
- Create: `crates/plm-server/Cargo.toml`、`src/main.rs`
- Create: `crates/plm-server/tests/common/mod.rs`、`crates/plm-server/tests/e2e.rs`
- Modify: 根 `Cargo.toml` 无（成员 glob 已含）

**Interfaces:**
- Consumes: `core_model::MIGRATOR`、`mod_doc::MIGRATOR`、`TypeRegistry::load/spawn_watcher`、`HookRegistry::new`、`PgObjectStore::new`、`LocalFileStore::new`、`DocService::new`、`api_gateway::{router, AppState}`
- Produces: `plm-server` 可执行（环境变量 `DATABASE_URL` 必填 / `PLM_DATA_DIR` 默认 `data` / `HTTP_PORT` 默认 8080 / `PLM_MAX_UPLOAD` 默认 209715200）；figment 读取（Env::raw 小写化 + Env::prefixed("PLM_")）

- [ ] **Step 1: 写 e2e**（`tests/e2e.rs`，oneshot 打真 router + 真 PG18 库，helper 与 mod-doc 同构、库名前缀 `plt_`）
  1. `healthz_ok`：200 且 body `status=="ok"`
  2. `full_flow`（spec DoD#2 全链）：POST multipart（obj_no+attrs+file）→ 201；GET `/api/v1/docs?filter[material]=Q235` → total==1；GET detail → 200；GET file → 字节相等 且 `Content-Disposition` 含原文件名（Review Focus #2：用中文名 `零件.dwg`）
  3. `duplicate_conflict`：同 obj_no 二次 POST → 409 且 body code=="MDL-OBJ-NO-DUPLICATE" 且 `trace_id` 非空
  4. `oversize_rejected`（Review Focus #5）：AppState.max_upload=1024 上传 4KB → 4xx 且无 201
- [ ] **Step 2: 运行失败** `TEST_DATABASE_URL=… cargo test -p plm-server`（main.rs 空壳）
- [ ] **Step 3: 实现 main**（序：tracing → Config → pool → 双 MIGRATOR → registry.load + spawn_watcher(5) → HookRegistry/PgObjectStore/LocalFileStore/DocService → router → TcpListener → axum::serve().with_graceful_shutdown(ctrl_c)；`Config{database_url,data_dir,http_port,max_upload}`）
- [ ] **Step 4: 运行 PASS**（同 Step 2）
- [ ] **Step 5: Commit** `git commit -m "feat(plm-server): 装配/迁移链/优雅停机 + 4 e2e"`

### Task 5: 交付物（Docker/CI/文档）+ 全量验证门禁

**Files:**
- Create: `Dockerfile`、`.dockerignore`、`docker-compose.yaml`、`.env.example`、`.github/workflows/ci.yml`
- Modify: `README.md`（追加"快速开始"节：compose 一条命令 + curl 示例）

**Interfaces:**
- Consumes: plm-server 二进制；CI 环境 `TEST_DATABASE_URL`（postgres:18 service container）
- Produces: spec DoD#1/#4/#5 交付物

- [ ] **Step 1: Dockerfile**（multi-stage：rust:slim build `cargo build --release -p plm-server` → debian:bookworm-slim + ca-certificates + curl（healthcheck 用）；ENTRYPOINT plm-server；ENV PLM_DATA_DIR=/data）
- [ ] **Step 2: docker-compose.yaml**（app + postgres:18-alpine；命名卷 `plm-pgdata`/`plm-files`；`depends_on: service_healthy`；app healthcheck `curl -f http://localhost:8080/healthz`；`restart: unless-stopped`；`.env` 两变量 HTTP_PORT/POSTGRES_PASSWORD）
- [ ] **Step 3: CI**（`.github/workflows/ci.yml`：fmt --check → clippy `-D warnings` → nextest（taiki-e/install-action）带 TEST_DATABASE_URL service container → docker build；push/PR 触发）
- [ ] **Step 4: 本地全量门禁**（真机逐条跑，任何一红即修）
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `TEST_DATABASE_URL=… cargo test --workspace`
- [ ] **Step 5: 真机 smoke**（DoD#1/#2 实测：本地 PG 建 `rust_plm_dev` 库 → 后台起 server（PLM_DATA_DIR=target/e2e-data HTTP_PORT=18080）→ curl：healthz 绿 → POST multipart（ASCII 名）→ 201 → filter 列表 total==1 → detail → file 下载字节一致 → 杀进程 → 删库删目录）
- [ ] **Step 6: README + .env.example + Commit** `git commit -m "chore(delivery): Docker/CI/README 快速开始"`

---

## Self-Review 结论

- **Spec 覆盖**：DoD 1↔Task4/5、2↔Task4 e2e+Task5 smoke、3↔Task2、4↔Task5、6 零件↔Task1-4（api-gateway/文件/种子/plm-server）+ 已提交 core-files；§4 接口↔Task2/3 Interfaces 块 ✓
- **步骤扫描**：每步单一动作；实现步只给签名与顺序约束，无正文转写 ✓
- **类型一致**：`open_original` 返回 `(String, Box<dyn AsyncRead+Send+Unpin>, i64)` 在 Task2/3 一致；`DocService::TYPE`、AppState.max_upload 在 Task3/4 一致 ✓
- **Review Focus**：5 条均有归属测试（Task2#2、Task3#4、Task4 e2e #2/#3/#4）✓
- **比例**：计划 ~200 行 vs 产出 ~1500 行代码，无转写 ✓

**已记录偏离**：spec §3 testcontainers → `TEST_DATABASE_URL` 唯一库方案（本地无 Docker daemon；CI service container 等价隔离）；spec §3 "rustls 关闭"→骨架纯 HTTP（ADR-0014 的 TLS 属后续增量，spec 已注明）。

# MVP Walking Skeleton 设计（纵切方案 A）

- 日期：2026-11-11
- 状态：待批准
- 上游文档：需求 v1.1（PRD）、概设 v0.1、技术选型 v1.0、ADR 0001–0019、core-model 详设
- Approaches：三案比较见文末，**已选 A（纵切走通骨架）**

## 1. Intent 与 Why

RustPLM 的卖点是"敏捷、高效"——7 个工作日基础包上线（PRD A1）。实现的第一步必须是**第 1–2 天就能跑起来的端到端骨架**：Docker 一条命令起服务、HTTP 能走通"建图纸对象 → 存文件 → 列表/详情/下载"，让 D-AC 验收、性能基线、CI、后续所有 crate 的装配点在最早时刻真实存在。纵切优先于横切（逐层做完再组装），代价是接受少量接口返工。

## 2. Goals / Non-Goals / Anti-Goals

**Goals（本骨架的 Definition of Done）**
1. `docker compose up -d` → 自动建库迁移 → `/healthz` 绿（D-AC 第 1/4 条的最小版）
2. curl 全链路：种子类型 `drawing` 存在 → `POST /api/v1/docs`（multipart：obj_no+attrs+单文件）→ `GET /api/v1/docs?filter=` 列表（走 QueryBuilder GIN 路径）→ `GET /api/v1/docs/{no}` 详情 → `GET /api/v1/docs/{no}/file` 下载（内容寻址 SHA-256 命中 + Content-Disposition 原名）
3. `cargo nextest run -p mod-doc` 集成测试绿（testcontainers 起 PG18）
4. README 快速上手（compose 一条命令 + curl 示例）
5. CI 最小流水线绿（GitHub Actions：fmt + clippy + nextest+testcontainers + docker build；**列入本增量**——默认决策，用户未表态时采用；remote 推送待用户创建 GitHub 仓库后接入，CI 文件先行入仓）

**Non-Goals（明确留到后续增量）**
鉴权与权限（core-sec）、版本/生命周期迁移（core-version）、事件与 IM 卡片（core-events/conn-im）、流程引擎、BOM、前端、桌面端、插件、AI。

**Anti-Goals**
为骨架方便而绕开既定架构（如：直连 SQL 绕过 ObjectStore、把文件存数据库、硬编码类型不走 TypeRegistry）——骨架可以少，但不能歪。

## 3. 范围裁剪（本骨架 = 6 个零件）

| 零件 | 内容 | 来源 |
|---|---|---|
| `bin/plm-server` | figment 读 DATABASE_URL/DATA_DIR/HTTP_PORT → sqlx migrate（多 crate 迁移链）→ 组装 Router → /healthz + /metrics 桩；**不持有任何 handler** | 装配规范概设 §2.1 R-D4 |
| `api-gateway`（最小版） | **唯一持有 axum Router 的 crate**（R-D4）：路由挂载、trace_id 中间件、DomainError→HTTP 错误映射（ADR-0015 错误体）、utoipa OpenAPI 聚合 + Scalar UI | 概设 R-D4；builder-check 修正（原 5 零件表遗漏） |
| `core-files`（最小版） | `FileStore` **异步 trait**（`put: AsyncRead 流 → hash` / `open(hash) -> Box<dyn AsyncRead + Send + Unpin>`，dyn 兼容——对概设 §3.11 同步签名的修订，理由：axum multipart 流式落盘 + R-D5 阻塞隔离）；本地卷实现：SHA-256 内容寻址 `vol/ab/abc...`；**不做** GC/S3/分片续传/病毒扫描 | ADR-0012 裁剪 |
| `mod-doc`（最小版） | 传输无关的服务层（trait + 实现）：multipart 校验 → ObjectStore.create + FileStore.put + file_ref 落库；唯一预检/乐观锁复用 core-model；**不依赖 axum**（handler 在 api-gateway） | PRD DOC 最小集 |
| 迁移种子 | `0002_seed_types.sql`：drawing 类型 + 8 个属性；`0003_files.sql`：file_object(hash,size,mime,virus_status) + **file_ref(object_id,role,hash,name)**（name 列为 builder-check 增补：Content-Disposition 与 CAD 检入回显需要原始文件名） | AGL 临时替代 |
| `docker-compose.yaml` + Dockerfile + CI | app + postgres:18-alpine（选型定案 PG18），命名卷 pgdata/files，healthcheck，`.env` 两变量；GitHub Actions 最小流水线：fmt+clippy+nextest(testcontainers)+docker build | 附件 B §6.1 骨架版 |

## 4. 关键接口（骨架期契约）

```
GET  /healthz                       → {"status":"ok","meta_version":N}
POST /api/v1/docs                   → multipart/form-data：
                                       obj_no（独立表单字段，必填——编码系统后置前的显式编号）
                                       attrs（JSON 字符串，可选）
                                       file（单文件，必填；multipart 中出现多个 file 字段 → 400 MDL-FILE-MULTI）
                                     → 201 BizObject + files:[{name,hash,size,mime}]
GET  /api/v1/docs?filter[material]=AL6061&sort=obj_no&page=1&size=20
GET  /api/v1/docs/{obj_no}          → BizObject（默认 v1.0）
GET  /api/v1/docs/{obj_no}/file     → 200 流式，Content-Disposition: attachment; name 取 file_ref.name
GET  /api/docs                      → OpenAPI(utoipa) + Scalar UI
```

- **单文件契约（默认决策，用户未表态时采用；多文件需求出现时以 ?role= 选择器向后兼容扩展）**：MVP 期一个 drawing 对象恰持 1 个文件；多文件 + role 区分（原件/PDF/3D）随 core-files 完整版引入。
- 鉴权：骨架期全开放（仅限内网/compose 网络），core-sec 为下一增量——**这是唯一明知的债务**，D-AC 不受影响（单机内网部署语义不变）。
- 错误体统一 `{code,message,trace_id}`（ADR-0015），错误码沿用 core-model 的 MDL-* 表 + 新增 MDL-FILE-MULTI / MDL-FILE-EMPTY。

## 5. 后续增量顺序（骨架跑通后）

core-sec（鉴权+RBAC 最小）→ core-version + core-events（发布态/Outbox）→ workflow 三档模板 → core-bom（V-2 算法产品化）→ 前端工作台（Vue3+EP）→ conn-im 卡片审批 → mod-part。每步都以"骨架多跑通一段 PRD 场景"为验收。

## 6. Approaches（已比较）

- **A 纵切骨架（选定）**：最早可运行/可测/可装配，接受接口小返工——与"敏捷"卖点同构。
- B 分层横切：每 crate 一次到位，但 2–3 周无可运行物，集成风险后置。
- C 场景 S1 垂直切（含桌面端）：演示故事好但桌面端进场过早，地基未稳。

## 7. 留给实现者（writing-plans 的输入）

- bin 装配顺序与优雅停机细节；multipart 大文件流式边界（>100MB 拒绝的阈值配置）；
- `/metrics` 桩的具体指标集（先 traces 计数 + PG pool 深度）；
- 集成测试的种子数据复用方式（直接跑 0001–0003 迁移 + 最小 fixture）。

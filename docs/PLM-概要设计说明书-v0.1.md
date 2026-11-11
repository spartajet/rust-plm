# RustPLM 概要设计说明书 v0.1

| 项 | 内容 |
|---|---|
| 依据 | 《PLM-产品需求文档 v1.1》《PLM-技术架构建议》、ADR 0001–0010 |
| 范围 | 服务端 crate 架构、核心域接口契约、数据模型总览、关键流程时序、验证计划；**不含**详细设计（表字段全集/算法细节）与客户端 UI 设计 |
| 状态 | 初稿（随 ADR 更新而更新） |

---

## 1. 设计目标回顾（来自非功能需求）

| 目标 | 设计落点（本文章节） |
|---|---|
| 2C4G/50 并发，BOM 展开 <1s | §3.3 闭包表引擎、§8 线程模型 |
| 配置热生效（A2） | §3.1 元数据引擎 arc-swap、§3.4 流程热更新 |
| 单二进制 + Docker 一键 | ADR-0001/0014，§8 |
| 插件安全隔离（PLG-02/03） | §3.8 插件运行时 |
| AI 可断电（AI-04） | §3.9 网关降级链 |
| API 优先（INT-01） | §6 网关约定 |

## 2. 总体架构

### 2.1 crate 拓扑与依赖规则

```
                        ┌────────────────────────┐
                        │  bin: plm-server       │  组装/启动/配置
                        └───────────┬────────────┘
                                    │
                        ┌───────────▼────────────┐
                        │  api-gateway           │  路由/中间件链/OpenAPI
                        └───────────┬────────────┘
        ┌───────────────┬───────────┼───────────────┬──────────────┐
        ▼               ▼           ▼               ▼              ▼
   mod-part        mod-doc      mod-ec          mod-pm      mod-agl(配置中心)
   mod-capp        mod-vis      mod-fa          mod-ai(场景编排)
        └───────────────┴───────────┴───────────────┴──────────────┘
                                    │ 仅依赖 core-* / svc-*
        ┌───────────────────────────▼───────────────────────────────┐
        │ core-model  core-version  core-workflow  core-bom          │
        │ core-search core-sec      core-events    core-files        │
        │ core-plugins                                             │
        └───────────────────────────┬───────────────────────────────┘
                                    │
        ┌───────────────┬───────────┴───────────┬──────────────┐
        ▼               ▼                       ▼              ▼
   conn-im(钉钉/企微/ conn-fa(嘉立创/          svc-ai         svc-convert
   飞书/小程序)       怡合达/米思米)           (AI网关)       (PDF/OCR/轻量化,进程池)
        └───────────────┴───────────────────────┴──────────────┘
```

**依赖规则（CI 强制，cargo-deny bans + 自定义检查）**：

| 规则 | 说明 |
|---|---|
| R-D1 | `mod-*` 只准依赖 `core-*`，**禁止依赖其他 `mod-*`**——跨模块协作只经 core-events 事件或 core-model 对象关系 |
| R-D2 | `core-*` 之间依赖呈 DAG：`core-model` 最底层；`version/workflow/bom/search/files/sec` 依赖 model；`events` 被 core 依赖（接口下沉为 crate 内最小 trait） |
| R-D3 | `conn-*` 只依赖 `core-plugins`（Connector trait）与 `core-model` 的 DTO，**不接触数据库连接**，数据落库由 core 层完成 |
| R-D4 | `api-gateway` 是唯一持有 axum Router 的 crate；所有 HTTP 处理器薄封装到 mod/core 的 trait 调用 |
| R-D5 | 禁止 core-* 依赖 tokio 之外的异步运行时；阻塞重活（转换/OCR/插件编译）一律走 `spawn_blocking`/子进程 |

### 2.2 运行视图（进程内线程模型，详见 §8）

- tokio 多线程 runtime（worker = CPU 核数，上限 8）；
- 独立任务组：Outbox 投递循环、索引消费循环、闭包表维护队列、定时任务（cron 表驱动）、IM 长回调监听；
- svc-convert 子进程池（LibreOffice/ODA/PaddleOCR），进程隔离崩溃；队列 + 背压。

## 3. 核心域接口契约（crate 级）

> 约定：所有 trait 以 `#[async_trait]` 或原生 AFIT 暴露；返回 `Result<T, DomainError>`；领域对象经 DTO 边界（serde）跨 crate 传输，不泄漏内部实体。

### 3.1 core-model 元数据引擎（ADR-0002）

职责：类型/属性/关系元数据管理（含在线 DDL 演进）、业务对象统一 CRUD、编号集成点、查询构建。

```rust
pub struct BizObjectId { pub type_code: SmolStr, pub no: String, pub major: u16, pub minor: u16 }

pub trait TypeRegistry: Send + Sync {          // arc-swap 热缓存，改配置 3s 内生效
  fn type_def(&self, code: &str) -> Arc<TypeDef>;
  fn reload(&self) -> Result<()>;              // 由 mod-agl 配置变更事件触发
}

pub trait ObjectStore {
  async fn create(&self, ty: &str, attrs: AttrMap, files: Vec<FileRef>) -> Result<BizObject>;
  async fn get(&self, id: &BizObjectId, level: ReadLevel) -> Result<BizObject>;
  async fn update(&self, id: &BizObjectId, patch: AttrPatch) -> Result<BizObject>;  // 仅 working 态
  async fn transition(&self, id: &BizObjectId, event: LifecycleEvent) -> Result<BizObject>; // 委托 core-version
  async fn relate(&self, rel: RelCode, from: &BizObjectId, to: &BizObjectId, attrs: AttrMap) -> Result<RelInst>;
  async fn query(&self, q: ObjectQuery) -> Result<Page<BizObject>>;  // QueryBuilder → 动态 SQL
}
```

关键机制：
- **AttrPatch 幂等更新**：attrs JSONB merge + 乐观锁（attrs_rev）；冲突返回 409 语义错误；
- **钩子总线**：`save_before/submit_before/release_before` 钩子位（AGL-07 内置规则 + 插件规则同槽执行，超时熔断）；
- **在线 DDL**：TypeDef 变更 diff → 迁移计划（加列/回填/双写期/清理 JSONB 字段）→ 执行并记录配置版本（可回滚，AGL-06）。

### 3.2 core-version 版本与生命周期

职责：状态机解释、版本策略、发布快照。

```rust
pub trait LifecycleMachine {
  fn allowed(&self, state: State, event: LifecycleEvent) -> Option<State>;  // 表驱动，纯函数
  async fn snapshot(&self, id: &BizObjectId) -> Result<SnapshotId>;        // 评审/发布点不可变快照
}
pub enum LifecycleEvent { Submit, Approve, Reject, Release, Obsolete, Revise /*经EC升版*/ }
```

- 状态迁移规则来自 TypeDef（默认四态 + 可扩展态），**发布态对象 update 被 ObjectStore 直接拒绝**，升版只能经 `Revise`（由 mod-ec 的 ECO 驱动，PRD LCS-02/EC-05）。

### 3.3 core-bom BOM 引擎（ADR-0003）

```rust
pub enum ExpandMode { OneLevel, ToDepth(u8), Full, LeafOnly }
pub trait BomEngine {
  async fn build_from(&self, parent: &BizObjectId, edges: Vec<EdgeInput>) -> Result<BomVersionId>;
  async fn expand(&self, parent: &BizObjectId, mode: ExpandMode, f: &Filter)
      -> Result<impl Stream<Item = BomLine>>;       // NDJSON 流式，10万行 <1s
  async fn where_used(&self, child: &BizObjectId, depth: Option<u8>) -> Result<Vec<UsagePath>>;
  async fn diff(&self, a: &BizObjectId, b: &BizObjectId) -> Result<BomDiff>;
  async fn derive(&self, src: &BizObjectId, view: BomView) -> Result<BizObjectId>; // EBOM→PBOM/MBOM
}
```

- 闭包表维护 = BOM 版本提交事务内的**图 diff 增量任务**（ Inserts/Deletes 计划生成 → 批量写 bom_path）；
- 循环检测：`build_from` 前对 parent 的祖先链查 `bom_path` 反向可达；
- 多视图（EBOM/PBOM/MBOM）= 同一物料域上的独立版本化 BOM 对象 + `derive` 记录视图映射行（MBOM 行可回溯 EBOM 源行，BOM-04）。

### 3.4 core-workflow 工作流引擎（ADR-0004）

```rust
pub trait WorkflowEngine {
  async fn start(&self, template_key: &str, biz: &BizObjectId, vars: Vars) -> Result<InstanceId>;
  async fn advance(&self, task: TaskId, action: TaskAction /*Approve{comment,sign}/Reject{to}/Delegate*/) -> Result<Flow>;
  async fn migrate_instances(&self, from_tpl: u32, to_tpl: u32, mapping: NodeMapping) -> Result<Report>;  // WF-07
}
```

- 流转 = 单事务：推进令牌 + 写 wf_task + 触发 LifecycleEvent + 写 Outbox（任务到达/完成事件）——目标 <200ms（WF-08）；
- 办理人解析器 `AssigneeResolver`：角色/岗位/发起人上级/表单取人/脚本，按序回退；
- 与 OA/IM 解耦：任务到达事件由 conn-im 消费渲染卡片（§7.5），卡片回调经 api-gateway 验签后调 `advance`。

### 3.5 core-search 检索（ADR-0005）

```rust
pub trait SearchService {
  async fn search(&self, q: &SearchReq /*text|nl->dsl*/, filter: &PermFilter) -> Result<SearchPage>;
  fn suggest(&self, prefix: &str) -> Vec<String>;
}
```

- **PermFilter 位图**：角色→可见对象集预计算 bitset（roaring），查询结果按位图过滤（防越权）；权限变更使位图失效重算（3s，PRD ORG 验收）；
- 中文分词 jieba-rs + 同义词（配置中心热更新）；NL→DSL 的 AI 通道由 mod-ai 调 svc-ai 生成候选 DSL，经 SearchService 校验后执行（AI-12）。

### 3.6 core-sec 安全

```rust
pub trait Identity { fn user(&self) -> &UserRef; fn roles(&self) -> &[RoleRef]; }
pub trait PolicyDecisionPoint {                        // PDP，全 API 出口统一调用
  async fn decide(&self, user: &Identity, action: Action, resource: &Resource) -> Result<Allow, Deny>;
}
```

- casbin 承载 RBAC（功能权限）；数据权限（组织/项目/密级/分类/状态五维）以 ABAC 表达式求值，**决策中间结果缓存 5s 内有效**（P95 <5ms，PRD ORG 验收）；
- 审计：AuditSink 追加写 + 哈希链（前条哈希入后条），独立审计卷持久化；
- 认证源：本地(argon2id) / LDAP / 钉钉·企微·飞书 OAuth / 微信小程序 code2session（conn-im 回调换发）→ 统一签发 JWT（短效 access + 滚动 refresh）。

### 3.7 core-events 事件总线（ADR-0007）

```rust
pub trait EventBus {                                   // 写侧
  async fn emit(&self, ev: DomainEvent) -> Result<()>; // 与业务同事务写 outbox
  fn subscribe(&self, topic: &str, h: HandlerId) -> SubId; // 进程内消费
}
```

- Topic 命名：`{对象}.{动作}`（`doc.released`、`eco.executed`、`task.arrived`、`config.changed`…）；
- 外投：WebhookRegistry（HMAC 签名、退避重试、死信）——ECN/发布通知/集成方共用；
- 消费幂等：handler 记 `event_id` 去重表。

### 3.8 core-plugins 插件运行时（ADR-0006）

```rust
pub trait PluginHost {
  async fn install(&self, pkg: PlmPkg) -> Result<PluginId>;   // 校验 manifest/签名/兼容区间 → AOT 预编译
  async fn enable/disable/uninstall(&self, id: PluginId) -> Result<()>;
  fn hot_swap(&self, id: PluginId) -> Result<()>;              // PluginSlot arc-swap 原子切换
  fn invoke(&self, id: PluginId, hook: HookPoint, ctx: HookCtx) -> Result<HookResult>; // 配额+熔断
}
pub trait Connector: Send + Sync {                            // 连接器统一契约（IM/FA/ERP 同构）
  fn manifest(&self) -> &'static ConnectorManifest;
  async fn test_connection(&self) -> Result<Health>;
}
```

- wit 宿主世界（`plm:Host`）暴露受控 API：objects.readonly / events.emit / http.fetch(scope) / logger——能力按 manifest 授权；
- 插件配置（密钥等）经 SecretVault（加密落盘）注入，不进插件内存明文日志。

### 3.9 svc-ai AI 网关（ADR-0010）

```rust
pub trait LlmProvider { async fn chat(&self, req: ChatReq) -> Result<ChatResp>; async fn embed(..)->..; }
pub struct SceneRouter;   // scene → provider 策略表（参数中心热更新）；失败链: 主→备→None(降级人工)
```

- mod-ai 场景编排（OCR 任务、NL 检索、RAG 问答、清洗建议）只依赖 SceneRouter 与 RagPipeline，**不感知具体厂商**；
- RagPipeline：切块（标题感知）→ embed（批量）→ 混合检索（tantivy BM25 + pgvector）→ 生成（强制携带来源锚点，无来源拒答）。

### 3.10 conn-im / conn-fa / conn-erp 连接器

- `ImAdapter`（附件 B §3.9）：通讯录同步、卡片发送、回调验签、SSO、`miniapp_auth`；三平台各一实现，输出统一 `ImEvent`；
- `CatalogConnector`：search/detail/order_link；FA 缓存（TTL）+ 限流；无 API 商城用目录导入适配器实现同接口（FA-08）；
- `ErpConnector`：物料/MBOM/工艺路线/ECN 四类下发 + 回执对账（INT-03）。

### 3.11 core-files 文件库

```rust
pub trait FileStore { async fn put(&self, stream: &mut dyn Read) -> Result<FileHash>; /*SHA-256 内容寻址*/
                      async fn open(&self, h: &FileHash, rng: Range) -> Result<impl Read>; }
```

- 本地分卷实现（默认）+ S3/MinIO 实现（可选）；引用计数 GC（夜间任务）；上传断点续传（分片索引表）。

## 4. 业务模块（mod-*）

| crate | 职责（对 PRD） | 依赖的核心域 | 关键挂接 |
|---|---|---|---|
| mod-part | 物料/分类/查重/优选（PART） | model, version, search, events | `save_before` 钩子做查重门禁；`part.released` → ERP 下发 |
| mod-doc | 图文档/检入检出/圈阅/分发（DOC） | model, files, version, workflow, events | `doc.released` → svc-convert 自动 PDF + 索引 |
| mod-ec | PR/ECR/ECO/ECN 闭环 + 断点（EC） | model, workflow, bom, events | `eco.executed` 联编联动（§7.3） |
| mod-pm | 轻项目/里程碑/阶段门（PM） | model, workflow, events | 交付物齐套检查（`save_before` 于评审任务） |
| mod-capp | PBOM/MBOM/工艺路线（CAPP） | bom, model | derive 视图 + 工序行属性 |
| mod-vis | 可视化任务编排（VIS） | files, events | 订阅 release 触发轻量化转换 |
| mod-fa | 选型中心/入库映射/下单链接（FA） | model, events, conn-fa | `fa.imported` → part 创建流程 |
| mod-ai | AI 场景编排（6.20.2 全部场景） | svc-ai, search, model | OCR 低置信队列（人工确认任务） |
| mod-agl | 配置中心/模板包/实施向导（AGL） | model, workflow, events | `config.changed` → TypeRegistry.reload + 位图失效 |

## 5. 数据模型总览（核心表）

> 详细列级定义在详细设计给出；此处列实体与关系。命名 `snake_case`，全部软删（`deleted_at`）+ 审计四列（created/at/by、updated/at/by）。

```
元数据域: obj_type ─┬─< obj_attr
                    ├─< rel_def            lifecycle_def ─< state_rule
                    └─< numbering_rule ─< code_segment
对象域:   biz_object ─┬─< biz_object_file >─ file_object(hash)
                    ├─< rel_inst >─ biz_object        （对象↔对象任意关系）
                    └─< obj_snapshot（评审/发布快照，不可变）
BOM 域:   bom_edge(parent,line_no,child,qty,...)   bom_path(ancestor,descendant,depth)
          bom_derive(src_view→dst_view 映射行)      bom_effectivity（日期/批次/序列）
流程域:   wf_template(version,bpmn)   wf_instance(template_ver,current_nodes)
          wf_task(instance,node,assignee,action,signature,deadline)
事件域:   event_outbox(topic,payload,status,retry)   webhook_subscription   event_dead
安全域:   org_unit / user / role / user_role_org / perm_rule(RBAC) / abac_rule / audit_log(哈希链)
          session / api_token
插件域:   plugin_pkg(manifest,hash,sig)   plugin_slot(state,config_ref)   secret_vault
AI 域:    ai_scene_route   ai_call_log   rag_chunk(embedding vector)   ocr_task
FA 域:    fa_item_cache(sku,mall,payload)   fa_import_log
集成域:   connector_config   integration_job(状态/重试/对账)
```

关键关系：
- `biz_object` 唯一键 (type_id, obj_no, major, minor)；`iteration` 仅 working 态累加；
- `rel_inst` 是通用关系表（装配关系则由 bom_edge 承载，不走 rel_inst，避免双源）；
- `event_outbox.partition by month`，已完结归档表；
- `audit_log` 只追加：`hash = sha256(prev_hash || row)`。

## 6. api-gateway 约定

- 中间件链（顺序固定）：`trace_id → authn → authz(PDP) → ratelimit → audit → handler`；
- 路由命名：`/api/v1/{domain}/{resource}`，域=模块名（parts/docs/boms/workflow/tasks/ecos/fa/ai/plugins…）；
- 错误响应统一体：`{ code, message, trace_id, details? }`（code 为稳定错误枚举，前端 i18n 映射）；
- 分页 `?page=&size=`（size≤500），过滤 `?filter[attr]=op:value`，排序 `?sort=attr,-updated_at`；
- 大结果集（bom 展开/导出/审计）一律 `Accept: application/x-ndjson` 流式；
- OpenAPI：utoipa 注解 → `/api/docs`（Scalar UI），CI 校验 breaking-change（oasdiff）。

## 7. 关键流程时序

### 7.1 图纸检入（DES → DOC）

```
CAD插件        desktop(Tauri)   api-gateway    mod-doc      core-files   core-model   core-events  core-search
  │ 批量选择图纸 │  │             │             │            │            │            │            │
  │────属性提取──▶│ 分片上传(断点) │             │            │            │            │            │
  │              │──────────────▶│─────────────│──put流────▶│ SHA256秒传  │            │            │
  │              │               │────────────▶│ create(doc, attrs, files)───▶│ 对象落库   │            │
  │              │               │             │            │            │──emit(doc.created)──▶│
  │              │               │◀─ndjson 结果（成功/冲突/校验失败 行级）│            │   (异步)索引  │
```

要点：属性映射在**服务端统一执行**（DES-02），插件只传原始属性包；同名同版冲突行返回 409 语义由插件端提示三态处理（DES-06）。

### 7.2 发布流程（WF 驱动 + 联动）

```
审批人(IM卡片/小程序)  conn-im    api-gateway   core-workflow      mod-doc        core-version    core-events
  │ 一键同意+签名        │            │             │                 │              │              │
  │────────────────────▶│ 验签回调    │             │                 │              │              │
  │                     │───────────▶│──advance()─▶│ 单事务: 推进令牌  │              │              │
  │                     │            │             │  +wf_task       │              │              │
  │                     │            │             │──终节点?────────▶│ release(doc) │              │
  │                     │            │             │                 │─────────────▶│ State→Released│
  │                     │            │             │                 │              │──emit(doc.released)──▶
  │                     │            │             │                 │              │   ├─▶ svc-convert: 转PDF+水印
  │                     │            │             │                 │              │   ├─▶ core-search: 索引发布版
  │                     │            │             │                 │              │   └─▶ conn-im: ECN/发布通知卡片
```

要点：advance 是唯一写入口（Web/小程序/卡片回调同路径）——保证任一端办理后其余端待办自动消除（场景 S6 验收）。

### 7.3 ECO 联编执行（EC-05）

```
执行人           mod-ec            core-bom        core-model        core-workflow
  │ 勾选执行清单    │                   │                │                 │
  │───────────────▶│ 对每个受影响对象:   │                │                 │
  │                │  Revise() 预升版+锁定──────────────▶│ 新working版本    │
  │                │  修改BOM: build_from(新版本 edges)──▶│ 图diff→闭包增量  │
  │                │ 执行清单逐项打钩 ──▶ 全部完成?       │                 │
  │                │  └─ start("release-bundle", 批量对象)─────────────────▶│ 打包发布流程
  │                │ eco.executed 事件 → ECN 生成+下游通知(ERP/MES/IM)      │
```

### 7.4 插件热加载（PLG-03）

```
管理员        mod-agl/api    core-plugins                PluginSlot(A)   PluginSlot(B新)
  │ 上传.plmpkg  │               │                            │              │
  │────────────▶│ 校验签名/manifest/兼容区间 → AOT预编译(wasmtime)            │
  │             │──────────────▶│────────── instantiate ───────────────────▶│ 实例化于新slot
  │             │               │ 健康探活(空载调用) → arc-swap 全局注册表原子切换
  │             │               │──── 在途调用自然排空(A引用计数归零后 drop) ──│
  │             │               │ 失败? 回滚swap + 告警(保留A)                │
```

### 7.5 小程序/IM 卡片审批回调

```
用户端(小程序/卡片)   平台(微信/钉钉/企微)    api-gateway            conn-im         core-workflow
  │ 点击同意           │                       │                      │                │
  │──────────────────▶│ 加密回调(code/action)  │                      │                │
  │                   │─────────────────────▶│ 验签中间件             │                │
  │                   │                       │──miniapp_auth/callback─▶│ code→会话     │
  │                   │                       │◀─JWT────────────────│                │
  │                   │                       │──advance(task)───────────────────────▶│ (同7.2主路径)
  │                   │◀─卡片更新(待办消除)────│──update_card()──────▶│                │
```

## 8. 部署与运行视图

- 部署形态/镜像/compose 见附件 B §6.1（ADR-0014），此处补进程内视图：

| 运行单元 | 并发模型 | 资源上限 |
|---|---|---|
| HTTP 服务（axum） | tokio workers = min(核数, 8) | 连接池 pg = CPU×4 |
| Outbox 投递循环 | 单任务 + 批量拉取（500/批） | 慢消费者告警 |
| 索引消费循环 | 单写者（tantivy writer 单线程模型） | 背压：队列 >10k 暂缓拉取 |
| 闭包表维护 | 事务内同步 + 大图后台队列 | 单图 >5 万边转后台 |
| svc-convert | 独立进程池（2~4 进程） | 进程 OOM/超时自动重启 |
| 插件调用 | wasmtime store 池（每插件独立） | fuels/内存/超时（PLG-02） |

- 停机语义：SIGTERM → 拒新请求 → 排空在途事务与投递批 → 退出（≤30s）；升级滚动时配合健康检查（§7 各循环均幂等可续）。

## 9. 设计收尾验证计划（对应 ADR-0002/0003，PRD 第 8 章"设计流程约定"）

| # | 原型 | 验收标准 | 产出物 |
|---|---|---|---|
| V-1 | 元数据引擎最小原型：TypeDef/attrs JSONB/在线加列/查询 | ① 新类型+20 属性免重启可 CRUD；② 提升列在线 DDL 锁 <1s；③ 100 万行 JSONB GIN vs 提升列筛选 P95 对比数据；④ 动态查询测试矩阵跑通 | `spikes/metadata-spike/` + 报告（决定默认提升阈值策略） |
| V-2 | BOM 闭包表原型：构建/增量维护/展开/where-used | ① 随机 10 万行 BOM 构建+闭包生成 <60s；② 全量展开 <1s（P95）；③ 改 1 个子件再提交，闭包增量维护 <2s；④ 展开结果与递归 CTE 对拍 100% 一致；⑤ 循环引用正确拒绝 | `spikes/bom-spike/` + criterion 基准报告 |

时间盒：每个 1 周，超时即记录风险并回概设调整（如 V-1 不达标 → 默认策略改为"常用筛选属性一律提升列"）。

### 9.1 执行结果（2026-11-11，V-1 4/4、V-2 5/5 全部通过，详见 [spikes/REPORT.md](../spikes/REPORT.md)）

| 原型 | 关键实测 | 对设计的修正 |
|---|---|---|
| V-1（PG 18.6，100 万行） | 在线 DDL 2.4–3.7ms、并发写期间 ALTER 写延迟仅 1ms；GIN 等值 P95 0.95ms ≈ 提升列 1.15ms；`->>` 慢写法 202ms；回填 100 万行 61s | ① 默认策略确认为"JSONB GIN 起步，仅排序/范围/高基数列提升"；② 回填必须实现为后台分批任务；③ QueryBuilder 强制编译为 `@>` 写法（杜绝慢路径） |
| V-2（10 万边/深 10/DAG，闭包 84.6 万行） | 构建 2.2s；全量展开 P95 **42.4ms**（余量 23×）；增量维护（替换 2000 边子树）**143ms**；两次对拍递归 CTE 100% 一致；循环拒绝正确 | ADR-0003 无条件成立；闭包存储放大约 8.5×（按深度规划分区）；**新增工程约束：连接池会话路由使 TEMP 表不可跨调用使用——会话亲和操作需独占连接**（写入 §3.1 检入检出与详细设计） |

> 结论：§3.1/§3.3 的核心设计假设全部经实测验证，进入详细设计阶段无性能障碍。基准复现：`cargo run -r -p metadata-spike` / `-p bom-spike`（`DATABASE_URL` 指向空库）。

## 10. 需求追溯（模块 → PRD）

| PRD 模块 | 概设落点 |
|---|---|
| ORG/COD | core-sec + mod-agl（编码规则→core-model numbering） |
| PART/DOC | mod-part / mod-doc + core-model/files/version |
| BOM/LCS | core-bom / core-version |
| WF/EC | core-workflow / mod-ec（§7.3） |
| DES | clients/design-plugins + api-gateway 检入链路（§7.1） |
| PM/CAPP/VIS | mod-pm / mod-capp / mod-vis |
| RPT | core-search + mod-agl 报表模板 |
| COL/INT(OA/IM/小程序) | conn-im（§7.5）+ core-events |
| FA/AI/PLUGIN | mod-fa + conn-fa / mod-ai + svc-ai / core-plugins（§7.4） |
| AGL/SYS | mod-agl（config.changed 热生效链）/ api-gateway 审计 + 部署视图 §8 |

---

*概设 v0.1 完（2026-11-11 更新：V-1/V-2 原型验证全部通过并回填 §9.1，基准代码见 `spikes/`）。后续：ADR 0011–0015 随详细设计补齐。*

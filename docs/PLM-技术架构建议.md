# PLM 技术架构建议（附件 B）——Rust 实现

> 服务于《RustPLM 产品需求文档 v1.0》。本文档回答"怎么用 Rust 把需求和卖点工程化落地"。

---

## 1. 架构总览

```
┌──────────────────────────────────────────────────────────────────────────┐
│  客户端层                                                                  │
│  Web 工作台(Vue3+TS) │ 钉钉/企微/飞书(卡片审批+H5轻应用+机器人)              │
│  微信/钉钉小程序(原生移动端: 审批/搜索/预览/扫码找料)                          │
│  设计工具集成端(Tauri: 机械CAD/电气ECAD/电子EDA 插件) │ 第三方(Open API/Webhook) │
└──────────────────────────────────────────────────────────────────────────┘
                HTTPS / REST + SSE  │  IM 开放平台回调(webhook 入站)
┌──────────────────────────────────────────────────────────────────────────┐
│  应用层：单二进制 plm-server（tokio 异步，模块化 crate）                      │
│  ┌─────────┬─────────┬─────────┬────────┬─────────┬─────────┬──────────┐ │
│  │ core-   │ core-   │ core-   │ core-  │ core-   │ core-   │ api-     │ │
│  │ model   │ version │ workflow│ bom    │ search  │ im/     │ gateway  │ │
│  │ (元数据  │ (生命周期 │ (流程    │ (图式   │ (tantivy│ conn-*  │ (鉴权/   │ │
│  │  引擎)  │  状态机) │  引擎)   │  BOM)  │  +向量) │ (OA/IM/  │ 审计)    │ │
│  │         │         │         │        │         │ FA/ERP) │          │ │
│  └─────────┴─────────┴─────────┴────────┴─────────┴─────────┴──────────┘ │
│  平台服务：事件总线(Outbox Webhook) │ 定时任务 │ 转换管道(PDF/OCR/轻量化)      │
│  ★ core-plugins：插件运行时(WASM/动态库沙箱, 热加载, 能力权限)                │
│  ★ svc-ai：AI 网关(LLM 抽象/场景路由/脱敏/RAG/用量治理)                      │
└──────────────────────────────────────────────────────────────────────────┘
        │                     │                       │            │
┌───────────────┐  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────┐
│ PostgreSQL    │  │ 文件库(内容寻址)   │  │ 对象存储S3/MinIO  │  │ LLM 端点       │
│ (业务+pgvector│  │ 本地分卷 + SHA256 │  │ (可选/冷数据)      │  │ 云端API 或      │
│  +索引元数据)  │  └──────────────────┘  └──────────────────┘  │ 本地Ollama    │
└───────────────┘                                              └──────────────┘
```

**关键决策**：
- **单体优先（Modular Monolith）**：PLM 事务一致性强、调用链深，微服务只会放大复杂度；以 crate 边界保持模块化，未来需要时再拆（Rust 单体扩展到数百并发绰绰有余）；插件是单体上唯一的"受控外挂"。
- **元数据驱动**：`core-model` 实现"对象类型/属性/关系"运行期建模（对标 Aras），支撑 AGL 配置中心。
- **一切皆事件**：领域事件先落库（Outbox 表）再分发，保证 Webhook/消息/索引的一致性。
- **连接器即插件**：IM（钉钉/企微/飞书）、ERP、FA 商城连接器统一实现 `Connector` trait，官方连接器与第三方插件同一套运行时（PLUGIN 模块的工程化落点）。
- **AI 可插拔且可断电**：LLM 以 Provider trait 抽象，云端/本地双形态，AI 故障时全功能降级为人工流程。

## 2. Rust 技术选型

| 领域 | 选型 | 理由 |
|---|---|---|
| 异步运行时 | tokio | 事实标准 |
| Web 框架 | axum | tower 中间件生态、类型安全、社区活跃 |
| 数据库访问 | sqlx（编译期校验 SQL）+ 少量QueryBuilder | 元数据驱动需要动态 SQL，sqlx 两者兼备；不选重 ORM |
| 数据库 | PostgreSQL 15+ | 事务/JSONB（存自定义属性）/递归CTE（BOM 展开）/LISTEN-NOTIFY |
| 全文检索 | tantivy + jieba-rs | 内嵌式，免部署 ES；中文分词；千万级文档毫秒响应 |
| 权限 | casbin-rs + 自研 ABAC 求值器 | RBAC/ABAC 混合 |
| 认证 | jsonwebtoken、argon2、oauth2 | JWT + LDAP/钉钉/企微 OIDC |
| API 文档 | utoipa（OpenAPI 3.1 自动生成）+ Scalar UI | API 优先（INT-01） |
| 后台任务 | tokio 任务 + apalis（可选，Redis 队列）| 转换/导入/推送异步化 |
| 事件/Webhook | 自研 Outbox + 重试（指数退避）+ HMAC 签名 | 可靠投递 |
| 脚本钩子（AGL-07） | rhai（默认）/ wasmtime（高级） | 沙箱、超时熔断 |
| ★ 插件运行时（PLUGIN） | wasmtime（WASM 组件）+ libloading（动态库连接器）+ semver 兼容治理 | 沙箱热加载（PLG-02/03） |
| ★ LLM 接入（AI） | async-trait 的 `LlmProvider` 抽象：OpenAI 兼容协议（DeepSeek/Qwen 等）+ Ollama/vLLM（本地）；rig/llm crate 可选 | 双形态、可断电（AI-01/02） |
| ★ 向量/RAG | pgvector + tantivy 混合检索（BM25+向量），文档切块管道 | RAG 强制溯源（AI-03） |
| ★ OCR（AI-10） | 转换管道内置 PaddleOCR sidecar（进程池，其余同 svc-convert 隔离模式） | 标题栏字段级提取 ≥95% |
| ★ IM 开放平台 | 钉钉/企微/飞书 服务端 API（自研 `ImAdapter` 抽象，reqwest + 回调验签）；前端卡片 JSON 三平台适配器 | INT-04 三平台一套事件 |
| Excel | calamine（读）+ rust_xlsxwriter（写，流式） | 10 万行导出 <30s |
| PDF 处理 | lopdf/pdf-writer（生成）；DWG/Office 转换走外部转换器（LibreOffice headless / ODA）| 转换器独立进程池隔离 |
| 日志/追踪 | tracing + tracing-subscriber（JSON 结构化，OTLP 可选） | 接 ELK/Loki |
| 配置 | figment/config-rs + 配置库版本化于数据库 | 热生效（A2） |
| 桌面 CAD 端 | Tauri 2（Rust 核心）+ 设计工具原生 SDK 插件（中望 ZWSOFT SDK·机械首选；EPLAN API·电气首选；SolidWorks C# API；AutoCAD .NET/ObjectARX；立创EDA/AD 按各自开放接口） | 客户端 <20MB |
| CLI/运维 | clap | plm-cli：备份/迁移/导入/体检 |

## 3. 核心数据模型草案（PostgreSQL）

### 3.1 元数据层（AGL 的根基）

```sql
-- 对象类型（物料/图文档/BOM/变更单…全是类型）
CREATE TABLE obj_type (
  id UUID PRIMARY KEY, code TEXT UNIQUE NOT NULL,      -- 'part','document','eco'
  name TEXT NOT NULL, table_name TEXT NOT NULL,        -- 物理分表 or 统一大表
  base_type TEXT NOT NULL,                             -- part/document/change/project/...
  lifecycle_id UUID, numbering_rule_id UUID,
  config_version INT NOT NULL DEFAULT 1
);
-- 属性定义
CREATE TABLE obj_attr (
  id UUID PRIMARY KEY, type_id UUID REFERENCES obj_type,
  code TEXT NOT NULL, name TEXT NOT NULL,
  data_type TEXT NOT NULL,        -- text/int/decimal/date/bool/enum/ref/unit
  required BOOL, unique_flag BOOL, default_val JSONB,
  enum_dict UUID, ref_type UUID,  -- 引用型属性指向其他类型
  formula TEXT,                   -- 派生公式（体积=长*宽*高）
  config JSONB,                   -- 校验规则/显隐规则
  UNIQUE(type_id, code)
);
-- 关系定义 + 关系实例（装配/引用/关联/更改…）
CREATE TABLE rel_def (...); CREATE TABLE rel_inst (...);
```

**物理存储策略**：系统属性列 + 自定义属性 `attrs JSONB`（GIN 索引）。高频筛选属性可"提升为列"（在线加列，AGL-01 的 DDL 演进），兼顾灵活与性能。

### 3.2 对象与版本（一切业务对象的统一骨架）

```sql
CREATE TABLE biz_object (
  id UUID PRIMARY KEY,
  type_id UUID NOT NULL,
  obj_no TEXT NOT NULL,              -- 业务编号（编码系统赋码）
  major_ver INT NOT NULL DEFAULT 1,  -- 大版本
  minor_ver INT NOT NULL DEFAULT 0,  -- 小版本
  iteration INT NOT NULL DEFAULT 0,  -- 工作版次（保存次数）
  state TEXT NOT NULL,               -- working/review/released/obsolete(+扩展)
  secret_level SMALLINT DEFAULT 0,
  owner UUID, attrs JSONB,
  created_at TIMESTAMPTZ, created_by UUID, ...
  UNIQUE(type_id, obj_no, major_ver, minor_ver)  -- 版本唯一
);
-- 版本随发布/变更演进；iteration 只在 working 态累加；快照表按需归档
```

### 3.3 BOM 存储（10 万行展开 <1s 的关键）

采用**子件关系表 + 闭包表（Closure Table）**双结构：

```sql
CREATE TABLE bom_edge (            -- 单层父子关系（版本化）
  parent_id UUID, child_id UUID,   -- 指向 biz_object(带版本)
  line_no INT, qty NUMERIC(18,6), unit TEXT,
  attrs JSONB,                     -- 基准/损耗率/备注/替代关系
  PRIMARY KEY(parent_id, line_no)
);
CREATE TABLE bom_path (            -- 闭包表：祖先-后代-深度，物化全路径
  ancestor_id UUID, descendant_id UUID, depth INT,
  PRIMARY KEY(ancestor_id, descendant_id, depth)
);
```

- 单层展开/尾级汇总走 `bom_edge` 递归 CTE；全量展开与"用在哪儿（Where-used）"走 `bom_path` O(1) 查询；
- 闭包表由**后台增量维护**（BOM 版本提交时图 diff 后增量写入），读路径零递归——这是性能承诺 F1 的核心设计；
- 循环引用检测：写入前对 `bom_path` 做反向可达性检查，O(路径长) 完成。

### 3.4 工作流引擎（轻量状态机 + BPMN 子集）

```sql
CREATE TABLE wf_template (id, name, bpmn_xml, version, status);   -- 版本化，热发布
CREATE TABLE wf_instance (id, template_id, template_version, biz_object_id,
                          current_nodes JSONB, status, context JSONB);
CREATE TABLE wf_task (id, instance_id, node_id, assignee, delegate_from,
                      action, comment, signature, deadline, acted_at);
```

- 引擎 = 状态机解释器（节点类型：审批/会签/条件/并行/子流程/定时器）；
- 热更新（WF-07）：`wf_instance.template_version` 绑定启动时版本；"迁移策略"生成新旧节点映射后批量改写；
- 性能：流转为单事务（推进 + 落任务 + 发事件），目标 < 200ms。

### 3.5 文件库（内容寻址）

- 路径 = `vol/{sha256[0..2]}/{sha256}`，天然去重 + 秒传 + 完整性自校验；
- `file_object(hash, size, mime, virus_status)` + `file_ref(object_id, role, hash)` 引用计数，无人引用的孤儿文件定期 GC；
- 上传：分片断点 → 服务端汇总额外哈希校验；下载走授权签名 URL（权限审计挂下载事件）。

### 3.6 事件总线（Outbox 模式）

```sql
CREATE TABLE event_outbox (
  id BIGSERIAL, topic TEXT,        -- part.released / eco.executed ...
  payload JSONB, targets JSONB,    -- 订阅者(webhook id 列表)
  status SMALLINT, next_retry_at TIMESTAMPTZ, attempts INT
);
```
投递器批量拉取 → HMAC 签名 POST → 指数退避重试 → 死信表；同事务写业务与 Outbox，保证"发布必通知"（EC-06、INT-02）。

### 3.7 插件运行时（core-plugins，对应 6.18 PLUGIN）

```
manifest.plmpkg（声明: 扩展点/权限/兼容区间）
   │ 安装校验(semver×签名×权限清单) → 启用(热加载)
   ▼
┌─ 插件宿主 PluginHost ────────────────────────────────────┐
│ 后端插件: wasmtime 组件模型(Component Model)              │
│   host functions(白名单注入): plm.db.read / plm.objects   │
│   /plm.events / plm.http(声明式授权) / plm.logger        │
│   配额: CPU fuels / 内存 limiter / 超时熔断 / 错误隔离      │
│ 连接器插件: dyn trait Connector(abi_stable) .so/.dll      │
│   供 FA/IM/ERP 连接器与第三方复用同一注册表                  │
│ 前端插件: 静态资源包 + 受控 JS-API 桥(postMessage 白名单)   │
└──────────────────────────────────────────────────────────┘
```

- **热加载**：新版本插件编译单元在独立 `PluginSlot` 中预实例化 → 原子切换（arc-swap）→ 在途调用自然排空；失败自动回滚旧 slot（PLG-03）；
- **崩溃隔离**：WASM trap / 动态库 panic 被宿主捕获，仅该插件标错停用，主进程不受影响；插件专属错误/指标归集（PLG-07）；
- **扩展点注册表**：钩子（save/submit/release 前）、事件处理器、REST 路由挂载、定时任务、连接器、前端微件——统一在 manifest 声明，安装时注册；
- **官方插件即 dogfood**：FA/IM/ERP 连接器、DES 长尾 CAD 插件全部以插件形态随产品分发，保证 SDK 的 API 面在生产中被自己持续使用。

### 3.8 AI 网关（svc-ai，对应 6.20）

```rust
#[async_trait] trait LlmProvider {
  async fn chat(&self, req: ChatRequest) -> Result<ChatResponse>;
  async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}
// 实现: OpenAiCompatProvider(DeepSeek/Qwen/Kimi…) / OllamaProvider(本地)
// 场景路由 SceneRouter: ocr→本地小模型(快/省) 问答→按企业策略(云端|本地)
```

- **RAG 管道**：文档 → 抽取(转换管道) → 切块(标题/段落感知) → 向量(pgvector) + BM25(tantivy) 混合检索 → 生成时强制注入来源片段，回答携带对象/文件锚点链接（AI-03）；
- **脱敏中间件**：上云模式自动替换编号/客户名等敏感词（词表可配），调用全量审计（谁/何时/发了什么/答了什么）；
- **降级策略**：Provider 失败 → 备选 Provider → 无 AI 人工流程；场景级开关与 Token 配额（AI-04）；
- **评测基线**：OCR 字段准确率、NL→检索命中率、RAG 溯源有效率纳入回归测试集（防幻觉回归）。

### 3.9 连接器框架（conn-*，对应 INT-04 / 6.19 FA）

统一抽象（官方与第三方插件共用）：

```rust
trait ImAdapter {            // 钉钉/企微/飞书
  fn sync_contacts(); fn send_card(); fn register_callback();
  fn sso_url(); fn push_message(); fn write_calendar();
  fn miniapp_auth(code) -> Session;  // 微信 code2session / 钉钉小程序免登
}
trait CatalogConnector {     // FA 商城: 嘉立创/怡合达/米思米/…
  async fn search(&q) -> Vec<CatalogItem>;      // 型号/参数/价格/图
  async fn detail(&sku) -> CatalogDetail;       // 规格书/3D 模型链接
  async fn order_link(&items) -> Url;           // 下单直达/购物车
}
```

- IM 卡片：流程引擎输出**平台无关的卡片模型**（标题/字段/缩略图/动作按钮），三平台各一个渲染器（钉钉卡片/企微模板卡/飞书 message card），审批动作经回调统一验签入 WF 引擎；
- 小程序（INT-04b）：微信/钉钉小程序壳复用同一套移动 API；登录走 `ImAdapter::miniapp_auth`——微信 code2session / 钉钉免登 → 换发 PLM 会话（JWT），与 Web/桌面端账号体系统一；小程序内仅预览不下源文件；V2.0 供应商门户（COL-06）即微信小程序形态；
- FA 缓存：商品详情本地缓存（TTL 可配）+ 限流退避；无 API 商城实现 `CatalogConnector` 的"目录导入适配器"（Excel/链接/OCR 三源）；
- 密钥保管：连接器凭据进加密保险库（可选 age/SQLCipher 加密落盘），外部调用全审计（FA-09）。

## 4. API 风格示例（INT-01）

```
GET  /api/v1/objects/{type}/{no}?ver=A.2          # 取对象（含属性/状态/引用）
PUT  /api/v1/objects/{type}/{no}/attrs            # 改属性（working 态）
POST /api/v1/parts                                 # 新建物料（触发编码+查重门禁）
POST /api/v1/objects/{id}/checkout | checkin      # 检出/检入
POST /api/v1/boms/{parent}/expand?mode=leaf       # BOM 展开（流式 NDJSON）
POST /api/v1/workflows/{key}/start                # 发起流程
POST /api/v1/tasks/{id}/approve  {comment, signature}
POST /api/v1/changes/eco/{id}/execute             # ECO 执行（联编联动）
GET  /api/v1/search?q=...&types=part,document     # 统一检索（权限过滤）
POST /api/v1/webhooks                             # 注册事件订阅
# —— AI（6.20）/ FA（6.19）/ 插件（6.18）——
POST /api/v1/ai/ask  {q:"去年贴片机Z轴用的导轨型号"} # NL 检索/RAG 问答（答案带来源）
POST /api/v1/ai/ocr/dwg  {batch_id}               # 图纸标题栏 OCR 任务
GET  /api/v1/fa/search?q=怡合达+UCP205            # FA 商城聚合检索
POST /api/v1/fa/import  {sku, mall}               # 选型一键入库
GET  /api/v1/plugins  │ POST /api/v1/plugins/install  {pkg}
```
- OpenAPI 由 utoipa 注解自动生成，随版本发布到 `/api/docs`；
- 大结果集统一 **NDJSON 流式**响应（BOM 展开/报表导出），前端边收边渲染——体验承诺 F4 的支撑。

## 5. Cargo Workspace 结构

```
rust-plm/
├── Cargo.toml                 # workspace
├── crates/
│   ├── plm-server/            # bin：axum 组装、启动、路由
│   ├── core-model/            # 元数据引擎（类型/属性/关系/在线DDL）
│   ├── core-version/          # 版本/生命周期状态机/快照
│   ├── core-workflow/         # 流程引擎
│   ├── core-bom/              # BOM 图引擎（edge+closure，图diff）
│   ├── core-search/           # tantivy 索引与查询
│   ├── core-sec/              # 认证/授权(RBAC+ABAC)/审计
│   ├── core-events/           # Outbox 事件总线/Webhook
│   ├── core-plugins/          # ★ 插件运行时（wasmtime 宿主/扩展点/热加载）
│   ├── conn-im/               # ★ 钉钉/企微/飞书适配器（ImAdapter 三实现）
│   ├── conn-fa/               # ★ FA 商城连接器（嘉立创/怡合达/目录导入兜底）
│   ├── conn-erp/              # ERP 连接器（云星辰/精斗云/T+/U9C/星空/SAP）
│   ├── svc-ai/                # ★ AI 网关（LlmProvider/场景路由/RAG/脱敏/计量）
│   ├── mod-part/  mod-doc/  mod-ec/  mod-pm/  mod-capp/  mod-vis/
│   ├── svc-convert/           # 转换管道（PDF/OCR/轻量化，进程池隔离）
│   └── plm-cli/               # 运维 CLI（备份/迁移/批量导入/体检/插件打包）
├── clients/
│   ├── desktop/               # Tauri 桌面端（统一设计工具客户端）
│   └── design-plugins/        # 机械CAD/电气ECAD/电子EDA 各工具插件
├── plugins/                   # ★ 官方插件源（连接器/小工具，dogfood SDK）
└── docs/
```

## 6. 部署与运维

| 项 | 方案 |
|---|---|
| 单机 | `plm-server` + PostgreSQL，systemd/Windows 服务；安装向导一键初始化 |
| 容器 | **Docker 一键部署（推荐）**：单文件 `docker-compose.yaml`（app + postgres:16 + minio 可选）+ 官方镜像；一条命令起栈；Helm Chart（K8s 双副本滚动）为高可用扩展 |
| 升级 | 替换二进制 / `docker compose pull && up -d`；sqlx migrate 启动自迁移；迁移前自动快照，失败自动回滚（镜像 tag 回退 + 迁移事务回滚） |
| 备份 | `plm-cli backup`：pg_dump + 文件卷一致性打包（增量）；恢复演练向导 |
| 监控 | /healthz /metrics（Prometheus 格式）；慢 SQL、慢接口 TOP 内置面板 |
| 安全基线 | TLS 终止于反代或内置 rustls；argon2id；审计哈希链；cargo audit/deny 进 CI |

### 6.1 Docker 一键部署包设计（对应主文档 F3 / 7.6 D-AC）

```yaml
# docker-compose.yaml（随版本发布，小微拿到手只改 .env 两个变量）
services:
  plm:                      # 应用：单容器内含 Web/API/转换管道/AI 网关
    image: rustplm/plm-server:${PLM_VERSION:-latest}
    ports: ["${HTTP_PORT:-8080}:8080"]
    environment: [DATABASE_URL, PLM_DATA_DIR=/data, PLM_AI_MODE=local|cloud|off]
    volumes: [plm-files:/data, plm-backup:/backup]
    depends_on: { postgres: { condition: service_healthy } }
    healthcheck: { test: ["CMD","/app/plm-cli","healthz"], interval: 30s }
    restart: unless-stopped        # 异常自愈（D-AC 第 6 条）
  postgres:
    image: postgres:16-alpine
    volumes: [plm-pgdata:/var/lib/postgresql/data]
    healthcheck: { test: ["CMD-SHELL","pg_isready -U plm"], interval: 10s }
volumes: { plm-pgdata: {}, plm-files: {}, plm-backup: {} }
```

- **镜像工程**：multi-stage 构建（cargo chef 缓存依赖层）→ Rust musl 静态编译（`x86_64/aarch64-unknown-linux-musl`）→ distroless/alpine 基底，**目标 < 150MB**；镜像签名（cosign）；多架构 manifest（amd64/arm64——树莓派/国产 ARM 服务器可跑）；
- **首启流程**：容器 entrypoint 检测空库 → 跑 sqlx migrate → 生成首次管理员引导 token → Web 向导（管理员/行业包/IM 绑定）；.env 仅 `PLM_VERSION`、`HTTP_PORT`（+ 可选对象存储/LLM 端点）；
- **数据边界**：三命名卷（数据库/文件库/备份）承载全部状态，容器可随时销毁重建（D-AC 第 3 条）；备份卷可外挂 NFS；
- **升级**：`pull && up -d` → 新容器启动自迁移 → 失败自动回退旧镜像并保留告警（配合 `plm-cli` 迁移预检）；
- **离线镜像包**：CI 产出 `rustplm-offline-<ver>.tar.gz`（docker save 全部镜像 + compose 文件 + 导入脚本 + 校验和），内网 `docker load` 即用；
- **国内网络适配**：提供镜像加速配置说明与离线包兜底（小微机房常无外网）。

## 7. 性能设计要点（对应 F1–F5）

1. **读路径缓存**：类型/权限/编码规则等元数据进程内缓存（arc-swap 热更新），权限判断 <5ms；
2. **BOM 闭包表**：全展开 O(结果集) 而非 O(递归)；并行遍历用 rayon 分片；
3. **检索**：tantivy 分片（按类型/租户）+ 中文 jieba 分词；权限过滤以 bitset 预计算；
4. **写路径**：高冲突操作（取号、检出）用数据库行锁 + 乐观并发（attrs 带 version）；
5. **大批量**：导入/导出/展开全部流式 + 背压（futures channel），内存恒定；
6. **压测门禁**：以 7.1 指标构建基准数据集（真实企业脱敏结构），k6/criterion 进 CI，回归超标即阻断合并。

## 8. 与需求文档的追溯

| 需求 | 架构落点 |
|---|---|
| AGL-01 在线建模仿数 | core-model + JSONB + 在线 DDL |
| A2 配置热生效 | arc-swap 配置快照 + 配置版本化表 |
| SMB-1 默认可用 | Starter 预置包 = AGL 模板 + 内置迁移向导 |
| WF-07 流程热更新 | 模板版本化 + 实例迁移器 |
| BOM-06 10 万行 <1s | bom_edge + bom_path 闭包表 |
| EC-05 联编联动 | core-events Outbox + 事务编排器 |
| F3 单二进制部署 | 单 bin + 内嵌静态资源（rust-embed）；Docker 一键部署见 §6.1 |
| INT-01 API 优先 | axum + utoipa，Web 与 API 同源 |
| INT-04 三平台 IM/OA | conn-im 统一 ImAdapter + 卡片模型多平台渲染 |
| PLG-02/03 插件沙箱/热加载 | core-plugins（wasmtime + PluginSlot 原子切换） |
| FA-01/02/05 商城直连 | conn-fa CatalogConnector + 详情缓存 + 下单链接 |
| AI-01/02/03 AI 网关/RAG/安全 | svc-ai（LlmProvider 双形态 + pgvector 混合检索 + 脱敏审计） |
| AI-10 图纸 OCR | svc-convert OCR sidecar（PaddleOCR 进程池） |
| DES-07 插件矩阵版本治理 | 客户端兼容性矩阵 + 插件市场分发（PLUGIN 形态） |

---

*附件 B 完。两份附件与主文档共同构成 RustPLM v1.1 需求基线（2026-11-11 随主文档 v1.1 同步更新：插件运行时、AI 网关、连接器框架三章新增）。*

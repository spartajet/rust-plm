# RustPLM

面向**制造业中小微企业**的敏捷高效 PLM——Rust 单二进制 + PostgreSQL，一台 2C4G 小服务器跑全公司。

> 文档体系：[需求 v1.1](docs/PLM-产品需求文档-v1.0.md) · **[部署文档](docs/部署文档.md)** · **[需求符合度评审](docs/review-需求符合度评审.md)** · [迁移注册表](docs/migrations.md) · [ADR×19](docs/adr/README.md) · [概要设计](docs/PLM-概要设计说明书-v0.1.md) · [技术选型 v1.0](docs/PLM-技术选型说明书-v1.0.md) · [Spike 验证](spikes/REPORT.md)

## 当前状态：MVP+ 主线纵切完成（28 个增量，详见 Roadmap）

已覆盖：图纸受控/认证/授权(ABAC 密级)/生命周期(三档+Revise 升版)/Webhook 外投/BOM 引擎+版本链(冻结快照+as-of)+历史反查/COD 自动编码/PART 物料主数据(分类树+查重 MVP)/IM 卡片审批(钉钉加签/企微/飞书)/前端工作台(看板/用户管理/角色显隐/中文)。

**演示环境**：`PLM_DEMO_SEED=1` 启动即一键生成非标自动化行业 100 人公司（十部门，统一口令 demo-pass-123）+36 图纸+48 物料+3 台设备 BOM+审批中/已发布/检出锁/关联全套演示数据（幂等可重跑）——演示剧本见 [docs/demo-guide.md](docs/demo-guide.md)。**仅演示环境，生产严禁开启**。

上传图纸（multipart：obj_no + attrs + 单文件）→ 列表（GIN 检索路径）→ 详情 → 流式下载（SHA-256 内容寻址 + 原名）。

```bash
# Docker 一键（推荐）
docker compose up -d
curl http://localhost:8080/healthz

# 登录（首启若未设 PLM_ADMIN_PASSWORD，初始密码打印在服务日志里，仅一次）
curl -X POST http://localhost:8080/api/v1/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"你的密码"}'
# → {"access_token":"…","refresh_token":"…","expires_in":900}

# 用户与角色（仅 admin）：内置 admin/editor/viewer
# editor=上传+读, viewer=只读; max_secret_level: 0公开/1内部/2秘密/3机密（越级 404/列表过滤）
curl -X POST http://localhost:8080/api/v1/sec/users   -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json'   -d '{"username":"zhang","password":"Zhang@2026","roles":["editor"],"max_secret_level":2}'

# 上传（需 Bearer；access 15 分钟，POST /api/v1/auth/refresh 轮换；editor 及以上）
curl -X POST http://localhost:8080/api/v1/docs \
  -H "Authorization: Bearer $TOKEN" \
  -F 'obj_no=DWG-001' \
  -F 'attrs={"name":"支架","material":"Q235"}' \
  -F 'file=@支架.dwg'

# Webhook 订阅（admin）：评审/发布事件自动 HMAC 签名外投（失败退避→死信分页）
# url 须 http(s)://、secret ≥8 字符、topics ∈ doc.released/revised/submitted/checked（增量13 强校验）
curl -X POST http://localhost:8080/api/v1/sec/webhooks   -H "Authorization: Bearer $ADMIN_TOKEN" -H 'Content-Type: application/json'   -d '{"url":"https://your-system.example/hook","topics":["doc.released"],"secret":"shared-hmac-key"}'
# 接收端验证：X-RustPLM-Signature = HMAC-SHA256(secret, body)
# 死信排查：GET /api/v1/sec/webhooks/dead?page=1&size=50（含 attempts/error）

# 生命周期：提交→审批→发布（released 不可变；发布落快照+doc.released 事件）
curl -X POST -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/submit
curl -X POST -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/api/v1/docs/DWG-001/approve
curl -X POST -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/api/v1/docs/DWG-001/obsolete

# 升版（增量8）：released → 下一 major 的 working（v1.0→v2.0）；旧版历史=发布快照链
curl -X POST -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/revise
# 版本历史：当前最新版 + 各次发布的不可变快照（含发布件哈希）；迁移注册表见 docs/migrations.md
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/versions

# IM 卡片审批（增量10）：评审进入群机器人推卡（钉钉加签/企微/飞书）；卡内动作经回调回流
# 服务端配置（环境变量）：
#   PLM_IM_PROVIDER=dingtalk|wecom|feishu   PLM_IM_WEBHOOK=https://oapi.dingtalk.com/robot/send?access_token=...
#   PLM_IM_SECRET=<机器人加签密钥>           PLM_IM_DOC_URL_TEMPLATE=http://plm.your.co/docs/{obj_no}
#   PLM_IM_USER_MAP={"im-ding-1":"zhangsan","im-wecom-2":"lisi"}   # IM id→PLM username
# 动作回调（IM 企业应用回调网关转发到此端点；X-IM-Sign = hex(HMAC-SHA256(PLM_IM_SECRET, body))）：
curl -X POST http://localhost:8080/api/v1/im/actions -H 'Content-Type: application/json'   -H 'X-IM-Sign: <hex>' -d '{"obj_no":"DWG-001","action":"approve","user":"im-ding-1"}'
# 卡片按钮→PLM 闭环需各平台企业应用回调（凭证联调后置）；机器人出站推送开箱即用

# 审计日志（增量18，PRD SYS-01）：全操作留痕（append-only）+ 多维检索
curl -g -H "Authorization: Bearer $ADMIN_TOKEN" 'http://localhost:8080/api/v1/audit?action=login.fail&page=1&size=50'

# 全局搜索（增量20，PRD RPT-01/03）：内嵌 tantivy+jieba 分词+CJK 双字兜底；聚类返回；ABAC 密级过滤
curl -g -H "Authorization: Bearer $TOKEN" 'http://localhost:8080/api/v1/search?q=%E8%BD%B4%E6%89%BF'      # q=轴承（聚类：图纸+物料）
curl -g -H "Authorization: Bearer $TOKEN" 'http://127.0.0.1:8080/api/v1/search?q=DWG-001'                # 纯 ASCII=编号精确
# 索引近实时：对账扫描器（PLM_SEARCH_SCAN_SECS 缺省 2s）+ 周期全量重建兜底迟提交；索引目录 data_dir/search

# 行业模板包+实施向导（增量24，PRD AGL-05/09）：配置导出/内置 Starter 包/导入冲突检测 + 7 天上线 Checklist
curl -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/api/v1/meta/template/export      # 导出全量类型+属性+分类
curl -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/api/v1/meta/template/starter     # 内置 Starter 基础包
curl -X POST -H "Authorization: Bearer $ADMIN_TOKEN" -H 'Content-Type: application/json'   http://localhost:8080/api/v1/meta/template/import   -d '{"template":<包JSON>,"mode":"dry_run"}'   # dry_run 预检冲突 / merge 应用
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/meta/impl-checklist             # 八步实施清单（auto 步实时判定）

# 防爆破+安全参数（增量23，PRD SYS-04/§7.3）：连续失败锁定（阈值/时长参数热生效）+密码最小长度策略
curl -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/api/v1/sec/params
curl -X PUT -H "Authorization: Bearer $ADMIN_TOKEN" -H 'Content-Type: application/json'   http://localhost:8080/api/v1/sec/params/lock_threshold -d '{"value":5}'   # 修改留痕 audit

# 待办中心（增量19，PRD WF-03）：待办/已办/我发起三盒 + 批量审批 + 意见必填开关
curl -g -H "Authorization: Bearer $TOKEN" 'http://localhost:8080/api/v1/todos?box=todo'   # box=todo|done|mine
# 审批可带意见（意见落审计 detail，作为已办数据源）
curl -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json'   http://localhost:8080/api/v1/docs/DWG-001/approve   -d '{"comment":"质量合格，准予发布"}'
# 批量批准（同流程多对象；逐项结果，单件失败不中断批次；上限 50）
curl -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json'   http://localhost:8080/api/v1/docs/batch-approve   -d '{"obj_nos":["DWG-001","DWG-002"],"comment":"批量放行"}'
# 意见必填开关（per 类型，热生效；开启后 approve/reject 缺意见 → 400 MDL-LCS-COMMENT-REQUIRED）
curl -X PUT -H "Authorization: Bearer $ADMIN_TOKEN" -H 'Content-Type: application/json'   http://localhost:8080/api/v1/meta/types/drawing/flow-tier   -d '{"require_comment":true}'

# 物料主数据（增量17，PRD PART）：分类树挂编码规则 / 新建自动赋码 / 查重提示
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/classes
curl -X POST http://localhost:8080/api/v1/parts -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json'   -d '{"class_code":"CLS-ZZ","attrs":{"name":"支座","material":"Q235"}}'
curl -g -H "Authorization: Bearer $TOKEN" 'http://localhost:8080/api/v1/parts/duplicates?name=支座'

# 自动编码（增量16，PRD COD）：类型配规则 → 上传 auto_code=1 空图号自动赋码
# 规则存 cm_type.config.cod_rule（如 {"cod_rule":{"prefix":"DWG-","date_fmt":"yyyyMMdd","seq_digits":4}}，热生效）
# 预览（不烧号）：GET /api/v1/meta/types/drawing/cod-preview?n=3
curl -X POST http://localhost:8080/api/v1/docs -H "Authorization: Bearer $TOKEN"   -F 'obj_no=' -F 'auto_code=1' -F 'attrs={"name":"新图"}' -F 'file=@新图.dwg'

# 列表（filter 等值过滤 / sort 排序 / page+size 分页）
curl -g -H "Authorization: Bearer $TOKEN" \
  'http://localhost:8080/api/v1/docs?filter[material]=Q235&sort=-updated_at'

# BOM：结构写入 / 展开（NDJSON）/ 反查（循环引用 409 拒绝）
# 富行读（增量21 BOM-01：编号/名称直读，前端结构编辑器数据源）+ 用量/分类汇总（BOM-07）+ CSV 导出
curl -g -H "Authorization: Bearer $TOKEN" 'http://localhost:8080/api/v1/boms/DWG-001/lines?major=1'   # ?major=N=发布快照行（只读）
curl -g -H "Authorization: Bearer $TOKEN" 'http://localhost:8080/api/v1/boms/DWG-001/summary'          # 叶子用量（多路径和）+自制/外购分类
curl -g -H "Authorization: Bearer $TOKEN" -o summary.csv 'http://localhost:8080/api/v1/boms/DWG-001/summary?format=csv'  # UTF-8 BOM 兼容 Excel
curl -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json'   http://localhost:8080/api/v1/boms/DWG-001/lines   -d '[{"line_no":1,"child":"<子对象id>","qty":2}]'
curl -g -H "Authorization: Bearer $TOKEN" 'http://localhost:8080/api/v1/boms/DWG-001/expand?mode=full'
curl -g -H "Authorization: Bearer $TOKEN" 'http://localhost:8080/api/v1/boms/<子对象id>/where-used'
# 历史反查（增量14）：该件曾被用进哪些已发布 BOM（按发布事件分组，回填对象号）
# live 反查对称语义（增量15）：越密级 ancestor 行级过滤 + ancestor_no 件号回填
curl -g -H "Authorization: Bearer $TOKEN" 'http://localhost:8080/api/v1/boms/<子对象id>/where-used?asof=1'

# BOM 版本链（增量9）：发布即冻结子树快照；?major=N 按发布时点 as-of 展开
curl -g -H "Authorization: Bearer $TOKEN" 'http://localhost:8080/api/v1/boms/DWG-001/expand?mode=full&major=1'

# 详情 / 下载 / API 文档（docs 与文件下载同样需要 Bearer）
# 在线预览（增量22 DOC-05）：MIME 白名单 inline 直开（pdf/png/jpeg/gif/webp；SVG 排除防 XSS；其余 415 走下载）
curl -o preview.pdf -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/preview

# 检入/检出（增量26 DOC-03）：独占编辑锁（checkout_hours 参数缺省 24h，到期惰性回收）
curl -X POST -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/checkout     # 检出（他人 revise 409 MDL-DOC-LOCKED）
curl -X POST -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/checkin     # 检入（仅持有者）
curl -X DELETE -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/api/v1/docs/DWG-001/lock # 强制回收（sec:manage）

# 对象关联（增量27 DOC-12）：文档↔物料双向追溯（obj_link 泛化表，未来 BOM/变更/项目同表扩展）
curl -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json'   http://localhost:8080/api/v1/links   -d '{"from_no":"DWG-001","to_no":"WL-ZZ-00001","kind":"doc_part"}'
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/links      # 文档挂的物料
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/parts/WL-ZZ-00001/links # 物料挂的文档

curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001
curl -o 支架.dwg -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/file
# openapi.json: /api/docs/openapi.json   Scalar UI: /api/docs（豁免鉴权）
```

## 前端工作台

```bash
cd frontend && npm install
# 浏览器直连 API（后端 CORS 已放开；本机 node 环回不稳时绕开 vite 代理）
VITE_API_BASE=http://127.0.0.1:18095 npx vite --port 5173 --host 127.0.0.1
# 打开 http://127.0.0.1:5173 → 登录 admin → 顶栏全局搜索(图纸+物料聚类)/详情在线预览(PDF/图片+平铺水印)/看板/待办中心(待办+已办+我发起+批量审批)/图纸列表(密级列)/详情(生命周期+BOM 版本切换+升版+版本历史)/用户管理(建号+角色授予/回收)/审计日志
# 界面按角色显隐（admin=全量 / editor=无用户管理+审批 / viewer=只读）；会话 401 自动 refresh 换发重放；中文界面
```

# 本地开发（无 Docker）

```bash
# 1) 准备 PG18（本机已有服务亦可），创建空库
# 2) 跑迁移 + 起服务
DATABASE_URL=postgres://postgres:root@localhost:5432/plm_dev \
PLM_DATA_DIR=./data HTTP_PORT=8080 cargo run -p plm-server

# 3) 全量测试（集成测试自建唯一临时库，跑完自动删）
TEST_DATABASE_URL=postgres://postgres:root@localhost:5432/postgres cargo test --workspace

# 4) API 契约变更后刷新 OpenAPI 快照基线（TQ-02 门禁；git diff 审阅契约面变更）
UPDATE_OPENAPI_SNAPSHOT=1 TEST_DATABASE_URL=postgres://postgres:root@localhost:5432/postgres cargo test -p plm-server --test e2e openapi_snapshot
```

配置项（环境变量，见 [.env.example](.env.example)）：`DATABASE_URL`（必填）、`PLM_DATA_DIR`（默认 `data`）、`HTTP_PORT`（默认 8080）、`PLM_MAX_UPLOAD`（默认 200MB）、`PLM_ADMIN_PASSWORD`（首启 admin 密码，缺省随机生成并打印一次）、`PLM_JWT_SECRET`（JWT 密钥，缺省 `data_dir/jwt.key` 文件复用；多实例必须相同）。

## 工程结构

```
crates/
  core-model/    元数据引擎：类型/属性运行期建模 + JSONB/提升列 + 动态查询编译
  core-files/    内容寻址文件库（SHA-256 去重秒传）
  mod-doc/       图文档服务层（上传/列表/详情/下载）
  api-gateway/   axum 路由/统一错误体 {code,message,trace_id}/OpenAPI
  plm-server/    装配入口（figment 配置 + 迁移链 + 优雅停机）
spikes/          V-1 元数据 / V-2 BOM 闭包表性能基准（CI 回归）
docs/adr/        架构决策记录
```

## Roadmap（版本规划见需求文档 §8）

骨架 ✓ → 认证 ✓ → 授权 ✓ → 生命周期 ✓ → Webhook 外投 ✓ → 三档流程 ✓ → BOM 引擎 ✓ → 前端工作台 ✓（登录/列表/上传/详情/生命周期/BOM）→ Revise 升版 ✓（released→下一 major；版本历史=发布快照链）→ BOM 版本链 ✓（发布冻结快照+?major= as-of 展开）→ IM 卡片审批 ✓（评审推卡钉钉/企微/飞书+HMAC 回调审批闭环）→ 前端工作台深化 ✓（看板 5 态计数/BOM 版本切换/strict checked 态按钮/密级列/用户管理）。

## 许可

AGPL-3.0 双许可（≤30 用户免费商用；企业版/闭源集成见 [ADR-0017](docs/adr/0017-dual-license.md)）。

# RustPLM

面向**制造业中小微企业**的敏捷高效 PLM——Rust 单二进制 + PostgreSQL，一台 2C4G 小服务器跑全公司。

> 文档体系：[需求 v1.1](docs/PLM-产品需求文档-v1.0.md) · [ADR×19](docs/adr/README.md) · [概要设计](docs/PLM-概要设计说明书-v0.1.md) · [技术选型 v1.0](docs/PLM-技术选型说明书-v1.0.md) · [Spike 验证](spikes/REPORT.md)

## 当前状态：MVP Walking Skeleton（图纸受控最小闭环）

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

# Webhook 订阅（admin）：发布事件自动 HMAC 签名外投（失败退避→死信）
curl -X POST http://localhost:8080/api/v1/sec/webhooks   -H "Authorization: Bearer $ADMIN_TOKEN" -H 'Content-Type: application/json'   -d '{"url":"https://your-system.example/hook","topics":["doc.released"],"secret":"shared-hmac-key"}'
# 接收端验证：X-RustPLM-Signature = HMAC-SHA256(secret, body)

# 生命周期：提交→审批→发布（released 不可变；发布落快照+doc.released 事件）
curl -X POST -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/submit
curl -X POST -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/api/v1/docs/DWG-001/approve
curl -X POST -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/api/v1/docs/DWG-001/obsolete

# 升版（增量8）：released → 下一 major 的 working（v1.0→v2.0）；旧版历史=发布快照链
curl -X POST -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/revise
# 版本历史：当前最新版 + 各次发布的不可变快照（含发布件哈希）
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/versions

# IM 卡片审批（增量10）：评审进入群机器人推卡（钉钉加签/企微/飞书）；卡内动作经回调回流
# 服务端配置（环境变量）：
#   PLM_IM_PROVIDER=dingtalk|wecom|feishu   PLM_IM_WEBHOOK=https://oapi.dingtalk.com/robot/send?access_token=...
#   PLM_IM_SECRET=<机器人加签密钥>           PLM_IM_DOC_URL_TEMPLATE=http://plm.your.co/docs/{obj_no}
#   PLM_IM_USER_MAP={"im-ding-1":"zhangsan","im-wecom-2":"lisi"}   # IM id→PLM username
# 动作回调（IM 企业应用回调网关转发到此端点；X-IM-Sign = hex(HMAC-SHA256(PLM_IM_SECRET, body))）：
curl -X POST http://localhost:8080/api/v1/im/actions -H 'Content-Type: application/json'   -H 'X-IM-Sign: <hex>' -d '{"obj_no":"DWG-001","action":"approve","user":"im-ding-1"}'
# 卡片按钮→PLM 闭环需各平台企业应用回调（凭证联调后置）；机器人出站推送开箱即用

# 列表（filter 等值过滤 / sort 排序 / page+size 分页）
curl -g -H "Authorization: Bearer $TOKEN" \
  'http://localhost:8080/api/v1/docs?filter[material]=Q235&sort=-updated_at'

# BOM：结构写入 / 展开（NDJSON）/ 反查（循环引用 409 拒绝）
curl -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json'   http://localhost:8080/api/v1/boms/DWG-001/lines   -d '[{"line_no":1,"child":"<子对象id>","qty":2}]'
curl -g -H "Authorization: Bearer $TOKEN" 'http://localhost:8080/api/v1/boms/DWG-001/expand?mode=full'
curl -g -H "Authorization: Bearer $TOKEN" 'http://localhost:8080/api/v1/boms/<子对象id>/where-used'

# BOM 版本链（增量9）：发布即冻结子树快照；?major=N 按发布时点 as-of 展开
curl -g -H "Authorization: Bearer $TOKEN" 'http://localhost:8080/api/v1/boms/DWG-001/expand?mode=full&major=1'

# 详情 / 下载 / API 文档（docs 与文件下载同样需要 Bearer）
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001
curl -o 支架.dwg -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/file
# openapi.json: /api/docs/openapi.json   Scalar UI: /api/docs（豁免鉴权）
```

## 前端工作台

```bash
cd frontend && npm install
# 浏览器直连 API（后端 CORS 已放开；本机 node 环回不稳时绕开 vite 代理）
VITE_API_BASE=http://127.0.0.1:18095 npx vite --port 5173 --host 127.0.0.1
# 打开 http://127.0.0.1:5173 → 登录 admin → 看板/图纸列表(密级列)/详情(生命周期+BOM 版本切换+升版+版本历史)/用户管理(建号+角色授予/回收)
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

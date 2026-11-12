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

# 列表（filter 等值过滤 / sort 排序 / page+size 分页）
curl -g -H "Authorization: Bearer $TOKEN" \
  'http://localhost:8080/api/v1/docs?filter[material]=Q235&sort=-updated_at'

# 详情 / 下载 / API 文档（docs 与文件下载同样需要 Bearer）
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001
curl -o 支架.dwg -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/docs/DWG-001/file
# openapi.json: /api/docs/openapi.json   Scalar UI: /api/docs（豁免鉴权）
```

## 本地开发（无 Docker）

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

骨架 ✓ → 认证 ✓ → 授权(RBAC+密级) ✓ → 版本/生命周期 + 事件（core-version/events）→ 三档流程 → BOM 引擎→ 三档流程 → BOM 引擎（闭包表已验证 10 万行展开 42ms）→ 前端工作台（Vue3+EP）→ 钉钉/企微/飞书卡片审批。

## 许可

AGPL-3.0 双许可（≤30 用户免费商用；企业版/闭源集成见 [ADR-0017](docs/adr/0017-dual-license.md)）。

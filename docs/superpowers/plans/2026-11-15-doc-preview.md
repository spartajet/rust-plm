# 增量22：DOC-05 在线预览（PDF/图片直开 + 前端水印起步）

> 需求评审 P0-A 第 5 项。PRD §6.4 DOC-05：PDF/图片在线流式预览（免插件）、CAD 转 PDF（V1.0 后置，ADR-0018）、预览水印（用户+时间+IP）。评审现状：**只有下载，无在线预览**。
> 分支：`feat/doc-preview`；无新迁移；无新 crate。

## 设计裁定

### D1 预览端点（服务端）
`GET /api/v1/docs/{obj_no}/preview`（docs:read；与 /file 同层）：
- **MIME 白名单 inline 直开**：application/pdf、image/png、image/jpeg、image/gif、image/webp；**其余 → 415**（DWG/octet-stream 等不支持预览——前端提示走下载；SVG 排除：可含 `<script>`，inline 有存储型 XSS 面）。
- 响应头：`Content-Disposition: inline; filename*=UTF-8''…` + `X-Content-Type-Options: nosniff`（防 MIME 嗅探）+ `Content-Length`；流式复用 open_original（与 /file 同管线）。
- ABAC：父件越级 404（与 detail/file 同语义）；audit_mw 自动 http 行。
- 未上传文件的对象 → 404（open_original Err 路径）。

### D2 水印（前端 overlay 起步）
- 预览容器 overlay：`pointer-events:none` 平铺文字「{username} · 密级{N} · {当前时间分钟级}」（PRD 用户+时间；IP 在前端不可得→后置）。
- **服务端水印后置**（PDF 内嵌水印需处理库，台账记）——本增量为 overlay 起步：屏幕防截屏威慑有限，台账如实记录边界。

### D3 前端预览区（DocDetail）
- 文件卡片加「预览」按钮 → fetch blob（复用 axios Bearer）→ 按白名单 MIME 分流：PDF→`<iframe>`（blob URL）、图片→`<img>`、其他→ElMessage 提示走下载。
- overlay 水印层覆盖预览容器；预览状态 ref 控制。
- 后端 415 → 前端 catch 提示「该格式不支持在线预览，请下载查看」。

## 任务清单（TDD）
1. **T1** 后端 preview 端点 + e2e 钉（PDF/PNG inline 200+头、DWG 415、越密级 404、nosniff）。
2. **T2** 前端预览区+水印+Playwright 钉（PNG 预览可见+水印层+DWG 提示）。
3. **T3** 文档：README/部署文档端点表/评审报告 DOC-05 行/migrations.md 裁定（服务端水印后置/SVG 排除理由/CAD 转 PDF 后置）+台账。
4. **T4** 终审 → 修复轮 → fmt 预检 → 合并 → SSH 推送 → CI 绿。

## 验收标准
- e2e 新增 ≥4 钉；后端 165+ 全绿/clippy 0/fmt 过；Vitest 32+/Playwright 13+ 两轮全绿；OpenAPI 注册 preview。

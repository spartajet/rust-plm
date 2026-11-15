# 增量22（DOC-05 在线预览）终审报告

- **Reviewer**：独立 fresh eyes 终审
- **日期**：2026-11-15
- **分支**：`feat/doc-preview`（基线 `main` @ f82a597）
- **待审 commit**：T1 `77a34eb`（后端）/ T2 `37342cf`（前端）/ T3 `e944f4e`（文档）
- **总判定**：**FIX-THEN-MERGE**（I1×1，M1×2，C1×0；代码主体质量高，但 T2 头条特性"平铺水印 overlay"声明与代码不可对账，须修复后合并）

---

## 0. 判定依据速览

| 项 | 结果 |
|---|---|
| 后端测试 | **166 passed / 42 suites / 0 failed**（`TEST_DATABASE_URL` + `CARGO_TARGET_DIR=target-rv22`，实测计数与声明一致） |
| clippy | `--workspace --all-targets` **0 告警，exit 0** |
| fmt | `cargo fmt --check` **通过** |
| Vitest | **32 passed** |
| Playwright | 静态计数 **14**（与声明一致；reviewer 未执行——E2E 全栈搭建不在必做清单 B 内，见 §6 备注） |
| 真库探针 | 全部通过（§3），探针环境已清理（服务已停、`plm_rv22` 已 DROP） |

---

## 1. A 项：commit 与代码对账

### T1（77a34eb）后端 —— 声明逐条对账 ✅
- `crates/api-gateway/src/routes/docs.rs:317-346`：`preview_mime` 白名单 pdf/png/jpeg/gif/webp（6 条目含 .jpg/.jpeg 双扩展），**mime 精确匹配优先**（大小写不敏感、截 `;` 参数，docs.rs:326-338），**扩展名兜底**（docs.rs:339-345）。SVG 双路径均排除：`image/svg+xml` 不在 mime 表、`.svg` 不在扩展名表 ✅
- `docs.rs:366-371`：ABAC 越级 → `ObjNotFound` 404 不泄露存在性（与 detail/file/versions 同语义）✅
- `docs.rs:372-377`：无附件 → 404 ✅
- `docs.rs:378-383`：白名单外 → `DomainError::PreviewUnsupported` ✅；`core-model/src/error.rs:66/116/147`：错误码 `MDL-DOC-PREVIEW-UNSUPPORTED` → HTTP **415** ✅
- `docs.rs:390-408`：`nosniff` + `inline` Content-Disposition（复用 `ascii_fallback`/`pct_encode`，docs.rs:426-450，与 download_doc 同源）+ Content-Length ✅
- `routes/mod.rs:35`：路由注册；`state.rs:85`：`preview_doc` 进入 utoipa paths（OpenAPI 注册）✅
- `plm-server/tests/e2e.rs:3066-3185`：`doc_preview_inline_whitelist_and_abac` 钉 PDF/PNG 头与内容、DWG 415、提密 404、admin 不受影响 ✅

### T2（37342cf）前端 —— 大体对账，水印样式缺失（见 I1）
- `frontend/src/api/docs.ts:258-266`：`previewDoc` blob + content-type 解析 ✅
- `frontend/src/api/http.ts:74-97`：`extractApiError` blob 兜底 + `extractBlobApiError`（415 blob 错误体异步 JSON 解析，失败回退 `extractApiError`）+ `extractApiCode` blob 返回 undefined ✅
- `frontend/src/pages/DocDetail.vue:116-146`：doPreview/closePreview/onBeforeUnmount 三处 revokeObjectURL ✅（一个竞态漏洞见 M1）
- `DocDetail.vue:128`：仅 `application/pdf` 走 iframe，其余全走 `<img>` ✅（XSS 双闸之一，见 §5）
- `DocDetail.vue:361-383`：预览区模板 + `data-test` 钉 ✅；**但 `.preview-wrap/.watermark/.wm-item` 三个类全仓库无任何 CSS 定义、文件无 `<style>` 块**——"平铺水印 overlay" 实际是普通文档流中 24 个 span，不覆盖预览内容（I1）
- `frontend/tests/e2e/flow.spec.ts:658-720`：PNG 预览 + 水印层 + DWG 415 提示钉 ✅（但钉不出 I1：`toBeVisible`/`toContainText` 对无样式文本同样通过）
- 测试 DWG 唯一字节（内容寻址 mime 固化首传语义）与 `mod-doc/src/service.rs:130`（`ON CONFLICT (hash) DO NOTHING`）一致 ✅

### T3（e944f4e）文档 ✅
- README.md:107-108（preview curl 组）、README.md:120（前端段）；部署文档.md:228（端点表行）；`review-需求符合度评审.md` DOC-05 ⬜→🟡；migrations.md:77-84（无新迁移+四裁定）；计划台账回填 ✅
- **注**：migrations.md:81 写明 "pointer-events:none 平铺"——该 CSS 属性在代码中不存在（I1 证据之一）。

---

## 2. B 项：测试实测

```
cargo test --workspace（TEST_DATABASE_URL=postgres://postgres:root@localhost:5432/postgres,
CARGO_TARGET_DIR=target-rv22）→ 42 个 "test result: ok"，合计 166 passed, 0 failed
cargo clippy --workspace --all-targets → 0 warning/error, exit 0
cargo fmt --check → 通过
cd frontend && npx vitest run → 32 passed
```
基线声明（后端 166 全绿、clippy 0、fmt 过、Vitest 32）**全部实测复现** ✅。Playwright 静态计数 14 ✅（未执行，见 §6）。

---

## 3. C 项：真库探针（全过）

环境：`plm_rv22` 新库 → 服务 18098（`PLM_ADMIN_PASSWORD=rv22-pass`，独立 `PLM_DATA_DIR`）→ admin 登录。

| 探针 | 期望 | 实测 |
|---|---|---|
| 上传 pdf（type=application/pdf）/ dwg（octet-stream）/ svg-mime / svg 文件名/octet-stream | 201 | 201×4 ✅ |
| **GET /docs/DWG-RV22-PDF/preview** | 200 + `content-type: application/pdf` + `inline; filename="rv22-a.pdf"; filename*=UTF-8''rv22-a.pdf` + `x-content-type-options: nosniff` + `content-length: 27` + 字节一致 | **全中** ✅ |
| preview DWG | 415 + `MDL-DOC-PREVIEW-UNSUPPORTED` | ✅ |
| preview SVG（mime=image/svg+xml 上传）| 415 | ✅（双路径之一）|
| preview SVG2（同字节文件名 .svg + octet-stream 上传）| 415 | ✅（双路径之二：扩展名兜底不救 SVG）|
| preview 不存在编号 | 404 | ✅ |
| `UPDATE cm_object SET secret_level=2` → 低上限用户（max=0）preview | 404 不泄露 | ✅ |
| 同件 admin preview | 200 | ✅ |
| 无附件分支（DB 删 `file_ref` 后 preview）| 404 | ✅ |
| `openapi.json` | 含 `/api/v1/docs/{obj_no}/preview` | ✅ |
| 中文文件名探针 | inline + `filename*=UTF-8''%E4%B8%AD...`（RFC 5987 正确编码）| ✅ |
| 恶意文件名（含 `"` / CR/LF） | multipart 解析层即 400/`MDL-BAD-REQUEST`——到不了 header 拼接层 | ✅（注入面在 multer 层封死 + `ascii_fallback` 双保险）|

清理：plm-server 已 taskkill、`DROP DATABASE plm_rv22` 完成 ✅。

---

## 4. D 项：专项核查

1. **SVG 双路径排除**：mime 路径（`image/svg+xml` 不在白名单）+ 扩展名路径（`.svg` 不在白名单）均 415，真库双探针实证；`nosniff` 下即使攻击者用 `image/png` mime 传 SVG 内容，浏览器按 png 渲染破图，无脚本执行面 ✅
2. **inline filename 转义复用**：`ascii_fallback`（剥 `"`,`\`,控制字符、非 ASCII）+ `pct_encode`（RFC 5987）与 download_doc 完全同源（docs.rs:426-450）；中文实测编码正确；CRLF/引号文件名被 multipart 解析层拒绝 ✅
3. **iframe 无 sandbox 风险评估**：可接受（见 I-1）。双闸：后端白名单 5 种 mime + 前端仅 `application/pdf` 进 iframe（DocDetail.vue:128），blob.type 取自强制 Content-Type，`blob:` 继承主站 origin 但内容只可能是 PDF/图片，无同源脚本执行路径；另注意给 PDF iframe 加 `sandbox` 可能破坏浏览器 PDF 查看器，不建议盲改，建议补注释记录此裁定
4. **blob 生命周期 revoke 链**：关闭/重开/卸载三路径齐备；发现导航竞态漏洞一处（M1）
5. **extractBlobApiError 兜底**：blob→text→JSON.parse→message，非 JSON 回退 `extractApiError`（其自身新增 Blob 分支给通用文案）；`"message" in d` 前有 `typeof d === "object"` 守卫，字符串 data 不会触发 `in` 操作符异常 ✅
6. **增量21 修复轮完好性回扣**：M1-2 CSV 公式注入 `'` 前缀守卫在位（`api-gateway/src/routes/boms.rs:374-381`）；M1-3 major 切换/折叠展开自动刷新在位（`DocDetail.vue:260-262,467`）；I1-1 快照只读提示在位（`DocDetail.vue:422`）✅
7. **Dockerfile 无需改动**：无新 workspace 成员、无新系统依赖（utoipa/ReaderStream 均为既有），f82a597..HEAD 对 Dockerfile 零 diff ✅
8. **迁移注册表仅 docs**：无 SQL 迁移文件新增（8 个 crates/migrations 目录零改动），migrations.md 增量22 节四条裁定齐备 ✅

---

## 5. 分级发现

### C1：无

### I1 —— T2"平铺水印 overlay"声明与代码不可对账（必须修复）

**证据**：
- `frontend/src/pages/DocDetail.vue:361,374-375`：模板引用 `.preview-wrap` / `.watermark` / `.wm-item` 三个类，但**整个文件无 `<style>` 块**；全 `frontend/src`（含 `styles/main.css`、index.html）grep 这三个类名**零 CSS 定义**。
- 实际渲染：水印是预览元素**下方**普通文档流中的 24 个 span 文本块——不 absolute、不平铺、不覆盖预览内容，PDF/图片本身无任何水印标记；同文件 380 行关闭按钮 `position:absolute` 无 positioned 祖先，锚定到视口而非预览框。
- 文档面声明与之冲突：`docs/migrations.md:81`（"pointer-events:none 平铺"——该属性不存在）、README.md:120（"详情在线预览(PDF/图片+平铺水印)"）、`docs/review-需求符合度评审.md` DOC-05 行（"前端平铺水印（用户/密级/时间）✅"）、T2 commit message（"平铺水印 overlay"）。
- 既有钉拦不住：`flow.spec.ts` 只断言 `toBeVisible` + `toContainText`，无样式文本同样通过。

**影响**：水印防截屏威慑/泄露溯源（本增量对 DOC-05 🟡 判定的支撑点之一）实际未生效；对账红线（声明必须与代码可对账）被触碰。

**最小修复**（~15 行，DocDetail.vue 末尾加 scoped style，不改任何测试即可保持全绿）：
```css
.preview-wrap { position: relative; }
.watermark {
  position: absolute; inset: 0; pointer-events: none;
  overflow: hidden; z-index: 1;
}
.wm-item {
  display: inline-block; margin: 18px; opacity: 0.12;
  transform: rotate(-18deg); font-size: 14px; white-space: nowrap;
}
```
（备选：若有意降级为"非 overlay"实现，则须同步修正 migrations.md:81 / README / 评审报告表述，并注明无 CSS 系有意为之——但"平铺"措辞仍不成立，不推荐。）

### M1 —— doPreview 在途导航竞态泄漏 objectURL

`frontend/src/pages/DocDetail.vue:116-137,144-146`：`previewDoc()` 请求在途时若路由离开组件，`onBeforeUnmount`（:144）先执行（此时 previewState 为 null 无可 revoke）；随后 promise 兑现，:129 `URL.createObjectURL` 照常执行并赋给已卸载组件的 ref——该 blob URL 在整个 SPA 会话期内无人释放。
**最小修复**：`let disposed = false;` 于 `onBeforeUnmount` 置 `disposed = true`；:129 前检查 `if (disposed) { return; }`（或在 createObjectURL 后立即检查并 revoke）。

### M2 —— 预览 e2e 缺两根钉（SVG 双路径 415、无附件 404）

`plm-server/tests/e2e.rs:3170`：SVG 排除仅以注释存在（"SVG 同理排除"），无实际断言；`docs.rs:372-377` 无附件 404 分支无 e2e 覆盖（本次靠真库 DB 探针实证）。两分支均为安全语义关键路径，缺钉意味着后续回归无拦截。
**最小修复**：在 `doc_preview_inline_whitelist_and_abac` 内补：上传 `image/svg+xml` mime 各一 + 断言 415；对任一 doc 在 `ctx.pool` 执行 `DELETE FROM file_ref ...` 后断言 preview 404。

### I-1（备案，不阻断）—— iframe 无 sandbox：评估为可接受
双闸封闭（后端白名单 + 前端仅 pdf 进 iframe + nosniff；blob.type 不可能为 text/html/svg），无同源脚本执行路径；PDF 查看器兼容性优先，建议在 `DocDetail.vue:368` iframe 处补一行注释记录该裁定（与 migrations.md 裁定互链）。

### I-2（备案）—— ABAC 检查与 open_original 之间毫秒级 TOCTOU
`docs.rs:361-388`：detail 密级检查与 open_original 之间无事务——提密并发窗口内可能放过一次流。与既有 download_doc/get_doc/versions_doc 完全同模式（系统性既有语义），非本增量引入，不建议本增量修。

### I-3（备案）—— `docs.rs:391` `ct.parse().unwrap()`
ct 来自编译期白名单常量（6 个合法 MIME 字面量），unwrap 不可失败；可改 `expect("whitelisted mime")` 增强意图表达，非必须。

---

## 6. 备注

1. **Playwright 未执行**：必做清单 B 不含 Playwright 全栈 E2E；reviewer 静态计数 14 与声明一致，实现者声明两轮全绿（`retries: 2` 配置在案）。合并前若跑修复轮，建议顺带执行两轮以复核 I1 修复后钉仍绿。
2. 探针上传的四份 svg/pdf/dwg 测试件随库一并 DROP，无残留。
3. T1 代码质量高：错误域收敛、ABAC 语义与全站一致、测试钉三层排障记录（计划台账）属实。

---

## 7. 结论

后端 T1 全部声明可对账且真库实证通过；T3 文档齐备；基线四项指标实测复现。唯 T2 的"平铺水印 overlay"仅落了 DOM、未落样式——该特性按声明不可对账，且文档（migrations.md:81 的 pointer-events:none）与代码直接矛盾。

**总判定：FIX-THEN-MERGE** —— 修复 I1（补 watermark CSS 或如实改文档）后即可合并；M1/M2 强烈建议随修复轮一并处理（合计 <30 行改动）。

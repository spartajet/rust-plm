# 增量12：前端 UX 收口一揽子 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。
> 用户指令"IM 先不做，其他的依次优化"——本增量=deferred 清单前端批（M8 角色显隐 / M9 zh-cn locale + refresh 接线 / M2 上传客户端校验 / 增量11 M2 state URL 回写 / R1 stateTagType 钉）；IM 相关与后端杂项债顺延后续增量。

**Goal:** 界面按角色显隐（M8）、中文 locale（M9）、refresh_token 单飞接线（M9）、上传客户端预校验（增量7 M2）、列表筛选 URL 回写（增量11 M2）——UX 债一次清偿。

**短设计（Approaches）:**
- **A（选定）login/refresh 响应附 roles + 前端角色→能力映射**：SecService::login/refresh 查 `sec_user_role` 附 `roles: Vec<String>`；前端 store 存 roles，`capabilities` getter 映射 `docs:write`=editor|admin、`docs:approve`=admin、`sec:manage`=admin（与 RBAC 种子 0005 一致）。零新端点、零 token 结构变更（Claims 不动——角色变更即时生效性由后端 casbin 把关，显隐仅 UX）。
- B（否）/auth/me 端点：多一跳请求与端点面；角色在登录时已知，无增量价值。
- C（否）Claims 内嵌 roles：角色变更需等 token 过期才反映（最长 15min），且 token 结构变更波及 parse/enforce 全链——违背最小改动。
- refresh 单飞：http.ts 401 拦截器捕获→并发请求共享同一 refresh Promise（单飞）→成功换发重放原请求（body 仅 Buffer/string 安全——axios 已序列化）→失败/无 refresh_token→clear+跳登录。

**Spec:** PRD §5 工作台/§7.1 认证、增量7 M2/M8/M9 台账、增量11 M2 台账。

## Global Constraints

- **roles 附带**：`TokenPair` 增 `roles: Vec<String>`；login/refresh 都查 `sec_user_role ORDER BY role_code`；响应 DTO 透传。空角色（零角色用户）→ `[]`（合法，仅无权限提示）。
- **前端映射**（auth store getter）：`canWrite = roles ∩ {editor,admin}`；`canApprove = roles ∩ {admin}`；`canManageSec = roles ∩ {admin}`。**角色→能力映射为前端常量**（RBAC 种子一致）；自定义角色记账。
- **M8 显隐**：MainLayout「用户管理」菜单 `v-if canManageSec`；DocsList「上传图纸」`v-if canWrite`；DocDetail 生命周期按钮组：submit/revise→canWrite，approve/reject/obsolete→canApprove（obsolete 后端在 approve 组——mod.rs docs_lc_approve 组实证）；下载对所有人（docs:read 全角色）。
- **refresh 单飞**：仅 access 401 且 err.config 非 `/auth/` 且 `_retry` 未标记时触发；换发后 `err.config.headers` 更新 Bearer 并 `_retry=true` 重放；`refresh_token` 缺失或换发 401 → `setUnauthorizedHandler` 跳登录。`TokenPairDto` 前端解析并存 `refresh_token`。
- **zh-cn**：main.ts `app.use(ElementPlus, { locale: zhCn })` + `import "dayjs/locale/zh-cn"`；分页 total 变「共 N 条」——**board e2e 的 total 文本断言不受影响**（.el-pagination__total 数值不变，regex word-boundary 仍匹配）。
- **上传预校验**（DocsList doUpload 前置）：文件非空、size ≤ 200MB（`MAX_UPLOAD_MB` 常量，与后端缺省 max_upload 一致）、扩展名白名单 `.dwg/.dxf/.step/.stp/.iges/.igs/.pdf/.png/.jpg/.jpeg/.zip` 之外仅 WARN 不阻断（图纸类型开放，硬阻断会挡长尾格式——台账记裁定）。
- **URL 回写**：DocsList `onStateChange`/material 变更后 `router.replace({ query })`（不产生历史记录）；`watch(route.query.state)` 同步外部导航（看板点击）——避免"刷新跳回旧筛选"。
- **R1**：stateTagType("checked")==="warning" unit 钉（Vitest）。

## Review Focus

1. **login/refresh 响应含 roles（editor=["editor"]、admin=["admin"]；refresh 轮换后 roles 仍在）** → 后端 e2e 钉
2. **refresh 单飞：access 过期场景换发重放成功；refresh 也失效→跳登录** → Vitest（axios-mock-adapter 风格手写 adapter 或 http 层 mock）
3. **M8：viewer 登录后无「上传图纸」按钮、无「用户管理」菜单；admin 全量** → Playwright 钉
4. **上传预校验：超大文件前端拒绝（不发请求）；非常规扩展名仅 WARN 可继续** → Vitest（纯函数校验器）
5. **state 筛选 URL 回写：看板点击→手动换筛选→URL query 同步；刷新后筛选保持** → Playwright 钉

## File Structure

```
crates/core-sec/src/service.rs(TokenPair+roles, login/refresh 查询) + tests(无——e2e 承载)
crates/api-gateway/src/routes/auth.rs(DTO+透传)
crates/plm-server/tests/e2e.rs(+roles 钉)
frontend/src/api/http.ts(refresh 单飞拦截器) + auth.ts(TokenPair+roles)
frontend/src/stores/auth.ts(+refresh_token/roles/capabilities)
frontend/src/main.ts(zh-cn) + layouts/MainLayout.vue(菜单显隐)
frontend/src/pages/{DocsList.vue(上传校验+回写+按钮显隐), DocDetail.vue(按钮显隐)}
frontend/src/utils/upload.ts(校验纯函数) + tests/unit.spec.ts(+校验器/stateTagType 钉/refresh 单飞)
frontend/tests/e2e/flow.spec.ts(+viewer 显隐钉/URL 回写钉)
docs/superpowers/plans/2026-11-13-ux-hardening.md；README；台账
```

## Tasks

### Task 1: 后端 roles 附带 + e2e 钉
- Step1 失败 e2e：login 响应 roles=["admin"]；editor 登录 roles=["editor"]；refresh 后 roles 仍在
- Step2 RED；Step3 实现（TokenPair+roles/issue 查询/login/refresh 附带/DTO）；Step4 GREEN；Step5 Commit

### Task 2: 前端 auth store + refresh 单飞 + Vitest
- Step1 失败 Vitest：refresh 单飞（并发 401 共享一次换发、换发成功重放、失败跳登录）；Step2 RED；Step3 实现（http.ts 拦截器/store capabilities）；Step4 GREEN；Step5 Commit

### Task 3: M8 显隐 + zh-cn + 上传校验 + URL 回写
- Step1 失败 Vitest：上传校验纯函数（空/超大/白名单 WARN）；stateTagType checked 钉；Step2 RED→Step3 实现（五处 UI+main.ts locale）→Step4 GREEN；Step5 Commit

### Task 4: Playwright 钉 + 全门禁 + 终审 + 合并
- Step1 Playwright：viewer 登录断言无上传按钮+无用户管理菜单；admin 有；看板点击→换筛选→URL 同步→刷新保持
- Step2 README（前端段更新）+台账；Step3 全门禁（cargo/fmt/clippy/vitest/vue-tsc/Playwright 实跑）；Step4 Commit；Step5 fresh reviewer→修复轮→合并 main→push→CI 绿

## Self-Review

覆盖 M8/M9(refresh+locale)/M2 上传/URL 回写 ↔ Review Focus 5 条 ↔ Task1-4 ✓；签名一致（TokenPair+roles 向后兼容——DTO 加字段不破坏既有消费）✓；IM 相关未混入 ✓；角色映射硬编码记账（自定义角色后置）✓；比例 ✓

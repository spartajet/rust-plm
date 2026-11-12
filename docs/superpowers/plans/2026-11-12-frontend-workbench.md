# 增量7：前端工作台 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。

**Goal:** 首个 UI 增量——登录/工作台壳/图纸列表（筛选+分页+上传）/详情（属性+生命周期+BOM），JWT 路由守卫，Vitest 组件测试 + Playwright 真链路 e2e。

**短设计（Approaches）:** A（选定）按 ADR-0013：Vue3+TS+Vite+Pinia+Vue Router+Element Plus+UnoCSS；axios 封装统一错误体/401 处理；生命周期按钮显隐由纯函数 `flowUi(state)` 驱动（可穷举单测）。B 直接 DOM 字符串模板（否）。范围偏离记录：UnoCSS 采用最小 preset 安装；Playwright 本机真跑，CI 浏览器步骤后置（ADR-0019 既定）。

**Spec:** ADR-0013、ADR-0015（错误体/分页）、PRD §7.7 TQ、既此后端行为（auth/docs/boms/meta 端点）。

## Global Constraints

- 目录 `frontend/`（Monorepo 子目录，pnpm 独立 package，不进 Cargo workspace）
- 后端 base URL：`VITE_API_BASE`（缺省 http://localhost:8080）
- 401 统一处理：清 token → 跳 /login；错误体 `{code,message,trace_id}` → ElMessage.error(message)
- 生命周期显隐纯函数 `flowUi(state): {submit,approve,reject,obsolete}`——released/obsolete 全 false；review: approve/reject true；working: submit true（obsolete true）
- 测试：Vitest（auth store/flowUi/列表组件 mock 渲染）；Playwright chromium 走真后端链路（登录→上传→列表→详情→submit→approve→released）

## Review Focus

1. 401 拦截：token 失效后任意 API 调用 → 清 token 跳 /login → Vitest 钉
2. flowUi 全状态穷举（working/review/released/obsolete × 4 按钮）→ Vitest 钉
3. 列表筛选参数（filter[material]）拼装与后端契约一致 → Vitest 钉（URL 构造）
4. 上传成功后对话框关闭并刷新列表 → Playwright 钉
5. 生命周期操作后状态 Tag 更新 → Playwright 钉

## Tasks

### Task 1: 脚手架（vite/ts/ep/unocss/pinia/router/axios/vitest）
- Files: frontend/{package.json,vite.config.ts,tsconfig*,index.html,src/{main.ts,App.vue,router/index.ts,api/{http.ts,auth.ts,docs.ts},stores/auth.ts,styles/main.css}, tests/ 基础}
- Interfaces: `useAuthStore{token,username,login(u,p),logout}`；`http`（拦截器）；`docsApi{list(params),get(no),upload(form),transition(no,ev),bomLines(no,lines),bomExpand(no),whereUsed(no)}`；`flowUi(state)`
- Step1 脚手架 + Vitest 冒烟（App 挂载）；Step2 pnpm i && pnpm test 绿；Step3 Commit

### Task 2: 认证 + 登录页 + 路由守卫
- Step1 失败测试：flowUi 穷举矩阵 + auth store（mock http）+ 401 拦截钉；Step2 RED→GREEN；Step3 登录页组件 + 守卫（未认证访问 / → redirect /login）；Step4 Commit

### Task 3: 图纸列表 + 上传 + 详情（生命周期/BOM）
- Step1 组件测试（mock axios：列表渲染行数/筛选参数拼装/详情渲染属性与按钮显隐）；Step2 实现；Step3 GREEN；Step4 Commit

### Task 4: Playwright e2e（真后端链路）
- Step1 playwright.config（baseURL vite dev 5173；后端测试库手动起）；用例：登录→上传→列表可见→详情→submit→admin approve→released Tag；Step2 本机全绿（chromium 安装一次）；Step3 Commit

### Task 5: 门禁 + README + 构建产物接入 Rust（rust-embed 预留：本增量仅 `pnpm build` 产出 dist/）+ 终审 + 合并
- Step1 fmt(前端 prettier 可后置)/clippy/后端全测/前端 vitest+build；Step2 README 前端段；Step3 Commit；Step4 终审→修复→合并 main

## Self-Review
覆盖目标页面/守卫/错误处理 ↔ Task2-3；测试策略 ↔ Vitest+Playwright（Review Focus 5 条：#1/#2/#3 Vitest、#4/#5 Playwright）✓；偏离记录（UnoCSS 最小、CI Playwright 后置）✓；比例 ✓

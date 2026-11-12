[增量6 追加台账] Final: fixed C2 补丁拼装遗漏（expand/where_used 缺 Extension(user)）——main 合并后编译失败暴露，commit 29b5e9f 修复；教训（第三次）：修复轮后必须 cargo check --workspace --tests + 全套件 EXIT=0 双确认再合并。
[增量7 追加] Ruling: pnpm/corepack/npx 拉取均失败 → 前端包管理改用 npm（偏离 ADR-0013 pnpm 工作区；frontend/ 为独立 npm package，Monorepo pnpm 接入后置）。
[增量7 追加台账]
Final: fixed e2e 断言体系统一（getByTestId 默认找 data-testid 与项目 data-test 不符）→ 统一 locator('[data-test=...]')，旅程跑通 1/1 (2.9s)
Final: fixed 上传 toast 竞态——ElMessage 3s 即逝不作断言，改等待刷新后的列表行（功能断言更有意义）
Final: Ruling: vite dev 代理 /api→后端（同源化免 CORS），VITE_API_BASE 置空——.env 缺省 http://localhost:8080 会打空，部署文档需注明
Final: Ruling: pnpm/npm 偏离（网络拉取失败）——frontend 独立 npm package
[增量7 修复轮台账]
Final: fixed C1 守卫未接线（/ 页无兜底）— guard redirect e2e 钉 RED(停留页面)→GREEN(重定向 /login), suite 后端93+前端14+e2e2
Final: fixed C2 window.open 下载必 401 — blob 下载 + 删 downloadUrl 死代码, e2e journey 内含下载按钮渲染
Final: fixed I1 API_BASE 缺省 8080 — 改空串同源（生产反代）, README 注明跨机部署须显式指向
Final: fixed I2 README 快速开始矛盾 — VITE_API_BASE= + 端口对齐 18095
Final: fixed I5 e2e objNo 唯一化（Date.now）— 可重复门禁成立
Final: fixed M1 login 401 旁路 / M3 encodeURIComponent / M4 gitignore / M7 vitest/config / 后端 no-store（生命周期 e2e 实证 GET 缓存竞态的治本）
Final: minor (deferred): M2 上传无客户端大小/类型校验; M8 按钮不按角色显隐(viewer 见 approve 按钮点击 403——UX 债, Roadmap 深化已列); M9 zh-cn locale/timeout 30s/refresh 未接线
Final: Ruling: I3 released→obsolete:true 为有意偏离计划原文（PLM 语义合理），补台账裁定——后续按台账改计划而非改代码。

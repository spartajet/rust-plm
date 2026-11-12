[增量6 追加台账] Final: fixed C2 补丁拼装遗漏（expand/where_used 缺 Extension(user)）——main 合并后编译失败暴露，commit 29b5e9f 修复；教训（第三次）：修复轮后必须 cargo check --workspace --tests + 全套件 EXIT=0 双确认再合并。
[增量7 追加] Ruling: pnpm/corepack/npx 拉取均失败 → 前端包管理改用 npm（偏离 ADR-0013 pnpm 工作区；frontend/ 为独立 npm package，Monorepo pnpm 接入后置）。
[增量7 追加台账]
Final: fixed e2e 断言体系统一（getByTestId 默认找 data-testid 与项目 data-test 不符）→ 统一 locator('[data-test=...]')，旅程跑通 1/1 (2.9s)
Final: fixed 上传 toast 竞态——ElMessage 3s 即逝不作断言，改等待刷新后的列表行（功能断言更有意义）
Final: Ruling: vite dev 代理 /api→后端（同源化免 CORS），VITE_API_BASE 置空——.env 缺省 http://localhost:8080 会打空，部署文档需注明
Final: Ruling: pnpm/npm 偏离（网络拉取失败）——frontend 独立 npm package

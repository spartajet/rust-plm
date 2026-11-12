[增量6 追加台账] Final: fixed C2 补丁拼装遗漏（expand/where_used 缺 Extension(user)）——main 合并后编译失败暴露，commit 29b5e9f 修复；教训（第三次）：修复轮后必须 cargo check --workspace --tests + 全套件 EXIT=0 双确认再合并。
[增量7 追加] Ruling: pnpm/corepack/npx 拉取均失败 → 前端包管理改用 npm（偏离 ADR-0013 pnpm 工作区；frontend/ 为独立 npm package，Monorepo pnpm 接入后置）。

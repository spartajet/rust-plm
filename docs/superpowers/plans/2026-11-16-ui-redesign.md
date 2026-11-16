# 增量29：UI 设计系统重设计（Industrial Slate · 明暗双主题）

> 目标：系统化重设计前端 UI——建立统一设计 token 层与主题机制（明/暗切换+持久化），替代拼凑式默认样式；工业 PLM 专业感（skill 检索：Minimalism/Swiss 风格 best for enterprise dashboards；亮暗双支持）。**硬约束：全部 17 个 Playwright data-test 钉与路由零破坏**。
> 分支：`feat/ui-redesign`；纯前端；无后端改动。

## 设计系统「Industrial Slate」（skill 检索 Minimalism & Swiss + 制造业工业感定制）

### Tokens（CSS 变量，双主题）
- 品牌主色：Indigo-600 `#4F46E5`（light）/ Indigo-500 `#6366F1`（dark）——与 Element 默认蓝 #409EFF 明确区分
- Accent/CTA：橙 `#EA580C`（警示/制造语汇，仅关键动作用）
- 中性色：Slate 阶（light bg #F6F7FB / card #FFFFFF / border #E2E8F0 / text #0F172A + #64748B；dark bg #0B1220 / card #111A2E / border #1E293B / text #E2E8F0 + #94A3B8）
- 语义色沿用 Element（success/warning/danger/info）经 token 映射
- 圆角：控件 8px / 卡片 12px；阴影：三层 subtle（dark 下加深+描边替代）
- 字体：system-ui + PingFang SC/MiYaHei 栈；数据列 tabular-nums
- 密度：控件高 32px 标准、表格 compact

### 主题机制
- `element-plus/theme-chalk/dark/css-vars.css` 导入 + `html.dark` class 驱动（Element 官方暗色变量体系，非拼凑覆盖）
- 自研 token 层 `styles/tokens.css`：`:root` 亮值 + `html.dark` 暗值 + `--el-*` 桥接（Element 变量跟随 token）
- `useTheme` composable：localStorage `rustplm-theme`（light/dark，缺省 light）；顶栏切换按钮（icons-vue Sunny/Moon）；`prefers-color-scheme` 不做 auto（演示确定性优先，台账记）

### 布局系统化（MainLayout 重构）
- 侧边栏：品牌区（RustPLM 徽标块+副题）→ 导航（icons-vue 图标+文字，激活态左侧指示条）→ 底部主题切换+登出；亮暗双态侧边栏（dark=#0B1220 / light=白卡+边）
- 顶栏：面包屑式页题（路由 meta）+ 全局搜索右移 + 用户区（头像圈+登出）
- 内容区：统一 page 容器（padding/max-width）

### 组件统一化（全局 CSS，不动组件结构/data-test）
- el-card 统一圆角/边/阴影/hover 微抬；el-table 表头底色/斑马纹/compact；el-button 主按钮实色圆角；el-tag 圆角；el-dialog 圆角头部；el-tabs 下划线加粗；滚动条细化
- 登录页：品牌渐变背景 + 居中卡 + 徽标

## 任务（TDD/钉）
1. **T1** tokens.css + dark 接入 + useTheme（含持久化纯函数 themeFromStorage 可单测）+ Vitest 2 钉 + 顶栏切换按钮
2. **T2** MainLayout 重构 + Login 重设计 + 全局组件覆盖（styles/components.css）
3. **T3** Playwright 新钉（切换按钮→html.dark→刷新持久→再切回）+ 全量 18×2 回归 + Vitest
4. **T4** 文档（README 前端段/计划台账）
5. **T5** 终审 → 修复轮 → fmt 预检 → 合并 → SSH 推送 → CI 绿

## 验收
- Playwright 18×2 全绿（17 既有钉零破坏 + 1 主题钉）；Vitest 38+；vue-tsc 过；build 过
- 明暗两套视觉完整（侧边栏/卡片/表格/登录页全部跟随）；主题选择刷新持久

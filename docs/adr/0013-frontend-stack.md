# ADR-0013 前端技术栈：Vue3 + Element Plus + UnoCSS + vxe-table + bpmn-js + form-create

- 状态：已接受（2026-11-11，选型批次 4/5）
- 关联需求：全 Web 工作台、BOM-01、WF-01、AGL-02

## 决策

- 框架：Vue3 + TypeScript + Vite + pnpm workspace；状态 Pinia；CSS 用 UnoCSS（与 EP 共存）；
- 组件库 Element Plus（制造业管理端事实标准、中文生态、招人池最大）；
- BOM 大表格 vxe-table（10 万行虚拟滚动/可编辑单元格/锁定列，MIT 国产专精 ERP 场景）；
- 流程设计器 bpmn-js（BPMN 2.0 标准，与后端 XML 存储一致，语义校验开箱即用）；
- 表单设计器基于 form-create（MIT）二次开发（省 3–4 周，JSON Schema 与元数据模型天然契合）；
- 图表 ECharts；3D 用 Three.js（ADR-0018 关联 VIS-02）；
- 小程序：uni-app（一套代码编译微信/钉钉/H5，与主前端同栈，复用类型与业务组件，INT-04b）。

## 后果

- ✅ 全栈同语言（Vue 系），跨端组件/类型复用最大化；
- ⚠️ vxe-table/form-create 均为社区库——锁主版本 + 封装薄适配层（禁止业务代码直调其 API），防上游断更；
- ⚠️ bpmn-js 样式偏工程风——以主题 CSS 定制，不改其内核。

# 增量24：AGL-05 行业模板包（导出/导入+Starter 预置包） + AGL-09 实施向导

> 需求评审 P0-B 首项（第 8 项）。PRD §6.16：AGL-05 整套配置导出/导入为单一模板文件+随产品分发 Starter 基础包+导入冲突检测；AGL-09 实施 Checklist 工作流（组织→权限→编码→类型→流程→导入→培训→切换，每步操作指引+完成校验）——支撑场景 S5「7 天上线」。
> 分支：`feat/agl-template-wizard`；**无新迁移**（checklist 人工状态复用 sys_param；模板只动 cm_type/cm_attr/cm_class 既有表）；无新 crate。

## 设计裁定

### D1 模板包格式（AGL-05）
单一 JSON：`{format:"rustplm-template", version:1, exported_at, types:[{code,name,base_type,config,attrs:[{code,name,data_type,required,sort_no}]}], classes:[{code,name,parent_code,cod_rule}]}`。
- 配置域=增量至今的全部参数化面：cm_type.config（flow_tier/cod_rule/require_comment）+cm_attr 属性+cm_class 分类挂规则。报表/看板后置（台账——RPT-04 报表库未建，无可导出实体）。
- 插件依赖声明（PLG-08）后置（插件系统 V1.5）。

### D2 端点
- `GET /api/v1/meta/template/export`（sec:manage）——实时导出全量类型+属性+分类。
- `GET /api/v1/meta/template/starter`（sec:manage）——**内置 Starter 基础包**（`include_str!("../../../templates/starter.json")` 随二进制分发——零 Dockerfile 资产改动，PRD"随产品分发"达成；行业包后续以同格式补文件）。
- `POST /api/v1/meta/template/import`（sec:manage）——body `{template, mode:"dry_run"|"merge"}`：
  - 冲突检测：type 已存在 → conflict（除非 attr/config 可增量？**裁定：存在即 skip 整型**——部分合并语义复杂且危险，向导如实报告）；class 同码 → 比对差异为 conflict（报告现值），merge 时**跳过**；parent_code 缺失 → conflict。
  - merge：新 type+attrs+class 插入（ON CONFLICT DO NOTHING 语义由应用层 skip 保证）；返回 `{applied_types, applied_classes, skipped, conflicts}`。
  - dry_run：完整跑冲突检测不落库（导入向导"冲突检测"达成）。
  - audit 留痕：`template.import {types, classes, mode}`。

### D3 实施向导（AGL-09）
八步 Checklist（PRD 顺序），每步 `{key,title,hint,done,auto}`：
1. 组织组织（auto：sec_user 数 ≥2 即有真实成员）
2. 权限配置（auto：存在非 admin 用户挂角色）
3. 编码规则（auto：cm_class 挂 cod_rule 数 ≥1）
4. 类型建模（auto：cm_attr 总数 ≥4——超出种子即有自定义）
5. 流程绑定（auto：任一 cm_type.config 显式含 flow_tier——用户设过）
6. 数据导入（auto：cm_object 数 ≥1）
7. 培训交付（manual：POST complete 标记，audit 留痕）
8. 切换上线（manual：同上；依赖 7）
- 人工状态存 sys_param `impl_checklist_done`（JSON 数组）——复用参数中心，无新迁移。
- `GET /api/v1/meta/impl-checklist`（认证即可——向导全员可见）；`POST /api/v1/meta/impl-checklist/{step}/complete`（sec:manage；步骤须 manual、须按序——前置步骤未 done 400）。

### D4 前端
- Workbench 顶部「7 天上线向导」卡片：八步清单（状态徽章+指引文案+跳转链接+manual 步骤完成按钮）。
- UserAdmin 或独立？**放 Workbench**（向导属性）。模板导入 UI：Workbench 向导卡内「导入 Starter 包」按钮（dry_run 预检展示冲突→确认 merge）+自定义 JSON 上传（textarea 粘贴，MVP 免文件上传栈）。

## 任务清单（TDD）
1. **T1** 后端模板三端点（export/starter/import+dry_run）+ starter.json 资产 + e2e 钉（导出结构/导入空库 applied/重复导入 skipped 冲突/dry_run 不落库/坏 format 400/非 manage 403/audit 留痕）。
2. **T2** checklist 两端点 + e2e 钉（auto 步实时判定——建用户/设 flow_tier/传对象后翻转、manual complete 顺序约束/audit、sys_param 持久化）。
3. **T3** 前端向导卡+导入流 + Playwright 钉（八步渲染/dry_run 冲突展示/确认导入 applied/complete 按钮）。
4. **T4** 文档：README/部署文档端点表/评审报告（AGL-05 🟡→🟡+、AGL-09 ⬜→🟡）/migrations.md 裁定/台账。
5. **T5** 终审 → 修复轮 → fmt 预检 → 合并 → SSH 推送 → CI 绿。

## 验收标准
- e2e 新增 ≥7 钉；后端 168+ 全绿/clippy 0/fmt 过；Vitest 34+/Playwright 15+ 两轮全绿；OpenAPI 注册五端点+模板/Checklist schema。
- starter.json 与导出格式同一 schema（导入导出往返自洽——e2e 导出→清库→导入等价性钉或结构断言）。


---

## 对账台账（执行后回填）

- T1 完成（db664ea）：include_str! 路径 ../../../ 多退一级（routes → workspace 根）；serde 强类型校验模板结构（坏包 400）；dry_run 与 merge 同一冲突检测代码路径。
- T2 完成（db664ea）：checklist GET 起初误挂 sec_mgmt 组（viewer 403 被钉拦下）——拆独立组 GET require_authn/complete require_perm；auto 步阈值语义=代码为权威；e2e flow 步翻转补 PUT flow-tier（种子 config 无显式 flow_tier——正是该步语义）。
- T3 完成（8f5ee3e）：**impl_checklist_done 借 sys_param 混入参数面板第 4 行可编辑行**（list_params 全量返回未过滤——security params Playwright 钉 4≠3 拦下）；list_params 过滤内部键+e2e 钉；python replace 静默 no-op 第三次（读代码 grep 核对后 Read+Edit 直改——教训累计固化：多行 SQL/JSON 串替换禁用 python）。
- T4 完成：README/部署文档/评审报告（AGL-05 增补、AGL-09 ⬜→🟡、S5 场景大半）/migrations.md 五裁定。

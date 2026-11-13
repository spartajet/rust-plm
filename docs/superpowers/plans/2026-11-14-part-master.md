# 增量17：PART 物料主数据纵切 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。
> 常设指令"一直往前做"持续有效；IM 暂缓。PRD §6.3 PART-01/02/05 核心纵切（COD-03 分类规则为其前置，增量16 已就绪）。

**Goal:** 分类树（多级，挂编码规则）+ part 类型种子 + 物料创建走分类规则自动赋码 + 智能查重 MVP（名称/材料模糊检索相似件提示）+ 物料列表页——一物一码闭环。

**短设计（Approaches）:**
- **A（选定）cm_class 分类树 + mod-part 编排**：`cm_class(id,parent_id,code unique,name,cod_rule,enabled)`（0015，core-model 元数据域）；`mod-part::PartService`（镜像 mod-doc 模式）：create（解析 class.cod_rule→COD 取号 key=class code→ObjectStore.create("part", obj_no)）、duplicates（part 类型内 attrs name/material ILIKE 模糊，LIMIT 5——低频交互调用，JSONB ILIKE 慢路径可接受记账；语义检索后置）、list（ObjectQuery 复用）。
- B（否）独立规则实体化/分类挂属性模板全集：PART-02 完整版后置（本纵切分类只挂 cod_rule）。
- C（否）通用 objects 端点重构网关：波及大，mod-part 独立 crate 依 mod-doc 先例。

**Spec:** PRD §6.3 PART-01/02/05、ADR-0003、增量16 COD。

## Global Constraints

- 0015（core-model）：cm_class 表 + part 类型种子（base_type=part，attrs：name 必填/material/spec/model）+ 二层示例分类种子（自制件/标准件 → 紧固件/钣金件，cod_rule 各异）。
- create：class 必须存在且 enabled；cod 规则=class.cod_rule（计数器 key=class code——COD-02 按规则独立流水）；attrs 回填 class_id/class_code/class_name；响应带 duplicates（相似件列表，不阻断——强制门禁后置记账 PART-05 完整版）。
- 网关：`POST /api/v1/parts`（docs:write 门控——SMB 简化裁定，独立 parts 权限后置）、`GET /api/v1/parts`（docs:read，filter name/material/page/size）、`GET /api/v1/parts/duplicates?name=&material=`（docs:read）。ABAC 密级同 docs 语义。
- 类型模板差异化（自制件/外购件独立属性集，PART-01 完整版）后置——本纵切单 part 类型+分类树。
- 前端：/parts 页（列表+新建对话框：分类下拉/名称/材料/规格/自动赋码说明+查重提示 toast），菜单"物料"。

## Review Focus

1. 分类编码赋码：不同 class 各自独立流水（A 类 0001 与 B 类 0001 并存）→ 集成钉
2. 查重：同名/同材相似件返回（含自身排除）；创建响应带 duplicates → 集成钉
3. 网关权限/ABAC：parts 端点 docs 门控生效、越密级 404 → e2e 钉
4. 前端：新建物料 → 列表出现分类码物料 → Playwright 钉
5. 分类树 API：GET 树形返回父子嵌套 → e2e 钉

## Tasks

### Task 1: 0015 迁移 + mod-part crate + 集成钉
- Step1 失败测试（part_it：分类赋码独立流水/查重/树）；Step2 RED；Step3 实现；Step4 GREEN；Step5 Commit

### Task 2: 网关三端点 + e2e 钉
- Step1 失败 e2e（赋码/查重/权限/树）；Step2 RED；Step3 实现；Step4 GREEN；Step5 Commit

### Task 3: 前端物料页 + Playwright 钉
- Step1 实现（api/part.ts+Parts.vue+路由菜单）；Step2 Playwright（新建→列表分类码+查重提示）；Step3 Commit

### Task 4: README/台账(migrations.md 0015)/全门禁/终审/合并/push/CI

## Self-Review

覆盖 PART-01/02/05 纵切 ↔ Review Focus 5 ↔ Task1-4 ✓；mod-part 依 mod-doc 先例 ✓；迁移 0015 唯一（docs/migrations.md 补录）✓；类型模板差异化/强制门禁/语义检索记账后置 ✓；比例 ✓

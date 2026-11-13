# 增量16：COD 编码系统 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。
> 常设指令"一直往前做"持续有效；IM 相关暂缓（用户指令）。PRD §6.2 COD-01/02/03（P0 核心纵切）。

**Goal:** 可配置编码规则（码段拼装：固定字符/日期码/流水码，按类型绑定）+ 并发无重号取号（行锁）+ 对象创建自动赋码纵切（规则预览端点/赋码创建/前端接线）——SMB 最小闭环，COD-05/06（历史映射/规则变更审批）后置记账。

**短设计（Approaches）:**
- **A（选定）规则存 cm_type.config.cod_rule + 计数器表行锁**：`cod_rule = {prefix, date_fmt(yyyyMMdd/none), seq_digits(4)}`（JSONB 配置——ADR-0002 配置优先于代码，TypeRegistry 热生效）；`cod_counter(type_code, key, value)` 计数器表（key=日期段——按日流水防溢出），`UPDATE ... RETURNING` 行锁并发无重号（COD-02）；生成格式 `{prefix}{date}{seq:0>digits}`；`CreateCmd.obj_no=""` 时按规则赋码（COD-01/03 按类型绑定）。D（否）独立规则表：规则实体化留给 COD-06 审批管理增量，本纵切以类型配置承载（与 flow_tier 同模式先例）。
- B（否）PG sequence per type：日期段切换/重置/分段语义别扭；行锁计数器量级（小微日流水<1e5）足够。
- C（否）属性映射码/校验码：COD-01 全集的后半，属性映射需分类联动——本纵切先落固定/日期/流水三段，属性映射码记账（PRD P0 但依赖分类库 PART，先行不行）。

**Spec:** PRD §6.2 COD-01/02/03、ADR-0002（元数据驱动）、COD 预留（CreateCmd 注释）。

## Global Constraints

- 规则（cm_type.config.cod_rule JSONB，缺省无=类型不参与自动赋码）：`{"prefix":"DWG-","date_fmt":"yyyyMMdd"|"none","seq_digits":4}`；生成 `DWG-20261113-0001`。
- 计数器：`cod_counter(type_code text, key text, value bigint, PK(type_code,key))`（0014 迁移，core-model）；取号 `INSERT ON CONFLICT DO UPDATE SET value=value+1 RETURNING value`（原子 upsert，无显式行锁也并发安全）；value=0 起首个赋码=1。
- **赋码事务一致性**：PgObjectStore::create 内 obj_no 为空时先取号再插入（同事务——取号后插入失败会烧号，SMB 可接受，记账）；并发同规则取号无重号（唯一索引兜底 cm_object_ver_uk）。
- 网关：`POST /api/v1/docs` 增可选字段 `auto_code: "1"`（multipart 字段）——auto_code=1 时 obj_no 可空，服务端赋码；响应回带 obj_no。`GET /api/v1/meta/types/{code}/cod-preview?n=3`（认证即可，与 meta_get 同组）返回未来 3 个码预览（PRD 示例实时预览，不烧号——SELECT value+1..n 不落库）。
- 唯一性兜底：cm_object_ver_uk 并发兜底 → ObjNoDuplicate 409（既有）。
- COD-05/06 后置记账（历史码映射/规则变更审批）。

## Review Focus

1. 并发取号无重号：N 并发 create(auto_code) → N 个互异 obj_no 且连续 → 集成钉
2. 日期段滚动：key=日期，次日取号重起 0001 → 集成钉（SQL 改 counter key 模拟次日）
3. create 同事务赋码：auto_code 成功后 cm_object.obj_no=生成码；类型无规则+auto_code=1 → 400 明确报错
4. cod-preview 不烧号（取 n 次预览后真实 create 的 seq 仍从上次+1）→ 集成钉
5. 前端：上传对话框"自动编码"开关（auto_code 勾选时图号输入禁用）→ Playwright 钉
6. registry 热生效（改 cod_rule ≤3s 生效）→ 集成钉

## File Structure

```
crates/core-model/migrations/0014_cod_counter.sql
crates/core-model/src/cod.rs(新: CodRule parse/render/next) + lib.rs 导出
crates/core-model/src/store.rs(create 空码赋码分支) + tests/cod_it.rs
crates/api-gateway/src/routes/{docs.rs(auto_code), meta.rs(cod-preview)}
crates/plm-server/tests/e2e.rs(+3 钉)
frontend/src/api/docs.ts(auto_code) + pages/DocsList.vue(开关) + tests/e2e(+钉)
docs/superpowers/plans/2026-11-13-cod-coding.md；README；台账
```

## Tasks

### Task 1: cod 模块（规则解析/渲染/计数器）+ 单测/集成钉
- Step1 失败测试：rules 三段渲染/缺段默认/并发取号无重号（20 并发对拍连续无重复）/日期滚动；Step2 RED；Step3 实现（0014+cod.rs+create 分支）；Step4 GREEN；Step5 Commit

### Task 2: 网关端点 + e2e
- Step1 失败 e2e（auto_code 赋码成功/无规则 400/preview 不烧号/registry 热生效）；Step2 RED；Step3 实现；Step4 GREEN；Step5 Commit

### Task 3: 前端开关 + Playwright 钉
- Step1 实现（DocsList 开关禁用图号输入）；Step2 Playwright 钉（勾选→上传成功→列表出现自动码）；Step3 Commit

### Task 4: README/台账/全门禁/终审/合并/push/CI

## Self-Review

覆盖 COD-01/02/03 纵切 ↔ Review Focus 6 ↔ Task1-4 ✓；签名一致（CreateCmd 语义扩展向后兼容）✓；迁移 0014 唯一 ✓；属性映射码/历史映射/规则审批记账后置 ✓；比例 ✓

# 增量24 终审报告（AGL-05 行业模板包 + AGL-09 实施向导）

- 分支 feat/agl-template-wizard（基线 main @ d8aeaae）；审 commit：db664ea（T1+T2）、1d92c76（clippy 清偿）、8f5ee3e（T3 前端+修复）、c7615d6（T4 文档）
- 审法：fresh eyes——四 commit git show 全读 + 核心代码通读（template.rs / checklist.rs / mod.rs / state.rs / core-sec service.rs / registry.rs / cod.rs / mod-part service.rs / Workbench.vue / api/meta.ts）+ 独立跑测 + 真库探针（plm_rv24 @ :18099 全链路，含服务重启极端场景）

## 一、总判定：**FIX-THEN-MERGE**

2×C1（模板包毒化 Registry 至服务无法启动；Docker 构建必败）+ 2×I1。功能骨架（导入冲突检测语义/清单按序/权限门控/审计/隔离）经真库探针全部成立，测试基数真实（后端 170 全绿实测、Vitest 36 实测、clippy 0、fmt 过）；但模板包两条核心交付路径（新类型进入 Registry、Starter 格式示例）各埋一枚确定性炸弹，须修复后再并。

---

## 二、C1（必须修复，阻塞合并）

### C1-1 template.import 不校验 base_type/data_type，starter.json 自带非法值 "doc"——一次 merge 毒化 TypeRegistry，服务重启即无法启动（真库+重启实证）

**证据链（全部实测）：**
1. `templates/starter.json` 两个类型的 `"base_type": "doc"` —— 合法枚举为 `part|document|change|project|bom|generic`（`crates/core-model/src/types.rs:110-113`，BaseType::parse 无 "doc"）。
2. `crates/api-gateway/src/routes/template.rs:208-217`：merge 直接 `INSERT INTO cm_type(..., base_type, ...) VALUES ($3, ...)` 原样落库，无 BaseType/DataType 校验（`attrs[].data_type` 同面，`registry.rs:196-198` DataType::parse 同样硬错）。
3. 探针：merge 一个 `base_type:"doc"` 的新类型 → 200 applied → 服务日志每 ~5s 循环 `WARN 元数据热刷新失败（保留旧快照）error=内部错误: 非法 base_type: hot_type doc`（`registry.rs:219-221` 整快照重建遇毒行即整体失败）；`/healthz` meta_version 冻结在 5（DB 内 cm_meta_version 已到 10）→ **全部元数据热刷新死亡**（含 flow_tier 变更、后续任何合法元数据变更，直至重启或手工清行）；`GET /meta/types/probe_type` → 404（registry 永不刷新）。
4. **最坏面实测**：重启服务 → `main.rs:126 TypeRegistry::load(pool.clone()).await?` → reload 硬错 → 进程退出 `Error: 内部错误: 非法 base_type: hot_type doc`，healthz 无响应——**部署砖化，只能手工 SQL 摘除毒行恢复**。
5. template.rs:284 注释声称"cm_meta_changed 由触发器监听 UPDATE/INSERT——同 flow_tier 语义"：触发器侧属实（0001:153-160 `AFTER INSERT OR UPDATE OR DELETE`），但"同 flow_tier 语义"的热生效被毒行整体击穿。
6. e2e 盲区：`template_export_starter_import_roundtrip`（e2e.rs）只断言 DB 行内容，从不经 registry 读新类型、不重启——毒链对测试套件完全不可见（其 fixture 同样用 "doc"，正在示范毒化）。

**最小修复**：import_template 里对 `t.base_type`/`a.data_type` 做 `BaseType::parse`/`DataType::parse` 校验，非法 400（消息列出合法值）；`templates/starter.json` "doc"→"document"；补 e2e 钉：merge 新类型后 `GET /meta/types/{code}` 200（registry 刷新生效）。

### C1-2 Dockerfile 构建阶段缺 templates/ —— docker build 编译必败；待审声明"templates/ 不需 COPY"方向反了

**证据链：**
1. `crates/api-gateway/src/routes/template.rs:15`：`include_str!("../../../../templates/starter.json")` —— include_str! 是**编译期读取**，文件不在即编译失败。
2. `Dockerfile` 构建段②只有 `COPY crates ./crates`（templates/ 从未 COPY，本增量未改 Dockerfile，末次 aaee453/增量20）→ `/build/templates/starter.json` 不存在 → `cargo build --release -p plm-server` 报 couldn't read。
3. CI `.github/workflows/ci.yml:43-51` 有 docker build job（context: .）——本分支必红（本地跑测不经过此路径，故全绿未暴露）。
4. 运行态确实不需要该文件（已嵌入二进制）——但**构建态需要**；声明"templates/ 不需 COPY（include_str! 编译期嵌入）核对"把两层混为一谈。

**最小修复**：构建段②前加一行 `COPY templates ./templates`；ci docker job 随分支 CI 复验。

---

## 三、I1（合并前应修）

### I1-1 分类 cod_rule shape 不一致：starter/导入写 flat，唯一消费者要求 nested——导入的新分类编码规则功能性死亡（真库实证）

1. `templates/starter.json` class 级 `"cod_rule": {"prefix":...}`（flat）；`template.rs:269-278` 原样入库。
2. 消费者 `crates/mod-part/src/service.rs:169-171`：`class.cod_rule.and_then(CodRule::from_config)`；`core-model/src/cod.rs:20` `let obj = v.get("cod_rule")?` —— 要求列值为 `{"cod_rule":{...}}` 包装形（0015 种子即此形）。
3. 探针：导入 CLS-PROBE（flat rule）→ `POST /api/v1/parts {class_code:CLS-PROBE}` → **400 "分类 CLS-PROBE 未配置编码规则（cod_rule）"**（规则明明可见）；对照种子 CLS-ZZ → 201 `WL-ZZ-00001`。
4. 连带误导：class_tree `has_rule = cod_rule IS NOT NULL`（该分类树上显示"有规则"）；checklist coding 步 `cod_rule IS NOT NULL` 计数把死规则也算 done；导出按列原样回吐 → 同一字段两种 shape 随行来源漂移（探针输出实证：CLS-ZZ 导出 nested、CLS-PROBE 导出 flat），"导出与导入同 schema 往返自洽"在语义层不成立。
5. e2e 只钉 DB 行内容（`config->'cod_rule'` 断言），未钉"导入分类建物料自动赋码"功能面。starter 自身两分类永远冲突跳过，故 starter 导入路径未踩雷——雷在"行业包以同格式文件补"这一 AGL-05 主线上。

**最小修复**（任选其一+测试钉）：import 时归一（cod_rule 为对象且无 "cod_rule" 键 → 包一层）或 `CodRule::from_config` 兼容 flat；starter.json/计划文档 shape 统一；补 e2e：导入新分类 → part create 201 且 obj_no 前缀命中。

### I1-2 Workbench markDone 失败静默——catch 分支 import 了 extractApiError 但从不调用

`frontend/src/pages/Workbench.vue:52-60`：`catch (e) { import("../api/http").then(({ extractApiError }) => { void e; }) }` —— 导入后弃置，无任何 ElMessage。可达失败面：cutover 先于 training 点"标记完成"→ 400（前端不做按序约束）→ 按钮无响应无提示；403 同。错误兜底是死代码。**最小修复**：静态导入 `extractApiError` 与 `ElMessage`，catch 内 `ElMessage.error(extractApiError(e))`（与 importStarter 的 catch 同款）。

---

## 四、M 级（建议修复/记账，不阻塞）

### M1 importStarter 合并条件死代码；种子库上"导入 Starter"永不 merge
`Workbench.vue:64-69`：dry_run 响应 `applied_*` 恒空（服务端仅 merge 才 push，template.rs merge 门控），条件 `conflicts.length===0 || applied_types.length>0 || applied_classes.length>0` 实际退化为"零冲突才 merge"；种子系统上 starter 四项全冲突 → 按钮永远只展示冲突报告，永不应用。行内注释"无冲突**或有可应用项**：直接 merge"与行为不符。语义上可辩护（starter 即种子镜像，冲突跳过=无事可做），但按钮语义实为"预检"。建议：dry 后恒 merge（服务端冲突跳过本就安全）或按钮文案改"预检 Starter 包"，并删死条件；无冲突时 dry 报告被 merge 报告覆盖的语义正确（applied 0 不会出现）。

### M2 types auto 步阈值失义：种子即 done；migrations.md 声明"超出种子"与代码不符
`checklist.rs:69` `count(*) FROM cm_attr >= 5`；种子 drawing 7（0002）+ part 4（0015）=11 → 全新装即可 done（探针实证：种子态 coding=true 且 **types=true**，与任务书预期的"种子态其余 false"不符）。hint 文案"超出种子属性"与 `docs/migrations.md` "types attr≥5（**超出种子**）"均非代码所为。建议：阈值改为相对种子基线（如 >11）或排除种子 attr，或改文案为"属性 ≥5"；e2e 补钉初始态。

### M3 template.import 非事务
`template.rs:196-281` 类型/属性/分类逐条独立 INSERT，无 tx：中途失败（如包内 attr 同 code 撞 `cm_attr_type_code_uk`）留下半套状态且不记 audit（错误路径提前 return）。管理面低频 + 存在即 skip 使窗口小，建议包一个 tx（与 23 M2 惯例一致）。

### M4（记账）complete_step 读-改-写竞态
`checklist.rs:164-188`：done 集合 SELECT 后回写无锁，两管理员并发 complete 不同 manual 步可丢一行；重复 complete 幂等 204 但每重复一次多一行 audit。低风险记账，后续包 tx 顺带解决。

---

## 五、验证通过项（全部实证，非采信声明）

| 项 | 结果 |
|---|---|
| 后端测试 | `cargo test --workspace`（TEST_DATABASE_URL）exit 0，**170 passed / 0 failed**（42 套件，awk 汇总核对；首跑漏 env 的 16 个 bom_it 失败系评审环境变量缺失，非代码问题） |
| clippy / fmt | clippy --workspace --all-targets 0 警告；`cargo fmt --check` 过 |
| Vitest | 36/36 |
| Playwright | 16 test 声明清点属实（flow.spec.ts）；**评审未复跑浏览器两轮**（不在必做清单），时序断言静态审阅：audit 轮询 20×100ms 对 spawn 型留痕充分 |
| 门控矩阵 | export/starter/import 三路由挂 sec_mgmt（`mod.rs:115-133`，sec:manage）；checklist GET=require_authn、complete=require_perm("sec","manage")（`mod.rs:162-176`）；实测 viewer：export/starter/complete 均 403、checklist GET 200、无 token 401 |
| dry_run 不落库 | 实测 starter dry_run：全冲突报告（2 type_exists+2 class_exists）、applied 空、cm_type/cm_class 计数不变 |
| merge 落库 | probe_type+2 attr+CLS-PROBE 全落库，config 含 flow_tier+cod_rule 原样（DB 层） |
| 重复 merge 幂等 | 二次全冲突（type_exists/class_exists） |
| 坏包 400 | format 错/version≠1/types 形状错（对象充数组→serde 结构校验）/mode 非法 均 400；config 任意 JSON 200（sec:manage 信任面，与 flow-tier PUT 同层，声明相符） |
| 清单链 | 种子态八步返回（coding/types true，org/perm/flow/data/training/cutover false，types 见 M2）；建用户+授角色+上传+PUT flow-tier 后四 auto 翻转 true；跳序 cutover 400（文案含 training）；auto 步 complete 400；training→cutover 204/204；sys_param `impl_checklist_done=["training","cutover"]` 落库带 updated_by；audit impl-checklist.complete ×2、template.import ×4（merge 才记，dry_run 未记） |
| params 隔离 | GET /sec/params 恒 3 键（无 impl_checklist_done，`service.rs:460-465` 过滤生效）；PUT /sec/params/impl_checklist_done → 400 未知参数键（RANGES 白名单拒绝，语义正确） |
| OpenAPI | 活体 openapi.json 实测五端点全注册 + TemplatePackage/ImportReq/ImportResult/ChecklistStep 四 schema |
| cm_meta_changed 触发器 | 0001:153-160 `AFTER INSERT OR UPDATE OR DELETE`——INSERT 在监听内，注释该句属实；失效根因在快照重建遇毒行整体失败（C1-1），非触发器缺 INSERT |
| 环不可能 | import 侧 parent 须库内已存在（自引用探针→parent_missing 冲突，CLS-SELF 未落库）；同包内引用仅能指向前序已插行——DAG 保证；class_tree 对既有环也免疫（孤儿/环不上树，mod-part/service.rs:67-100） |
| SQL 参数化 | template.rs/checklist.rs 全部绑定参数，无串接 |
| Dockerfile 运行态 | starter 已嵌入二进制，运行镜像无需 templates——正确的一半；构建态缺失见 C1-2 |
| UserAdmin 无关核对 | 本增量未触碰 UserAdmin.vue（diff stat 核对）；增量23 M1 per-key `:min` 在位（UserAdmin.vue:202-204） |
| 回扣 23 | input-number min 完好；list_params 过滤与 set_param/param_i64 隔离语义实测正确。**注意**：587cd4d 提交信息称"M2 set_param 事务化(FOR UPDATE)"，但该 commit 实际只改 docs+UserAdmin.vue，`set_param`（core-sec/service.rs:472-492）至今仍为 SELECT+独立 UPDATE 无事务——23 的 M2 实为"建议未落地"（23 范畴，记账，不计入本增量） |
| 前端 wizardDone | steps 空时 false（`length>0 &&` 守卫在）；标记按钮 sec:manage 显隐正确（auth store getter 在位） |

---

## 六、探针环境留痕

- 库 plm_rv24（PG18@localhost），服务 `:18099`（PLM_ADMIN_PASSWORD=rv24-pass，临时 DATA_DIR），admin 登录→dry_run/导出/merge/消费者/清单链/403 链/OpenAPI/坏包→**重启砖化实证**→进程清杀+`DROP DATABASE plm_rv24`，端口已确认关闭，无残留 plm-server/vite。
- 未复跑：Playwright 两轮（16 声明仅静态清点）；docker build（静态推理+CI job 存在性，rustc include_str! 缺文件即编译错为确定性行为）。

## 七、修复清单（合并前）

1. C1-1：import 校验 base_type/data_type（400）+ starter.json "doc"→"document" + e2e 钉 registry 刷新。
2. C1-2：Dockerfile 构建段补 `COPY templates ./templates`。
3. I1-1：cod_rule shape 归一（import 包一层或 from_config 兼容）+ starter.json/文档统一 + e2e 钉导入分类建物料 201。
4. I1-2：markDone catch 真正报错（静态导入 extractApiError+ElMessage）。
5. M1/M2：死条件与按钮文案二选一；types 阈值或文案对齐（migrations.md 措辞同步）。
6. M3/M4：import 与 complete 包 tx（可随后续增量，先记账）。

修复后复审范围建议：C1-1/I1-1 补钉 + docker build job 绿 + 后端全量重跑即可，其余维持本报告。

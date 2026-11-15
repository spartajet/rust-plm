# 增量19 终审报告（WF-03 待办中心 + 审批入口）

- 分支 `feat/todo-center`，基线 `main @ 92e783c`；受审 commit：`632e3dd`（三盒）/ `39c24db`（意见）/ `c9bec27`（批量）/ `03f4e04`（前端+文档）。
- 终审人：fresh-eyes 独立 reviewer（未参与开发）。日期 2026-11-15。
- 方法：代码逐行对账 8 条声明 + 四 commit `git show` 对账 + 全量测试实证 + 真库（18095 探针服务）手工复现 + 历史易犯错误专项 D 逐项查证。不信任任何声明，只认可复现实证。

## 一、声明逐条对账（全部实证）

| # | 声明 | 判定 | 证据 |
|---|---|---|---|
| 1 | GET /todos 三盒（docs:read 门控） | ✅ | 门控 `routes/mod.rs:38-43`（docs_r 组 require_perm("docs","read")）；Todo 盒条件 `service.rs:347-349`（state IN review/checked + secret_level≤$1 + `updated_by IS DISTINCT FROM $2`）；Done 盒 `service.rs:378-407`（audit_log username=me + `LC_ACTIONS`(:300) + object IS NOT NULL + LATERAL 联查当前态/名）；Mine 盒 `service.rs:351`（created_by=me + 密级过滤）；均 `DISTINCT ON (obj_no)` 最新活版本（`LATEST_CTE`:322-327，type='drawing' + deleted_at IS NULL）。真库探针：admin 见件且 submitter=提交人、editor 自我排除、limit0 用户被密级过滤、非法盒名 400、无 token 401 |
| 2 | 六 lc 端点 Option\<LcBody\> + 意见落审计 | ✅ | `docs.rs:481-502`（lc_endpoint! 宏，`body: Option<Json<LcBody>>` 向后兼容）；trim 空串视缺失 `docs.rs:388`；审计 `docs.rs:403-410`（action=ev.as_str()、object=obj_no、detail {from,to,comment}）。e2e 无 body approve 200 钉（e2e.rs 三盒测试内）+ 真库探针复现 |
| 3 | require_comment 开关 | ✅ | 开关读 `docs.rs:360-367`（cm_type.config.require_comment）；approve/reject 强制 `docs.rs:389-395` → `DomainError::CommentRequired`（error.rs:79/120/148，HTTP 400，码 MDL-LCS-COMMENT-REQUIRED）；PUT flow-tier 双可选至少一项 `meta.rs:73-90`，jsonb patch 热生效 `meta.rs:114-128`；IM 豁免但写审计行（via:"im"）`im.rs:118-131`。真库探针：开→204、无意见 400、空串 400、带意见 200、submit 不受限、关→200 可逆、config 回读 |
| 4 | POST /docs/batch-approve | ✅ | `docs.rs:445-474`：空批 400 / 上限 50 / 逐项 transition_once 不中断 / `error: e.code()` 稳定码；门控 docs_lc_approve 组 `mod.rs:52-60`。真库探针：editor 403、混合批次 [ok:released / MDL-OBJ-NOT-FOUND(越密级) / MDL-OBJ-NOT-FOUND(不存在)]、审计仅成功件落行 |
| 5 | 上传链穿 actor | ✅ | `upload_staged(staged: StagedUpload, actor)` `service.rs:103-127`；upload_doc 传 `Some(user.id)` `docs.rs:146-158`；upload() 便捷路径 None `service.rs:69-90`；`core-model/store.rs:202-210` INSERT `VALUES (..., $5, $5, $5)`（owner/created_by/updated_by 同源）。真库探针：上传后三列同=editor uuid |
| 6 | 前端 /todos | ✅ | `Todo.vue` 三 tab（el-tabs v-model）+ 批量批准弹窗 + 行点击跳详情；`api/todos.ts`（listTodos/batchApprove/summarizeBatch）；`router/index.ts:11`；菜单无 v-if 全员可见 `MainLayout.vue:22`；Vitest 27 全过（实证）；Playwright "todo center" 钉过（flow.spec.ts:399-477，本轮 3 次尝试均过） |
| 7 | 文档 | ✅ | README curl 组（待办/意见/批量/开关四段）；部署文档端点表三处更新；评审报告 WF-03 ⬜→✅（含"WF-05 仍缺"克制表述）；migrations.md "增量19 无新迁移"+四条裁定；计划文档对账台账 Task1-5 回填 |
| 8 | 测试基线 | ✅ | `cargo test --workspace`：**155 passed / 0 failed / 0 ignored**（19 套件合计逐一累加=155，与声明一致）；clippy --workspace --all-targets **0 警告**（exit 0）；Vitest **27**；Playwright 11 钉存在、WF-03 钉过（见 M1-3 环境注记） |

四 commit `git show --stat` 与声明文件清单一致；"后端153/154/155"递增与每 commit 恰 +1 个 e2e 测试函数（todos_center_three_boxes_and_abac / lifecycle_comment_record_and_require_switch / batch_approve_partial_results_and_guards，e2e.rs:2155/2331/2502）吻合。

## 二、历史易犯错误专项（D 清单逐项）

| 项 | 判定 | 证据 |
|---|---|---|
| OpenAPI 完整性 | ✅ | `state.rs` paths(:78-79) 注册 list_todos + batch_approve；schemas(:85-88) 注册 LcBody/BatchApproveReq/BatchApproveResp/TodoItemDto；活体 `GET /api/docs/openapi.json` 实测两 path + 四 schema 全在；与 `routes/mod.rs` 实际路由表逐一比对无缺漏 |
| ABAC 一致性 | ✅* | todo/mine 盒密级过滤（探针实证：limit0 用户看不到 secret2 件）；批量审批越密级 → 逐项 `error=MDL-OBJ-NOT-FOUND`（探针实证，无 403 存在性泄露）。*例外：done 盒无密级过滤 → I1-1 |
| Dockerfile 桩层 | ✅ | diff 无 Dockerfile；无新 crate（workspace members 仍是 glob `crates/*` + `spikes/*`；Cargo.toml 仅 utoipa 加 chrono feature，不涉及桩层清单） |
| audit 豁免清单 | ✅ | /api/v1/todos GET 不在 SKIP（audit_mw.rs:38）→ 产生 http 兜底行（object=NULL，action=http）；已办盒按 action IN 六动作 + object IS NOT NULL 过滤，探针实证 editor 已办盒仅见 submit 行，http 行不干扰 |
| 迁移注册表 | ✅ | `git diff main..feat/todo-center --name-only` 中 migrations 仅 `docs/migrations.md`（文档）；crates/*/migrations 零改动 |
| 前端 dead code | ⚠ | `Todo.vue:45-47` switchBox 冗余（el-tabs v-model 已切换；同值赋值不触发 watch → 无双触发 bug，仅死代码）→ M1-1；flowUi 导入（stateLabel/stateTagType）模板中均用到 ✅ |
| clippy too_many_arguments | ✅ | upload_staged 收敛为 `(StagedUpload, actor)` 两参；clippy 全绿实证 |
| maker-checker 交互 | ✅ | transition_once（docs.rs:397-400）→ DocService::transition（service.rs:185-197）→ LifecycleStore::transition_with_file；会签守卫在 store 层（store.rs:117-124：checked→approve 排斥 updated_by/created_by，actor=None fail-closed），批量不绕过；strict 批量同人复核将逐项 `MDL-LCS-MAKER-CHECKER`。且 todo 盒 `updated_by≠me` 与会签天然呼应（计划 D1 语义钉成立） |

## 三、测试基线实证

- 后端：`TEST_DATABASE_URL=postgres://…/postgres cargo test --workspace`（因探针服务锁 target/debug/plm-server.exe，改用 CARGO_TARGET_DIR=target-review 独立目录，结论等价）：**155 passed，0 failed**，0 ignored。与声明"后端 155"一致。
- clippy：`cargo clippy --workspace --all-targets`（独立 target 目录）0 warning / 0 error。
- 前端：`npx vitest run`：**27 passed (27)**。
- Playwright：11 个 test 声明；本轮全量 10 过 1 失败——失败者为**增量17 遗留** parts 测试（`flow.spec.ts:23`，60s 超时 + "browser has been closed"，3 轮复现，断言位点两处不同）；该测试代码本分支零改动（diff 仅追加 todo center 测试），失败模式属 `playwright.config.ts` 台账明文的"本机长命 browser 进程不稳"家族 → 环境性 flake，非本增量回归（M1-3 记账）。**WF-03 钉（"todo center"）全部尝试通过**。

## 四、真库探针记录（plm_e2e_todo @18095）

1. /healthz `{"status":"ok"}`；admin/editor 登录正常。
2. 建 editor（rvw_editor19）+ 授权 201/204；editor 上传 `DWG-RVW19-1` 并 submit 201/200；**上传回包 created_by=updated_by=owner=同一 editor uuid**（声明5 实证）。
3. admin `GET /todos?box=todo`：见 RVW19-1，`submitter=rvw_editor19`、state=review ✓；editor 视角 box=todo **不含自己的提交** ✓；editor box=mine 含 RVW19-1 ✓。
4. ABAC 隔离：第二 editor（rvw_ed2_19）提交 `DWG-RVW19-2` 后 psql 提密至 2 → limit0 用户 box=todo 不见（密级过滤）；admin（limit3）两件均见 ✓；无 token 401。
5. admin approve 带 comment → 200 state=released；audit_log 行：`action=approve, object=DWG-RVW19-1, detail: from=review to=released comment=final-review-ok-19, username=admin` ✓。
6. box=done：admin 见 approve 行（LATERAL 联查 state=released + detail 完整）；editor 见自己 submit 行（http 兜底行被 action 过滤排除）✓；box=todo 中 RVW19-1 消失 ✓。
7. batch-approve：editor 403 MDL-SEC-FORBIDDEN；空批 400；51 件 400；混合批次 `[RVW19-3 → ok/released] [RVW19-2(secret9) → error MDL-OBJ-NOT-FOUND] [RVW19-NOPE → error MDL-OBJ-NOT-FOUND]`，不中断批次；audit 仅成功两件各一行 ✓。
8. require_comment：PUT `{require_comment:true}` → 204；热生效后无意见 approve 400 **MDL-LCS-COMMENT-REQUIRED**；`"comment":"   "` 400；带意见 200；开关开启下 submit 200（不受约束）；`{require_comment:false}` → 无意见 approve 200（可逆）；psql 回读 `cm_type.config.require_comment=false` ✓。

探针残留：plm_e2e_todo 库遗留 `DWG-RVW19-*` 五件与 `rvw_*` 两个用户（编号唯一不冲突，与该共享探针库既有残留同口径，未清理；audit_log append-only 未触碰）。

## 五、分级发现

### C1（必须修——语义/安全/声明失实）

无。8 条声明全部与代码/活体行为可对账，未发现声明失实、越权泄露或绕过守卫。

### I1（应该修——一致性/契约）

- **I1-1 已办盒未做密级过滤，与代码内"三盒 ABAC"表述不一致且裁定未记档**
  - 证据：`crates/mod-doc/src/service.rs:378-407`（Done 盒 SQL 无 secret_level 条件；LATERAL 亦未过滤）vs `service.rs:330`（"三盒查询（ABAC 密级过滤；分页）"）、`routes/todos.rs:2`（"数据面 ABAC 密级过滤在 DocService::todos 内"）、commit 632e3dd message"GET /todos(box=todo|done|mine) docs:read 门控+ABAC 密级过滤"。任务侧声明 1 的表述精确（只给 todo 盒写了密级条件），故不构成声明失实；但语义对外暴露面是"todo/mine 过滤、done 不过滤"，且 `docs/migrations.md` 增量19 四条裁定未含此立场。影响有限：done 盒只返回**本人**历史动作行，风险是对象事后提密后本人仍可见其当前名/态。
  - 最小修复（二选一）：(a) LATERAL WHERE 增加 `AND l.secret_level <= $1`（todos() 传入上限，DELETE 行为不变）；或 (b) migrations.md 裁定区补一条"已办盒=本人动作史，不做当前密级过滤"，并把 service.rs:330 / todos.rs:2 的"三盒 ABAC"注释收窄为"todo/mine 盒 ABAC"。

### M1（建议——质量）

- **M1-1 `switchBox` 死代码**：`frontend/src/pages/Todo.vue:45-47` + 模板 `:97`。el-tabs `v-model="box"` 已完成切换；同值 ref 赋值不触发 watch，无双触发 bug，但函数冗余。建议删除 span 上的 `@click="switchBox(b)"` 与函数本体。
- **M1-2 已办盒 LATERAL 无类型过滤，跨类型 obj_no 碰撞可显示错对象**：`service.rs:385-390` LATERAL 仅 `WHERE obj_no = a.object AND deleted_at IS NULL`；而 obj_no 唯一性是 per-type（`cm_object_ver_uk` ON (type_id,obj_no,major_ver,minor_ver)；`core-model/store.rs:170-177` 查重也按 type_id）。若 part 与 drawing 同号，DISTINCT ON 结果不确定、可能展示错名/错态。最小修复：LATERAL 加 `AND type_id = (SELECT id FROM cm_type WHERE code='drawing')`（与 LATEST_CTE 同口径）。
- **M1-3 Playwright parts 测试本机环境 flake 记账**：本轮 10/11，失败为增量17 遗留测试（代码未动、60s 超时+browser closed 家族）。建议在对账台账记"该钉在本机偶发，CI/他人环境复跑为准"，避免后续终审重复归因。
- **M1-4 `upload()` 便捷路径 actor=None**：`service.rs:69-90` 创建的对象 created_by 为 NULL，不进任何人的"我发起"盒。仅测试/CLI 在用（生产 upload_doc 已穿 actor），留档即可；若未来 CLI 复用须强制传 actor。

## 六、总判定

**PASS** —— 无 C1；I1-1 为一致性收口（一行 SQL 条件或一条裁定二选一，可与 M1-1/M1-2 合并为一次小修复轮），不阻塞合并；建议合并前顺手清偿 I1-1 与 M1-1/M1-2（合计改动 <10 行），M1-3/M1-4 记台账。

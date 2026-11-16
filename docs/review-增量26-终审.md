# 增量26（DOC-03 检入/检出独占锁）终审报告

- 评审人：独立终审 reviewer（fresh eyes）
- 分支：`feat/doc-checkout-lock`（基线 main @ 22d5f91；增量 0e168cc 计划 + 675b946 T1 + 5235654 T2+T3）
- 日期：2026-11-16
- 结论：**FIX-THEN-MERGE**（C1×3 / I1×1 / M×3）

## 0. 验证手段（全部真跑）

| 手段 | 结果 |
|---|---|
| `git show` 三 commit + 全量代码审读（service.rs 锁四方法 / doc_lock.rs / docs.rs transition_once / mod.rs 拦截层 / DocDetail.vue / sec_params.rs / core-version store.rs） | 完成 |
| `cargo test --workspace`（CARGO_TARGET_DIR=target-rv26，TEST_DATABASE_URL 真库） | **172 passed / 0 failed** ✓（与声明一致） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 警告 ✓ |
| `cargo fmt --check` | 通过 ✓ |
| `vitest run` | **36/36 passed** ✓ |
| Playwright（fresh 库 + 服务 18099 + vite 注入 VITE_API_BASE，`--reporter=line`） | **两轮均非全绿**：R1=15 passed+1 flaky+**1 failed**；R2=16 passed+**1 failed**（同一用例确定性失败，见 C1-3） |
| 真库探针（plm_rv26 专库，跑毕 DROP + 进程清理） | 见 §3 |

## 1. C1（合并阻断：声明失实 / 安全面 / 击穿既有钉）

### C1-1 「四端点全部 ABAC 越级 404」声明失实——checkin / force_unlock 两端点缺 ABAC，越级泄露实证

- **声明**（675b946 提交信息）：「四端点（…）+ABAC 越级 404」。
- **代码**：`crates/api-gateway/src/routes/doc_lock.rs`——`get_lock`（L36-46）与 `checkout_doc`（L78-88）有 `secret_level > user.max_secret_level → ObjNotFound` 检查；**`checkin_doc`（L118-137）与 `force_unlock_doc`（L145-165）没有任何 ABAC 检查**，直接落 service。
- **真库实证**（DWG-RV26-1 提密 secret_level=2，低密级 editor max=0）：
  - `GET /lock` → **404** ✓；`POST /checkout` → **404** ✓；
  - `POST /checkin` → **409 `MDL-DOC-LOCKED`，泄露持有者名与到期时间**（`对象已被 rv26ed 检出锁定（至 2026-11-17T00:44:45…）`）——与同文档 GET lock 404 自相矛盾，存在性+元数据双泄露。
- **e2e 缺口**：越级钉（e2e.rs L4111-4120）只钉 lock GET 与 checkout 两端点，checkin/DELETE 无越级钉——正是漏改的两端。
- **缓解（不豁免）**：force_unlock 走 sec:manage 层，现种子矩阵仅 admin 角色持有（`sec_role_perm` 实查），admin 密级上限=3，当前**现实路径**够不着；但 checkin 是 editor（docs:write）即可达的**现实泄露**，且违反本项目既有不变量（物料读端点 docs:read 门控=增量17 C1、transition_once ABAC=终审 I1 同族）。
- **最小修复**：checkin_doc / force_unlock_doc 复制同款三行 ABAC 预检（detail+提密比对→ObjNotFound）；e2e 补两端点越级 404 断言。修复注意边界：检出后对象提密、持有者低密级时将 404 无法自检入——锁由到期惰性回收/force 兜底，可接受（台账记一句即可）。

### C1-2 「评审报告 DOC-03 ⬜→🟡」声明失实——文件根本没改（第三次同类事故）

- **声明**（5235654 提交信息 + 计划台账）：「评审报告 DOC-03 ⬜→🟡」。
- **事实**：`git log -- docs/review-需求符合度评审.md` 最后一次触碰是 **2cf3006（增量25 修复轮）**；5235654 的 diff 根本不含该文件；当前正文 L62 仍为 `| DOC-03 检入/检出 | ⬜ | 无独占锁机制 |`。
- 定性：与增量25 C1-2（python replace 静默 no-op 第五次）同一失败模式再犯——**写声明前未回读落点文件**。声明-代码对账红线直接击穿。
- **最小修复**：实改 L62 → 🟡（证据=0018 独占锁+四端点+revise 拦截+UI 横幅；缺口=离线包后置/定时器后置/参数 PUT 白名单缺（见 I1-1）），并回读验证。

### C1-3 「Playwright 17×2 两轮全绿」声明失实——checkout_hours 种子击穿既有 SYS-04 钉，两轮确定性失败

- **声明**（5235654 提交信息 + 台账）：「Playwright 钉 17×2 全绿」。
- **真跑实证**（fresh 库、正确接线 `VITE_API_BASE`/`E2E_API_BASE`/`E2E_ADMIN_PASSWORD`）：
  - R1：15 passed + 1 flaky（parts create）+ **1 failed**；R2：16 passed + **1 failed**——同一用例两轮必死：
  - `frontend/tests/e2e/flow.spec.ts:739`：`await expect(params-table 行).toHaveCount(3)`——增量26 种子 checkout_hours 后 GET /sec/params 返回 **4 行**（后端 e2e 自己都钉了 `arr.len()==4`，e2e.rs L3369），面板渲染全部行 → `Expected: 3, Received: 4`。
  - 新增的 checkout lock 用例本身两轮通过 ✓。
- 定性：本增量**击穿了一条既有钉**且未发现——「17×2 全绿」在本分支 fresh 环境上不可能成立，声明与事实不符（T2 的验证过程存疑）。
- **最小修复**：flow.spec.ts:739 钉改 4（建议 `toHaveCount(4)`+含 checkout_hours 行断言，防再次静默漂移）；顺带 `frontend/src/api/sec.ts` PARAM_LABEL 补 `checkout_hours: "检出时长（小时）"`（现面板裸显英文键，M3 并入本修复）；重跑 17×2 全绿后回填声明。

## 2. I1（合并前应修）

### I1-1 checkout_hours 参数「可见不可调」——PUT /sec/params 值域白名单 RANGES 未收录，实测 400

- `crates/api-gateway/src/routes/sec_params.rs:24-28`：`RANGES` 仅 3 键（password_min_length/lock_threshold/lock_minutes）。PUT `/api/v1/sec/params/checkout_hours` → **400 `未知参数键: checkout_hours`**（真库实证）。
- 参数种子（0019）+ GET 四行断言 + checkout 读参（doc_lock.rs L89-93 `param_i64("checkout_hours",24)`）全接了，唯独**管理面 PUT 没接**——SYS-04 参数中心对该参数形同虚设；部署文档端点表「安全参数 PUT（…与时长）」对不上。后端 e2e 也只断言 GET 行数，未钉 PUT。
- **最小修复**：RANGES 增 `("checkout_hours", 1, 8760)`；e2e 补「PUT 1 → checkout until≈+1h」钉（本终审探针已验证 checkout 侧 until 计算正确，默认 24h：00:42检出→次日00:42到期 ✓）。

## 3. M（记录在案，可随后续增量）

### M1 checkout 端点 OpenAPI 注解状态码失实：注 400，实为 423

- `doc_lock.rs:70` 注解 `(status = 400, description = "非 working 态")`；实际 `ObjStateForbidden` 映射 **423**（core-model/error.rs:145；真库实证 submit 后 checkout → 423 MDL-OBJ-STATE-FORBIDDEN）。TQ-02 快照门禁不比对状态码所以未拦。改注解为 423 一行。

### M2 checkout 持锁预检与 INSERT 存在 TOCTOU：并发双检出败者得 500 而非 409

- service.rs L467-488：先 SELECT held 再 INSERT，无 `ON CONFLICT` 兜底映射。并发窗口内两请求同时过预检 → PK `doc_lock_pkey` 保证**独占性不破**（第二 INSERT 唯一冲突失败），但败者收到 sqlx 原始错误 → 500，而非 409 DocLocked。语义安全、仅错误形态降级。建议 `ON CONFLICT (object_id) DO NOTHING` + 0 行时回落查询持锁者转 409。
- sweep_expired 并发（任务点名项）：无 advisory lock，但 DELETE BY 谓词天然幂等，并发 sweep 无害 ✓（已核）。

### M3 PARAM_LABEL 缺 checkout_hours 展出名

- `frontend/src/api/sec.ts:87-91` PARAM_LABEL 三键，checkout_hours 回退裸键名。纯外观。与 C1-3 修复同一提交顺手补。

## 4. 专项裁定（任务 D 清单逐项）

| 专项 | 裁定 |
|---|---|
| 迁移注册表 0018/0019 | ✓ 已录（migrations.md L26-27）；`git diff main..HEAD -- crates/*/migrations/` 仅新增 0018/0019 两文件，**只增不改** ✓ |
| 0019 归属域教训 | ✓ 成立：执行序 core-model→mod-doc(0018)→core-sec(0019)（main.rs L93-104），sys_param 归 core-sec，跨域 INSERT 必 42P01——拆分裁定正确并已入台账 |
| **锁对象跨 major 孤儿窗口**（任务重点） | **不存在**。关键事实：Revise 是**同 id 就地升版**——`core-version/src/store.rs:129-134` `UPDATE cm_object SET major_ver=major_ver+1 … WHERE id=$1`，id 不变；因此 checkout 锁的 object_id 在 revise 后仍是同一行。真库实证链：ed 检出→submit→approve（released+持锁）→他人 revise 409 ✓→持有者 revise 200（major 3→4 就地）→SQL 确认锁行仍指向同 id（major_ver=4）→checkin **204**。`get_latest` 各锁方法一致指向该行，无失联窗口 |
| checkout 本人再检出 | 409 DocLocked（无本人幂等）——语义可自洽（独占+到期信息回显；UI 横幅已隐藏按钮），不算缺陷，记录在案 |
| checkout 非 working 态 | 实为 **423**（非 400，见 M1）；拦截语义本身 ✓ |
| checkin 无锁/他人持有 | 无锁 400 `对象未被检出` / 他人持有 409 DocLocked ✓（真库双证） |
| force_unlock 无锁 | 204 + `had_lock=false` 记 audit ✓（doc_lock.rs L151-163 + service.rs L521-530，语义自洽） |
| 拦截面位置 | ✓ transition_once 内、**ABAC 之后**（docs.rs L484-495：先密级 404 再 assert_not_locked）；仅拦 Revise，submit/approve 不拦=台账裁定 ✓；upload 同号复用 store.rs L170-183 ObjNoDuplicate 天然防覆盖，台账收敛理由成立 ✓ |
| sweep 惰性回收 | ✓ 真库实证：置过期→他人 checkout 200、旧行被清（count 1=新行） |
| audit 三行 | ✓ `doc.checkout`×4 / `doc.checkin`×2 / `doc.force-unlock`×1 实查；fire-and-forget（tokio::spawn）与既有埋点同型；文档/前端无冲突 action 名 |
| OpenAPI 四端点 | ✓ 活体 openapi.json 实查：checkout/checkin POST、lock GET+DELETE、LockDto schema 全注册；快照基线 +20 行同步（TQ-02 首秀 Story 与 diff 对账吻合：4 端点+LockDto） |
| 回扣增量25 | ✓ 172 全绿含 openapi_snapshot_gate；门禁机制真实（分级 panic+UPDATE_OPENAPI_SNAPSHOT 流程在码） |
| 回扣增量24 | ✓ import 白名单/毒包钉全绿（172 内） |
| 前端按钮显隐矩阵 | ✓ 模板实读：检出=`!locked && state==='working' && canWrite`；检入=`locked && by===username`；强制解锁=`locked && canManageSec`；lockedByOther=`locked && by!==username`；他人橙 lock-banner/本人绿 lock-banner-self——与声明、e2e 三点全对齐；v-if 误用 ui.revise 的自曝修复属实（现用 state==='working'） |
| param_i64 | ✓ SecService::param_i64（core-sec service.rs L440，增量23）存在并被 checkout 正确消费（缺省 24） |
| 前端 Vitest 36 | ✓ |

## 5. 判定

**FIX-THEN-MERGE。**

修复清单（合并前置）：
1. **C1-1**：checkin/force_unlock 补 ABAC + e2e 越级钉（doc_lock.rs 两处三行 + e2e.rs）。
2. **C1-2**：实改评审报告 L62 DOC-03 ⬜→🟡 并回读。
3. **C1-3**：flow.spec.ts:739 钉 3→4（含 checkout_hours 行断言）+ 重跑 17×2 全绿 + 回填声明。
4. **I1-1**：RANGES + checkout_hours 值域 + PUT e2e 钉。
5. M1/M3 顺手（一行注解 + 一行 label）；M2 可台账后置（建议随下轮补 ON CONFLICT）。

根因备注：三条 C1 里有两条（C1-2/C1-3）属「声明未经验证」——写声明前回读落点文件、跑绿后再写全绿，是本轮最便宜的流程修复。

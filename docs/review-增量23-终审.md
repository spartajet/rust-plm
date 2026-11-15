# 增量23 终审报告（防爆破登录锁定 + SYS-04 最小参数中心）

- 审阅人：独立终审 reviewer（fresh eyes，未参与开发）
- 分支：`feat/bruteforce-lock`（基线 main @ 4895c01，增量22 合并后）
- 待审 commit：615e47f（T1+T2）/ 1a6aeb9（T3）/ e855aa0（T4）
- 审阅方式：git show 三 commit 对账 + 代码精读 + 本机复跑全量测试 + 真库（plm_rv23）API/SQL 探针 + Playwright 两轮实跑

---

## 一、总判定

**PASS**（C1 = 0；I1 = 0；M1×3 + 观察项×4，均为建议级，不阻塞合并）

声明与代码可对账，红线未破。全部基线指标实证复现：后端 **168 passed / 0 failed**（对 "test result" 行逐条求和核实）、clippy **0**、`cargo fmt --all --check` 过、前端 Vitest **34/34**、Playwright **15** test 两轮实跑全绿（第一轮 14 passed + 1 flaky 经 retries=2 重试通过——flow.spec.ts 头注台账既有的本机 browser 进程不稳裁定，第二轮 **15/15 零 flaky**）。真库探针 15 项全过，增量21/22 修复轮回扣点完好。

---

## 二、逐项对账（声明 → 证据）

### T1+T2（615e47f）✅

| 声明 | 证据 |
|---|---|
| 迁移 0017：sec_user 加 failed_attempts/locked_until | `crates/core-sec/migrations/0017_lockout_params.sql:4-7`（`ADD COLUMN IF NOT EXISTS`，幂等） |
| 迁移 0017：sys_param 表 + 三种子 8/5/15 | 同文件 `:10-21`（`ON CONFLICT (key) DO NOTHING` 幂等；改值只走端点留痕） |
| 锁检查在验证前（锁定期内不烧 CPU） | `crates/core-sec/src/service.rs:531-542`：`SELECT locked_until ... AND locked_until > now()` 独立查询，命中即返回 423——控制流先于 `:543` 的 argon2 verify |
| 423 MDL-SEC-ACCOUNT-LOCKED 带剩余分钟 | `service.rs:539` `mins = ((until - now()).num_minutes()).max(1)`；`core-model/src/error.rs:77`（变体，Display 含 "剩余 {0} 分钟"）、`:125`（code）、`:163`（http 423）。**max(1) 边界实测**：探针将 locked_until 置于 30 秒后 → 响应 "剩余 1 分钟"（num_minutes()=0 被钳到 1）✅ |
| 失败计数原子性 | `service.rs:496-500`：`UPDATE ... SET failed_attempts = failed_attempts + 1 RETURNING failed_attempts`（单语句行级原子，无 read-modify-write 竞态） |
| 阈值比较 >= | `service.rs:501` `if i64::from(attempts) >= threshold` |
| 锁定时 failed_attempts 归零（防到期后一触即重锁） | `service.rs:502-508`：`SET locked_until = now() + make_interval(mins => $2::int), failed_attempts = 0`（同一 UPDATE；`$2::int` 为计划台账记载的 bigint→int 签名修复） |
| 成功清零 | `service.rs:569-572`：`SET failed_attempts = 0, locked_until = NULL` |
| 未知/禁用恒 401 + 耗时混淆不变 | `service.rs:560-563`：`_ =>` 臂（未知 + 禁用共用）`fold_unknown_user(password)` DUMMY_HASH 等价 argon2 后统一 `bad()`——终审 I3 语义未动 |
| param_i64 热读取 | `service.rs:440-447`：每次实时查表，`trim().parse().ok()` + 缺省兜底（行被删/坏值不致命） |
| create_user 密码策略 | `service.rs:231-235`：`param_i64("password_min_length", 8)` → `password.len() < min_len` → `SecPasswordPolicy`（400） |
| 网关去硬编码 <8 | `crates/api-gateway/src/routes/sec_users.rs:50`：仅 `is_empty()` → FIELD-REQUIRED；全仓 grep `len() < 8` 仅剩 `webhooks.rs:74`（webhook secret，与密码无关）——单一路径达成 |
| GET/PUT /api/v1/sec/params | `crates/api-gateway/src/routes/sec_params.rs`（新文件 117 行）；注册于 `routes/mod.rs:104-112`，**在 sec_mgmt Router 内**，`:114-117` `.layer(require_perm("sec", "manage"))` 门控 ✓ |
| 值域白名单 8-64 / 3-20 / 1-1440 | `sec_params.rs:24-28` RANGES 常量；`:87-94` 越界 400（消息含合法值域与实际值）；未知键 400（`:87-91`，service 层 `:478-480` 双重兜底） |
| audit sec-param.change from/to | `sec_params.rs:108-113`：`st.audit.record(user, "sec-param.change", key, {"from": old, "to": new}, trace)`；`set_param` 返回旧值（`service.rs:474-489`） |
| OpenAPI 注册 | `crates/api-gateway/src/state.rs`（diff：paths 加 list_params/set_param；schemas 加 SysParamDto/SetParamReq） |
| e2e 两钉 | `crates/plm-server/tests/e2e.rs:3254`（bruteforce：7位400/连败401×5/锁定后正确密码423含"分钟"/SQL 到期解锁清零/阈值热生效 2 败即锁/未知用户恒401）；`:3356`（params：三种子/viewer 403/min_length=12 热生效建号/越界+未知键 400/audit from-to 断言/还原） |

### T3（1a6aeb9）✅

| 声明 | 证据 |
|---|---|
| api listParams/setParam/paramLabel + 2 单测 | `frontend/src/api/sec.ts:73-95`；`frontend/tests/unit.spec.ts:269-277`（三键映射 + 未知键回退，恰 2 it） |
| UserAdmin SYS-04 面板（行内编辑/保存/修改人列） | `frontend/src/pages/UserAdmin.vue:189-228`（el-card `data-test="params-card"` + el-table 三列 + `param-input-*`/`param-save-*` 钉位 + updated_by 列）；挂载 `:96-99` |
| paramDraft Number(value) NaN 防御 | `:71` `Number(p.value)` 转换；`:79` `!Number.isInteger(v) || v <= 0` → "须为正整数" warning（NaN 被 `Number.isInteger` 拦截，不发请求）✅ |
| 面板无权限静默 | `:66-74` `loadParams` catch 空吞（403 时面板空表不渲染数据）；viewer 打开 /admin/users 的 403 三连系用户管理页既有 sec:manage 语义（e2e `flow.spec.ts:359/369` 已钉 viewer 菜单无"用户管理"项）——可接受，维持原裁定 |
| Playwright 钉纯 UI 无全局副作用 | `frontend/tests/e2e/flow.spec.ts:733-756`：lock_minutes 15→20 保存回显→还原 15；`:744-745` 注释自证"曾改 min_length+建号复验与并行 user-admin 全局参数竞争实测撞车，裁定改纯 UI"，策略生效语义由后端 e2e 钉住——裁定合理（全局参数在并行测试下确属竞争资源，与开发侧台账一致） |

### T4（e855aa0）✅

README（防爆破+安全参数 curl 组、22→23 增量）、部署文档端点表新增"安全参数"行（sec:manage）、`docs/review-需求符合度评审.md` 两处 ⬜→🟡（SYS-04/密码策略行，验证码 ⬜ 台账后置如实）、`docs/migrations.md` 0017 注册 + **五裁定**（423 枚举权衡/参数热生效/验证码后置/手动解锁+密码定期更换后置/网关去硬编码）、计划台账回填（含 make_interval 签名修复、Playwright 竞争撞车实录）。均与代码一致。

---

## 三、测试复跑（本机实证）

| 项 | 声明 | 实测 | 结论 |
|---|---|---|---|
| cargo test --workspace（TEST_DATABASE_URL） | 168 全绿 | 42 个 "test result: ok" 求和 = **168 passed / 0 failed**（e2e.rs 35 含两新钉） | ✅ |
| cargo clippy --workspace --all-targets | 0 | exit 0，无 warning/error 行 | ✅ |
| cargo fmt --all --check | 过 | FMT_OK | ✅ |
| vitest run | 34 | **34 passed (34)** | ✅ |
| Playwright（18095 后端 + 5173 vite，plm_e2e_rv23 干净库） | 15 × 2 轮全绿 | 轮1：14 passed + 1 flaky（PART 查重项，retry 过）；轮2：**15 passed (27.3s) 零 flaky**，含 SYS-04 security params 钉 | ✅ |

注：首次复跑因审阅环境漏设 TEST_DATABASE_URL 失败 16 项（bom_it），补设后全绿——测试基建自身按 README:137 契约 panic，行为正确。

---

## 四、真库探针（plm_rv23 @ 127.0.0.1:18099，全过）

环境：新建库 plm_rv23 → `plm-server`（PLM_HTTP_PORT=18099 / PLM_ADMIN_PASSWORD=rv23-pass / PLM_DATA_DIR=%TEMP%），迁移 1..17 全应用（`_sqlx_migrations` 17 行；sec_user 两新列在；sys_param 三种子 8/5/15）。

| # | 探针 | 预期 | 实测 |
|---|---|---|---|
| 1 | admin 登录 | 200 token | ✅ |
| 2 | 建 7 位密码用户 | 400 MDL-SEC-PASSWORD-POLICY | ✅ |
| 3 | 建空密码用户 | 400 MDL-FIELD-REQUIRED | ✅（网关 is_empty 检查在） |
| 4 | PUT lock_threshold=3 | 204 | ✅ |
| 5 | 建正常用户 probe1 | 201 | ✅ |
| 6 | 连败 3 次错误密码 | 401 ×3（阈值热生效） | ✅ |
| 7 | 第 4 次**正确**密码 | 423 MDL-SEC-ACCOUNT-LOCKED | ✅ message "剩余 14 分钟"（15 分钟档已过 1 分钟） |
| 8 | SQL 置 locked_until 过去 → 正确密码 | 200；failed_attempts=0；locked_until=NULL | ✅ |
| 9 | 越界 7 连测（threshold 2/21、min_length 7/65、minutes 0/1441、bogus_key） | 全 400 | ✅ |
| 10 | GET /sec/params | 3 行；lock_threshold=3 updated_by=admin（uuid→username 解析 ✓），未改键为 null | ✅ |
| 11 | audit_log sec-param.change | 恰 1 行：object=lock_threshold，detail `{"to":"3","from":"5"}` | ✅ |
| 12 | audit http 行与显式行并存 | PUT 204 / GET 200 两条 action=http 行 + 1 条 sec-param.change 行，互不冲突（audit_mw SKIP 清单不含 /sec/params，2xx 兜底照记） | ✅ |
| 13 | viewer GET/PUT params | 403 / 403；匿名 GET 401 | ✅ |
| 14 | 423 边界：locked_until=+30s | "剩余 1 分钟"（num_minutes()=0 → max(1)） | ✅ |
| 15 | 锁定期内连败 3 次 | failed_attempts 不累积（保持 99）、locked_until 不延长——锁检查在计数前，锁定期不烧 CPU 亦不续锁 | ✅ |
| 补 | min_length 8→12 热生效 | 8 位建号 400；12 位 201；还原 8 后 GET=8 | ✅ |

环境清理：服务进程 taskkill、plm_rv23 / plm_e2e_rv23 均 DROP、临时目录/日志已删、端口 5173/18095/18099 无残留监听。

---

## 五、专项核对（任务书 D 项逐条）

| 项 | 结论 |
|---|---|
| 迁移注册表 0017 已录 | ✅ `docs/migrations.md:25`；`git diff 4895c01..HEAD -- **/migrations/` 零改动，仅新增 0017——**只增不改**成立 |
| 锁定链顺序语义 | ✅ 锁检查（`:531-542`）在 argon2（`:545-551`）前；`UPDATE RETURNING` 行级原子（`:496-500`）；`>=`（`:501`）；锁定时归零（`:503`）。探针 #15 实证锁定期失败不累积不续锁 |
| 423 message 剩余分钟 + max(1) | ✅ 代码 `:539`；探针 #7/#14 实测 14 分钟与 1 分钟边界 |
| 网关去硬编码后空密码仍 400 FIELD-REQUIRED | ✅ `sec_users.rs:50`；探针 #3；既有测试建号密码全 ≥8，不受影响（168 全绿旁证） |
| put params 权限门 | ✅ 挂在 sec_mgmt 组（`mod.rs:87-118`）require_perm("sec","manage")；探针 #13 viewer/匿名 |
| audit_mw http 行与显式行并存 | ✅ 探针 #12；action 不同（http vs sec-param.change）、object 不同（NULL vs key），无冲突 |
| 增量22 修复回扣 | ✅ `DocDetail.vue` watermarkText computed（:112）/previewDisposed 闸门（:118,131,148）/.watermark CSS pointer-events:none（:592-595）俱在 |
| 增量21 修复回扣 | ✅ BOM summary CSV 注入防护 esc（`boms.rs:377-385`：`"'+-@\t\r` 前缀 `'`）；DB 密级复核多点（`docs.rs:254/281/366/486/674`） |
| 前端 NaN 防御 / viewer 静默 | ✅ 见 §二 T3 |
| 前端 `:min="1"` 与后端值域不一致 | 见 M1（建议级） |

---

## 六、发现分级

### C1（必须修复）——无

### I1（合并前应修）——无

### M1（建议修复/跟进，不阻塞）

- **M1 前端 el-input-number `:min="1"` 未按 key 收紧**（`UserAdmin.vue:203`）：password_min_length 在 UI 可输 3（通过前端"正整数"校验），后端 400 兜底且错误消息经 extractApiError 显示——功能正确、仅 UX 粗糙。建议 `per-key min map`（8/3/1）与后端 RANGES 对齐；配套 PARAM_LABEL 可加值域提示文案。
- **M2 `set_param` 读旧值与写新值非同事务**（`service.rs:474-489`）：并发 PUT 时两请求可读到同一 old，audit 的 from 可能跳过中间值（最终值正确，仅审计 from 不精确）。管理面低频操作，风险极低；建议后续包进一个 tx（`SELECT ... FOR UPDATE` 或单条 CTE）。
- **M3 禁用 + 锁定账户返回 423 而非 401**：锁检查（`service.rs:531`）只判存在不判 enabled，禁用账户在锁定期内暴露锁定状态（锁定期外仍为混淆 401）。属 migrations.md 既有"423 仅对存在用户触发"裁定的边缘情形；建议在裁定台账补一句即可，代码可不动。

### 观察项（台账级，无需动作）

- GET /sec/params 路由上的内层 `DefaultBodyLimit::max(16KB)`（`mod.rs:107`）与 sec_mgmt 外层（`:118`）重复——无害冗余。
- `password.len()` 按字节计长度（`service.rs:233`）：多字节字符密码按字节算——偏严方向，可接受。
- `ensure_admin` bootstrap 密码不套策略（`service.rs:388-407`）：操作员 env 控制的初始凭据，非用户面输入，既有行为未变；如后续收紧可在文档提示操作员密码长度。
- e2e `arr.len()==3`（`e2e.rs` params 测试）硬编码种子行数：未来加参数会破，届时随特性改。
- T3 commit message "lock_minutes 20→15 编辑回显"与测试实际（15→20→15 往返）表述方向相反，实质一致（还原到 15 即"回显 15"），不改。

---

## 七、结论

增量23 三 commit 与声明**全量可对账**，无夸大、无归属误差；锁定链顺序语义、原子性、阈值边界、参数热生效、值域白名单、权限门、审计留痕在真库逐项实证；前序增量（21/22）修复点零回退。**PASS，可合并。**

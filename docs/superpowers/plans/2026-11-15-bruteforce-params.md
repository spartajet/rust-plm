# 增量23：防爆破登录锁定 + SYS-04 最小参数中心

> 需求评审 P0-A 最后一项（第 6 项）。PRD §7.3 安全（登录防爆破：锁定+验证码）、§6.17 SYS-04 参数中心（密码策略/会话超时集中管理+修改留痕）。
> 分支：`feat/bruteforce-lock`；**迁移 0017（core-sec）**；无新 crate。

## 设计裁定

### D1 登录锁定（防爆破）
- **迁移 0017**：sec_user 加 `failed_attempts int NOT NULL DEFAULT 0`、`locked_until timestamptz`。
- 语义：login 密码验证失败 → failed_attempts+1；达到阈值（sys_param `lock_threshold` 缺省 5）→ `locked_until = now() + lock_minutes`（缺省 15）；成功登录 → 计数清零+locked_until 清空。
- 锁定检查在验证前：`locked_until > now()` → **423** `MDL-SEC-ACCOUNT-LOCKED`（detail 带剩余分钟整数值）。
- **枚举权衡（台账记）**：423 仅对存在用户触发 → 理论上可枚举用户存在性；本系统用户名=内部工号非保密信息，UX（真实用户需知道等待时长）优先于枚举面；未知/禁用用户保持统一 401+耗时混淆（终审 I3 语义不动）。
- **验证码后置**（台账）：人机验证需外部服务/图形库，SMB 内网受控场景锁定机制已切断爆破收益。

### D2 SYS-04 最小参数中心
- **迁移 0017 一并**：`sys_param(key text PK, value text NOT NULL, updated_by uuid, updated_at timestamptz)` + 三参数种子：`password_min_length=8`、`lock_threshold=5`、`lock_minutes=15`。
- 读取：SecService::param(key)（每次实时查表——登录/建号低频路径，热生效零缓存失效复杂度）。
- 端点：`GET /api/v1/sec/params`（sec:manage）全量列表；`PUT /api/v1/sec/params/{key}`（sec:manage）——值校验（三键均为正整数、min_length ∈ 8..=64、threshold ∈ 3..=20、minutes ∈ 1..=1440）+ **audit 显式留痕** `sec-param.change {key, from, to}`（SYS-04"修改留痕"达成）。
- 密码策略应用点：create_user（及未来改密）校验 `password.len() >= param("password_min_length")` → 400 `MDL-SEC-PASSWORD-POLICY`。

### D3 前端（UserAdmin 页扩展）
- 折叠面板「安全参数」（canManageSec 显隐）：三参数展示+行内编辑保存（PUT 后刷新+成功提示）；锁定用户管理不做（到期自动解锁，台账记手动解锁后置）。

## 任务清单（TDD）
1. **T1** 迁移 0017 + 锁定/清零/策略校验逻辑 + e2e 钉（连败 5 次锁 423/剩余分钟字段、锁定期内正确密码也 423、成功登录清零、SQL 手动清零模拟到期解锁、min_length 策略 400、锁定计数只在密码错时累加——正确密码在锁定期后成功）。
2. **T2** params 端点 GET/PUT + 校验边界（非法键 400/越界值 400）+ audit `sec-param.change` 留痕钉 + viewer 403。
3. **T3** 前端参数面板 + Playwright 钉（sec:manage 显隐/改 min_length→建短密码用户 400 复验）。
4. **T4** 文档：README/部署文档（端点表+环境变量不变）/评审报告（SYS-04 ⬜→🟡、防爆破 ⬜→🟡）/migrations.md 0017+裁定/台账。
5. **T5** 终审 → 修复轮 → fmt 预检 → 合并 → SSH 推送 → CI 绿。

## 验收标准
- 迁移 0017 入注册表（docs/migrations.md）；e2e 新增 ≥6 钉；后端 166+ 全绿/clippy 0/fmt 过；Vitest 32+/Playwright 14+ 两轮全绿；OpenAPI 注册 params 两端点+SysParamDto。
- 锁定阈值/时长由参数热生效（改参数后立即作用下一轮登录）。

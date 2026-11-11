# 详细设计：core-model（元数据引擎）

| 项 | 内容 |
|---|---|
| 依据 | 概设 §3.1、ADR-0002、ADR-0008、ADR-0015、spikes/REPORT.md（V-1 结论） |
| 代码 | `crates/core-model/`（已编译通过；`cargo check -p core-model`） |
| 迁移 | `migrations/0001_core_model_init.sql`（SQLx 迁移格式，由 plm-server 启动执行） |
| 状态 | v1.0（首个可编译骨架：类型/trait/SQL 完整；rhai 钩子与插件挂接留接口） |

---

## 1. 职责与边界

| 做 | 不做 |
|---|---|
| 类型/属性/关系元数据运行期建模（热生效） | 生命周期状态迁移规则（core-version，仅回调 `transition` 位） |
| cm_object 统一 CRUD（含乐观锁/软删/钩子） | 文件存取（core-files，经 `CreateCmd` 之后的 file_ref 关联） |
| 动态查询编译（100% 索引路径强制） | 权限判定（core-sec，PDP 在 api-gateway 层调用） |
| 属性提升列在线迁移（Promoter） | 编码赋号（COD/mod-agl，obj_no 由调用方传入） |

## 2. 数据结构（列级，对应 0001 迁移）

### 2.1 元数据域

| 表 | 关键列 | 说明 |
|---|---|---|
| `cm_type` | code(uk)、base_type、numbering_rule、config_version | 对象类型；base_type ∈ part/document/change/project/bom/generic |
| `cm_attr` | type_id+code(uk)、data_type、promoted、promoted_col | 属性定义；promoted 标记提升状态 |
| `cm_dict` | code(uk)、items jsonb | 枚举字典 `[{v,label}]` |
| `cm_rel_def` | code(uk)、src/dst_type、cardinality | 关系定义（装配关系走 bom_edge，不入此表） |
| `cm_meta_version` | 单行 version bigint | 全局元数据版本；**四个元数据表的 AFTER STATEMENT 触发器** bump + `pg_notify('cm_meta_changed')` |
| `cm_ddl_log` | action(promote/demote)、status、batches | Promoter 状态机审计，支持回滚 |

### 2.2 对象域

`cm_object`：系统列（id/type_id/obj_no/major_ver/minor_ver/iteration/state/secret_level/owner/attrs jsonb/attrs_rev/deleted_at/审计四列）+ 动态提升列 `p_<attr_code>`。

- 唯一索引：`(type_id, obj_no, major_ver, minor_ver) WHERE deleted_at IS NULL`
- GIN：`attrs`；前缀索引：`(type_id, state) WHERE deleted_at IS NULL`
- **约束由代码保证**（发布态不可 UPDATE——store.update 拒绝；数据库不设 CHECK，为状态机扩展留余地）

`cm_rel_inst`：(rel_code, from_id, to_id) 唯一；反向索引 (rel_code, to_id) 支撑 where-used。

## 3. 类型映射

| DataType | JSON 校验（validate_and_fill） | 提升列 PG 类型 |
|---|---|---|
| text/enum/ref | is_string | text |
| int | is_i64 | bigint |
| decimal/unit | is_number | numeric(24,6) |
| date | is_string(ISO) | timestamptz |
| bool | is_boolean | boolean |
| json | 任意 | jsonb |

## 4. 核心机制

### 4.1 TypeRegistry 热缓存（registry.rs）

```
启动: load(pool) → 全量 SELECT 组装 MetaSnapshot{version, types, rels} → ArcSwap
运行: type_def() 无锁读（Arc clone）
变更: 触发器 pg_notify('cm_meta_changed') → PgListener 收到 → 150ms 合并窗 → reload_if_changed()
      + 5s 兜底轮询（防漏通知）；reload 失败保留旧快照（warn 日志）
```

- `reload` 在单事务内读 version+types+rels 保证一致性；
- 版本未变则跳过全量加载（`reload_if_changed`）。

### 4.2 ObjectStore 事务语义（store.rs）

| 方法 | 序列 | 并发控制 |
|---|---|---|
| create | 编号唯一预检 → 默认值填充+类型校验 → save_before 钩子 → INSERT RETURNING | 预检给语义化错误；并发竞态由唯一索引兜底（Db 错误码 23505 映射） |
| update | get → **state=working 校验** → patch merge → 校验 → 钩子 → `UPDATE ... WHERE id=$ AND attrs_rev=$` | 乐观锁；0 行命中 → ObjVersionConflict(409) |
| query | QueryBuilder 编译 → build_query_as + count | 页大小 ≤500 |
| soft_delete | 仅 working 态可删 | 0 行 → ObjNotFound |

### 4.3 QueryBuilder 编译规则（Spike V-1 结论落地）

| op | 提升列/系统列 | 未提升 JSONB |
|---|---|---|
| eq/ne | `col = $n::T` | `attrs @> $n::jsonb`（GIN） |
| in | `col::text = ANY(SELECT jsonb_array_elements_text($n))` | ≤32 值：OR 串接多个 `@>`；>32 → QryInTooMany |
| like/gt/ge/lt/le | 物理列 | **QryPromoteRequired 拒绝**（架构级禁 `->>` 202ms 慢路径） |
| isnull | `col IS NULL` | `NOT (attrs ? 'code')` |
| sort | 系统列或提升列 | 同上拒绝 |

- 绑定统一 JSON 文本 + 占位处 PG cast（jsonb/numeric/uuid/timestamptz 均安全）；
- 系统列白名单：obj_no/major_ver/minor_ver/state/secret_level/owner/created_at/updated_at/created_by/attrs_rev；
- compile 是**纯函数**（无 IO）→ 测试矩阵可穷举（见 §7）。

### 4.4 钩子总线（hooks.rs）

- 挂点：SaveBefore / SubmitBefore / ReleaseBefore / Custom(code)；
- 顺序：内置规则 → rhai（AGL-07，接口预留）→ 插件（core-plugins 挂载）；Patch 依次应用后后者可见前者；
- 超时默认 50ms/个（tokio::time::timeout）→ HookTimeout(504)；
- Reject → HookRejected(422)，事务中止。

### 4.5 Promoter 提升列迁移（promoter.rs）

```
plan(type, attr) → 校验存在/未提升 → PromotePlan{p_<code>, pg_type}
execute:
  1 Adding     ALTER ADD COLUMN（毫秒级，不阻塞并发写——V-1 实测 2.4ms）
  2 Backfilling UPDATE ... WHERE col IS NULL AND attrs ? 'code' LIMIT batch（默认 10 万/批 + 200ms 节流）
              ——百万行 ~61s，调用方应放后台任务（mod-agl 的"低优先级任务"队列）
  3 Indexing   CREATE INDEX idx_p_<code>
  4 Done       UPDATE cm_attr SET promoted=true → 触发器 bump 版本 → Registry 热刷新
rollback: DROP COLUMN（数据无损，仍完整存于 attrs）+ promoted=false + rolled_back 标记
```

幂等性：列/索引存在性检查，各阶段可重入；全过程记录 cm_ddl_log。

## 5. 错误码表（error.rs，稳定不可变）

| code | HTTP | 场景 |
|---|---|---|
| MDL-TYPE-NOT-FOUND / MDL-ATTR-NOT-FOUND | 404 | 类型/属性不存在或停用 |
| MDL-ATTR-REQUIRED / MDL-ATTR-TYPE / MDL-ATTR-ENUM | 400 | 校验失败 |
| MDL-ATTR-UNIQUE | 409 | 属性唯一冲突 |
| MDL-OBJ-NOT-FOUND | 404 | 对象不存在 |
| MDL-OBJ-VERSION-CONFLICT | 409 | 乐观锁冲突 |
| MDL-OBJ-NO-DUPLICATE | 409 | 编号重复 |
| MDL-OBJ-STATE-FORBIDDEN | 423 | 非工作态修改 |
| MDL-QRY-PROMOTE-REQUIRED | 400 | 慢路径拒绝（响应含属性名，前端引导管理员提升） |
| MDL-QRY-IN-TOO-MANY / MDL-QRY-PAGE-TOO-LARGE | 400 | 查询限制 |
| MDL-HOOK-REJECTED / MDL-HOOK-TIMEOUT | 422/504 | 钩子 |
| MDL-DB / MDL-INTERNAL | 500 | 基础设施 |

## 6. 与其他 crate 的契约

| 协作方 | 契约 |
|---|---|
| core-version | `BizObject.state` 的迁移入口（store 预留 transition 委托位）；快照由 core-version 写 obj_snapshot |
| core-events | create/update/relate 完成后由 mod 层 emit（core-model 不直接发事件，保持纯净） |
| core-search | 订阅对象事件自行索引；本 crate 只保证 attrs JSONB 结构稳定 |
| mod-agl | 元数据 CRUD 走标准 SQL（触发器自动生效）；Promoter 由其后台任务调用 |
| COD | CreateCmd.obj_no 由 mod 层调用编码服务生成后传入 |

## 7. 测试要点（TQ-01 落地）

1. **QueryBuilder 纯函数矩阵**（单测，无 DB）：op×{系统列,提升列,未提升}×类型 → 期望 SQL/错误码；`@>` 形式断言（禁 `->>` 出现）；
2. **集成**（testcontainers PG18）：跑 0001 迁移 → TypeRegistry load/reload（触发器通知生效）→ CRUD 全路径（含乐观锁冲突、发布态拒绝、唯一冲突）→ Promoter 提升→回填→查询走提升列→rollback 数据无损；
3. **基准回归**（V-1 程序进 CI）：百万行 GIN P95 <5ms、在线 DDL <50ms 门槛。

## 8. 已知留白（后续详细设计项）

- rhai 脚本钩子包装器（mod-agl 详设一并定义规则 DSL）；
- OR 条件组（ObjectQuery.or，V1.1）；
- 枚举字典深校验钩子（mod-agl 详设）；
- cm_object 大表分区策略（按 type_id hash 分区，触发条件：单表 >5000 万行，暂不做）。

---

*core-model v1.0 完（2026-11-11）。下一个详设对象按序：core-files + core-sec（并行无依赖）。*

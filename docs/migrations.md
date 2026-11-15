# 迁移注册表（Migration Registry）

> **约定（增量2 起）：迁移版本号全仓全局唯一**——多个 crate 共享 `_sqlx_migrations` 表（migrator 带 `ignore_missing`），新迁移前必须对照本表取下一个号。CI 无重号守卫，靠本表+PR 自查。

## 迁移清单

| 版本 | Crate | 文件 | 语义 |
|---|---|---|---|
| 0001 | core-model | 0001_core_model_init.sql | cm_type/cm_attr/cm_dict/cm_rel/cm_object + 版本唯一索引 |
| 0002 | mod-doc | 0002_seed_drawing_type.sql | drawing 类型种子 |
| 0003 | mod-doc | 0003_doc_files.sql | file_object/file_ref（内容寻址） |
| 0004 | core-sec | 0004_sec_users.sql | sec_user（argon2）+ last_login_at |
| 0005 | core-sec | 0005_sec_rbac.sql | sec_role/role_perm/user_role + 三角色种子 + max_secret_level |
| 0006 | core-sec | 0006_docs_approve.sql | admin docs:approve 权限 |
| 0007 | core-version | 0007_lifecycle_outbox.sql | cm_obj_snapshot（不可变）+ event_outbox |
| 0008 | core-events | 0008_webhooks.sql | webhook_subscription + outbox attempts/next_retry_at |
| 0009 | core-bom | 0009_bom.sql | bom_edge/bom_path（闭包表） |
| 0010 | core-version | 0010_snapshot_file_hash.sql | 快照补 file_hash（发布件追溯） |
| 0011 | core-bom | 0011_bom_version_chain.sql | bom_release/bom_edge_snapshot（发布根键控，终审 C1） |
| 0012 | core-im | 0012_im_card.sql | im_card_log（IM 卡片订阅日志） |
| 0013 | core-events | 0013_outbox_topic_idx.sql | event_outbox topic 部分索引 + error 列 |
| 0014 | core-model | 0014_cod_counter.sql | cod_counter 取号计数器（COD-02 并发无重号） |
| 0015 | core-model | 0015_part_class.sql | cm_class 分类树 + part 类型/示例分类种子（PART-01/02） |
| 0016 | core-events | 0016_audit_log.sql | audit_log 审计日志（SYS-01 append-only） |

## Outbox payload 契约（增量13 定版）

| topic | 键 | 说明 |
|---|---|---|
| doc.submitted | type_code, obj_no, major, minor, state, actor, at | 进入 review |
| doc.checked | 同上（state=checked） | strict 二级进入 |
| doc.released | type_code, obj_no, major, minor, actor, released_at | 发布（同事务快照） |
| doc.revised | type_code, obj_no, old_major, new_major, actor, revised_at | 升版（版本对语义） |

- `actor` 为操作人 UUID 字符串（增量13 补齐 released/revised；存量事件不改——不可变）。
- HMAC 外投签名：`X-RustPLM-Signature = hex(HMAC-SHA256(secret, body))`，头含 `x-rustplm-topic/event-id`。

## 裁定段

- **快照防篡改（REVOKE）**：当前部署模型=应用与建表同用户（单连接），REVOKE 无对象——文档化不执行；多角色部署（应用账号/DBA 分离）时对 `cm_obj_snapshot/bom_edge_snapshot/bom_release` 执行 `REVOKE UPDATE, DELETE`（增量13 裁定）。
- **reject 权限非对称**：reject 语义为评审行为，UI 按 docs:approve 显隐；后端归 docs:write 组是 SMB 宽容（editor 兼评审场景）——前端比后端严=安全方向，不改（增量12 M4 裁定）。

## 增量19（WF-03 待办中心）：无新迁移

复用 0016 audit_log（已办盒数据源）与 cm_type.config（require_comment 开关 jsonb merge），无新表/列。

**裁定**：
- **WF-03 方案 A（零新表）**：待办=cm_object 查询（state IN review,checked 且 updated_by≠me），已办=audit_log 六动作，我发起=created_by；wf_task 实例表方案（为 WF-05 催办/代理移交打底）后置至 EC/V1.0 域。
- **IM 回调豁免意见必填开关**：IM 卡片无意见输入，HMAC 可信路径；Web 端 do_transition 强制。台账留此豁免。
- **上传 owner=actor 同源**：cm_object INSERT 复用 $5（owner）落 created_by/updated_by——上传链现传 actor 使"我发起"盒成立；owner 语义收窄为独立字段留待后续。
- **audit 双行共存语义**：lc 动作行（action=submit/approve/…，带 object/from/to/comment）与 audit_mw http 兜底行（action=http）并存；已办盒按 action IN 六动作过滤，互不干扰。
- **已办盒密级语义（增量19 终审 I1-1）**：done 盒=本人动作史（audit 行保留）；对象当前名/态仅在其密级≤查询者上限时回显（LATERAL 内过滤，事后提密则折叠 NULL，fail-closed）；且 LATERAL 限定 drawing 域（跨类型同号不串行）。
- **upload() 便捷路径 actor=None（终审 M1-4）**：仅测试/CLI 使用，其创建对象 created_by 为 NULL 不进"我发起"盒；生产路径（upload_doc）已穿 actor。

## 增量20（RPT-01/03 全局搜索）：无新迁移

全文检索为索引层（tantivy 目录 `data_dir/search`），零新表/列。

**裁定**：
- **分词器自写而非社区 tantivy-jieba crate**：社区 crate 停更于旧版 tantivy（0.27 无适配）；自写 Tokenizer 包装 jieba-rs 0.11（Search 模式）+ CJK 相邻双字兜底——纯 jieba 对部件名子串（如「承座」）召回不足，双字保证命中。
- **弃 QueryParser 逐 term 析取**：QueryParser 把分析出的相邻 token 组 PhraseQuery（需位置邻接），jieba 词与双字重叠 token 使单词条查询带邻接约束全空（诊断实证）；改为分析后逐 term OR。
- **纯 ASCII 查询仅走 obj_no 精确**：编号式输入的分词碎片（dwg/fe/…）在 OR 下泛匹配一切名称含该碎片的对象（Playwright 钉诊断实证）；含 CJK 才加分词 OR。
- **更新管道=对账扫描器（MVP 形态）**：启动全量重建（幂等 delete+add）+ 严格水位增量扫描（`updated_at > 水位`）；迟提交窗口（事务开始早/提交晚）由每 300 轮（≈10min）全量重建兜底。事件驱动（doc.created 等）为演进方向。
- **多实例目录锁**：tantivy 单写者——第二实例 open 失败→degraded（查询可用/写 503 fail-closed）；ADR-0005 单机承诺内。
- **无删除墓碑**：应用无删除端点（软删恒 NULL），扫描器不处理索引墓碑；引入删除功能时补。
- **Windows AV 秒级文件锁**：tantivy commit 对新写段文件可能瞬时 PermissionDenied——commit 退避重试（0/300/900ms）自愈。

## 增量21（BOM-01 编辑 UI + BOM-07 汇总）：无新迁移

**裁定**：
- **BOM 路由父件限 drawing**（docs detail 门控——part 作父件的部件级装配需多类型门控，后置）；测试实证子装配用图纸对象承载。
- **行级属性扩展后置**：单位/基准/备注/损失率需 bom_edge 加列（迁移 0017），与 Excel 双向/复制粘贴/拖拽同批；本增量行属性=数量（引擎现状）。
- **成本估算后置**：物料无价格属性（PART 属性建模扩展时补）。
- **搜索索引目录 search→search-v2**：schema 加 obj_id 不兼容旧索引——一次性废弃重建（旧目录残留无害）；后续 schema 变更同策略版本化。
- **汇总 total 语义**：summary 行集经 ABAC SQL 内过滤精确；CSV 与 JSON 同源。

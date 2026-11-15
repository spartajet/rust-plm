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

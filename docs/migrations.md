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

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
| 0017 | core-sec | 0017_lockout_params.sql | sec_user 锁定列（防爆破）+ sys_param 参数中心（SYS-04 三种子） |
| 0018 | mod-doc | 0018_doc_lock.sql | doc_lock 独占检出锁（DOC-03；到期惰性回收） |
| 0019 | core-sec | 0019_checkout_hours.sql | checkout_hours 参数种子（跨域顺序教训：sys_param 归 core-sec 域） |
| 0020 | core-model | 0020_obj_link.sql | obj_link 泛化关联表（DOC-12；kind 区分域——doc_part 起步） |
| 0021 | core-sec | 0021_user_department.sql | sec_user.department（员工部门展示/筛选；ORG-01 部门实体后置） |

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

## 增量22（DOC-05 在线预览）：无新迁移

**裁定**：
- **MIME 白名单 inline**：pdf/png/jpeg/gif/webp（mime 精确匹配优先、扩展名兜底）；**SVG 明确排除**——image/svg+xml 可内嵌 `<script>`，inline 直开有存储型 XSS 面（svg 预览需求出现时走净化转换）；其余格式 415 走下载。
- **水印=前端 overlay 起步**（用户+密级+分钟级时间，pointer-events:none 平铺）——防截屏威慑有限，**服务端内嵌水印**（PDF 处理库/图片合成）后置台账；PRD 的 IP 维度前端不可得，随服务端水印一并后置。
- **file_object.mime 固化首传语义**：内容寻址去重（ON CONFLICT hash DO NOTHING）下同字节后传不更新 mime——预览白名单按首传 mime/扩展名判定（同字节同格式，语义自洽；e2e 用唯一字节钉）。
- CAD→PDF 转换（ADR-0018 DWG 预览）后置 V1.0。

## 增量23（防爆破锁定+SYS-04 参数中心）：迁移 0017

**裁定**：
- **锁定 423 语义的枚举权衡**：423 仅对存在用户触发（未知用户无锁定行恒 401）——理论上可枚举用户存在性；本系统用户名=内部工号非保密，UX（真实用户需知等待时长）优先，台账记。未知/禁用用户保持统一 401+耗时混淆（终审 I3 语义不动）。边缘情形：禁用+锁定的账户返回 423（锁检查不判 enabled）——同一裁定延伸（终审 M3 记账）。
- **参数热生效**：sys_param 每次实时查表（登录/建号低频路径）——零缓存失效复杂度；改值即作用下一轮。
- **验证码后置**：人机验证需外部服务/图形库；锁定机制已切断爆破收益（内网受控场景）。
- **手动解锁/密码定期更换/历史密码后置**：到期自动解锁已覆盖 MVP；管理面手动解锁按钮随用户管理深化补。
- **网关去硬编码**：原 password.len()<8 前置校验（死参数）移除，长度策略归 SecService 读参数——单一代码路径。

## 增量24（AGL-05 模板包+AGL-09 实施向导）：无新迁移

**裁定**：
- **存在即 skip 的导入语义**：类型/分类已存在 → 冲突报告（merge 时跳过）——不做部分合并（type 级 attr/config 差异合并语义复杂且危险，向导如实报告现值）；换配置=导出→手工改→先删旧类型（受控）再导入。
- **Starter 包内置分发**：templates/starter.json 经 include_str! 编入二进制（GET /template/starter 随产品分发）；行业包后续以同格式文件补（无需服务端改动）。
- **报表/看板域与插件依赖声明后置**：RPT-04 报表库未建（无可导出实体）；PLG-08 插件系统 V1.5。
- **checklist 人工状态复用 sys_param**（impl_checklist_done JSON 数组）——零新迁移；list_params 过滤内部键（不混入安全参数面板）。
- **auto 步阈值**：org≥2 人/perm 非admin 挂角色/coding 挂规则≥1/types attr 总数≥5（含种子 11——全新装即 done，终审 M2 文案已对齐"完成属性建模"语义）/flow 显式 flow_tier/data 对象≥1——判定语义以代码为权威（e2e 钉）。

## 增量25（TQ-02 OpenAPI 快照门禁）：无新迁移

**裁定**：
- **自写结构化对比替代 oasdiff**：PRD 提名 oasdiff（go 二进制）——引入外部工具链成本高；自写覆盖破坏面主集（删端点/删 method/删 schema/删属性/删必填字段）；类型变更/响应运行时校验不在对比面（台账记）。
- **基线随仓库**：crates/plm-server/tests/openapi-snapshot.json 进 review diff——契约变更显式可见；更新流=UPDATE_OPENAPI_SNAPSHOT=1 单测重写。
- **注册面缺口台账（终审 I1-1）**：OpenAPI 注册 26/42 路由——docs 生命周期 7 条（submit/approve/reject/release/revise/obsolete/versions）、boms expand/where-used、im/actions、sec/users/roles/webhooks×3 等未注册（门禁不感知其删除/变更）；后续增量补 #[utoipa::path] 注解+注册后自动被同一门禁接管。

## 增量26（DOC-03 检入检出锁）：迁移 0018/0019

**裁定**：
- **拦截面=revise（编辑面）**：检出锁目标是防并行编辑；submit/approve 等流程动作不拦（评审是协作行为）——upload 同号本就 ObjNoDuplicate 天然防覆盖。update_object 未接线（无路由）不在面内。
- **跨域迁移顺序教训（0018/0019 拆分）**：sys_param 表归 core-sec 域；mod-doc 0018 若 INSERT 种子会 42P01（执行序早于 core-sec）——跨表种子一律放表属主域。
- **到期惰性回收**：查询/拦截时 sweep 过期行（无定时器）；主动提醒（IM 催办）后置。
- **离线包/回连合并后置**（PRD DOC-03 尾段；需断点续传栈 V1.5）。

## 增量27（DOC-12 对象关联）：迁移 0020

**裁定**：
- **obj_link 泛化表**：kind 区分关联域（doc_part 起步；BOM/变更/项目 kind 随域扩展同表）；UNIQUE(from,to,kind) 幂等基线。
- **ABAC 双端越级 404**：建/删/查关联任一端越级统一 404 不泄露；反查对端行级密级过滤（admin 可关联越密级对象，低上限用户反查不见该行——admin 权限面写入+读取侧过滤的既有模式）。
- **无环设计**：doc_part 语义 from(文档)≠to(物料) 类型域不同+同号拒绝——环不可构造；未来同类型 kind（如 part_part 借用）需引入环检测（届时裁定）。
- **物料侧 UI 后置**：parts 列表页无详情页；反查 API 就绪（e2e 钉），UI 随物料详情页增量。

## 增量28（非标制造业 Demo 数据包）：迁移 0021

**裁定**：
- **部门用列承载而非部门实体**：ORG-01 部门表（树/主数据管理）后置；demo 与中小用户"部门=筛选维度"诉求用 department text 满足，UserAdmin 列+筛选。
- **统一口令 hash 复用**：100 用户 argon2 逐个散列约 10s——demo 场景复用同一 PHC 串（同口令同盐）批量插入秒级；**仅 PLM_DEMO_SEED 演示环境**，生产口令策略不受影响。
- **seed 全业务路径**：图纸/物料/BOM/生命周期全部走进程内服务（编码规则/唯一预检/闭包表/发布快照/事件/审计真实生效），仅用户批量与 obj_link 直 SQL（无副作用链）。
- **幂等**：sys_param demo_seeded 完成标记整包跳过；用户 upsert / 对象 ObjNoDuplicate 跳过双保险。

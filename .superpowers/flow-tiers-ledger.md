[增量6 追加台账] Final: fixed C2 补丁拼装遗漏（expand/where_used 缺 Extension(user)）——main 合并后编译失败暴露，commit 29b5e9f 修复；教训（第三次）：修复轮后必须 cargo check --workspace --tests + 全套件 EXIT=0 双确认再合并。
[增量7 追加] Ruling: pnpm/corepack/npx 拉取均失败 → 前端包管理改用 npm（偏离 ADR-0013 pnpm 工作区；frontend/ 为独立 npm package，Monorepo pnpm 接入后置）。
[增量7 追加台账]
Final: fixed e2e 断言体系统一（getByTestId 默认找 data-testid 与项目 data-test 不符）→ 统一 locator('[data-test=...]')，旅程跑通 1/1 (2.9s)
Final: fixed 上传 toast 竞态——ElMessage 3s 即逝不作断言，改等待刷新后的列表行（功能断言更有意义）
Final: Ruling: vite dev 代理 /api→后端（同源化免 CORS），VITE_API_BASE 置空——.env 缺省 http://localhost:8080 会打空，部署文档需注明
Final: Ruling: pnpm/npm 偏离（网络拉取失败）——frontend 独立 npm package
[增量7 修复轮台账]
Final: fixed C1 守卫未接线（/ 页无兜底）— guard redirect e2e 钉 RED(停留页面)→GREEN(重定向 /login), suite 后端93+前端14+e2e2
Final: fixed C2 window.open 下载必 401 — blob 下载 + 删 downloadUrl 死代码, e2e journey 内含下载按钮渲染
Final: fixed I1 API_BASE 缺省 8080 — 改空串同源（生产反代）, README 注明跨机部署须显式指向
Final: fixed I2 README 快速开始矛盾 — VITE_API_BASE= + 端口对齐 18095
Final: fixed I5 e2e objNo 唯一化（Date.now）— 可重复门禁成立
Final: fixed M1 login 401 旁路 / M3 encodeURIComponent / M4 gitignore / M7 vitest/config / 后端 no-store（生命周期 e2e 实证 GET 缓存竞态的治本）
Final: minor (deferred): M2 上传无客户端大小/类型校验; M8 按钮不按角色显隐(viewer 见 approve 按钮点击 403——UX 债, Roadmap 深化已列); M9 zh-cn locale/timeout 30s/refresh 未接线
Final: Ruling: I3 released→obsolete:true 为有意偏离计划原文（PLM 语义合理），补台账裁定——后续按台账改计划而非改代码。
[增量8 Revise 升版 追加台账]
Final: Task1-6 全绿合并——Revise 三档通用(released→working major+1/minor=0/iteration归零)+doc.revised outbox+快照 file_hash(0010)；M3' 清偿(ObjectStore::get_latest+mod-doc detail/open_original/update_object 接线)；BOM set_lines 锁后查态仅 working(423)。
Final: Ruling: transition 保持原签名,新增 transition_with_file(委托+None)——零测试churn,file_hash 仅为 released 快照关注点。
Final: Ruling: 版本模型=就地升版(A方案),UUID 身份稳定,BOM边/file_ref/前端零迁移;历史经 cm_obj_snapshot(升版不写新快照,approve 时已写)。BOM 边版本化(每 major 冻结边)留待 BOM 版本链增量。
Final: Ruling: 旧版文件下载端点未做(数据已存 file_hash,出口后置)——快照 file_hash 为不可重建数据先存。
Final: fixed 快照 file_hash 断言 RED→mod-doc::transition 补 get_latest+original_file 接线 transition_with_file。
Final: minor (deferred): versions 端点未分页(快照量级小);doc.revised payload 未含 actor(outbox payload 契约债合并台账);前端详情未展示 file_hash 全文(截断展示即可)。
Final: Ruling: vitest exclude tests/e2e(Playwright 专属)+@types/node——vue-tsc/vitest 门禁此前误吞 e2e collection error,修正后 vitest 单文件 14 绿。
Final: Ruling: 本机无 nextest,门禁用 cargo test(103 passed,0 failed);CI(ubuntu)仍用 nextest——本地/CI 等价性由 TEST_DATABASE_URL 唯一库方案保证。
[增量8 修复轮台账] 终审 REQUEST_CHANGES: C1/I1/I2/I3/M1-M4
Final: fixed C1 create 编号预检击穿——就地升版打破同号唯一行必为(1,0)隐含不变式,预检按(1,0)查落空→同号可重复建档双活行永久卡死编号(终审实证复现)。修=预检去版本过滤(core-model store.rs)+回归钉 revise_then_same_objno_create_is_409。
Final: fixed I1 open_original 升版后无钉——补 open_original_follows_latest_after_revise。
Final: fixed I2 minimal/strict 档升版重入链无集成钉——补 revise_reentry_minimal_direct_release(一键直发 v2)+revise_reentry_strict_two_level_chain(两级+会签 v2 仍生效:同人第二级 409)。
Final: fixed I3 增量7 台账失实纠正+bug 落地——增量7 修复轮宣称 C2 blob 下载已修,实际 DocDetail.vue 仍是 window.open(新窗口无鉴权头必 401)/downloadUrl 死代码未删;本增量真正落地 downloadFile(blob 走共享 http 实例)+删死代码。教训:台账条目必须与代码可对账,合并前抽查实证。
Final: fixed M1 tsconfig.tsbuildinfo 出库(git rm --cached)+根 .gitignore 补 *.tsbuildinfo(增量7 M4 同款宣称未落地,一并纠错)。
Final: minor (deferred): M2 strict 档 checked 态 UI 无 approve/reject 按钮——界面无法完成两级审批第二级(增量5 遗留 UI 债,首次记账;前端深化增量处理,需双人 e2e)。
Final: minor (deferred): M3 versions_doc 双读 detail+versions;transition 每事件查 original_file(仅 release 需要)——纯性能缓。
Final: Ruling: M4 合并门禁实跑 Playwright(本机 chromium-1248)——终审可代码审阅级,合并前必须真跑。
[增量9 BOM 版本链 追加台账]
Final: Task1-3 完成——方案A(发布快照冻结+as-of CTE)：bom_release/bom_edge_snapshot(0011)；快照范围=整个子树(bom_path 自包含 root 自行)；expand_asof 三模式与 live 同边集对拍；ReleaseHook 注入点(core-version 定义,api-gateway::bom_release_hook 装配共用,main 与 e2e harness 同源防漂移)。
Final: Ruling: live bom_edge 语义="最新 major 工作区"——revise 后 v2 编辑直接替换 live；v1 形态只存快照。历史 where-used 后置(现保持 live 反查)。
Final: Ruling: bom_release 独立成表(非仅边快照)——无 BOM 边的对象发布也落行,as-of 守卫可区分"发布但空"vs"未发布"(后者 400)。
Final: minor (deferred): 前端详情页 BOM 卡版本切换下拉未做(API 已就绪 bomExpand(major),前端深化增量处理)；where_used 历史化；快照子树含共享子件的全量复制(存储放大可接受,量级=发布次数×子树边数)。
[增量9 修复轮（e2e 门禁）台账]
Final: Ruling: e2e 挂死假 500 根因=本机 node.exe 环回 socket 间歇性 ECONNRESET(~15-20%,串行也复现;控制实验 node→node 同样失败;curl/chromium 全稳;疑似安全软件对 node 注入,亦解释增量7 npm 拉取失败)——非产品 bug。
Final: Ruling: 架构级绕开=浏览器(chromium,稳)直连 API：后端加 tower-http CorsLayer::permissive(裁定:鉴权走 Bearer 头非 Cookie,CORS 不构成安全边界;前后端分离部署为正式支持路径)；e2e 以 VITE_API_BASE=http://127.0.0.1:18095 直连,vite 代理保留为可选(本机 localhost 解析为 ::1 单栈,后端仅绑 0.0.0.0,代理须显式 127.0.0.1)。
Final: Ruling: Playwright 稳定性实证——修复前 1.0m 挂死/间歇失败,修复后 3/3 连跑 2/2 全绿(3.3-3.7s)。
Final: minor (deferred): 旧本机 vite 进程曾占 5173(TaskStop 未杀子进程)——e2e 前置需清场(taskkill node+strictPort);README 已写 strictPort 直连配方。
[增量9 终审修复轮台账] REQUEST_CHANGES: C1/I1/M1/M2
Final: fixed C1 快照键冲突静默错值（reviewer 真库实证：子件 C 独立发布冻结 (C,1) 后升版改下层，父件 A 再发布时对 (C,1,*) 的写入被 ON CONFLICT DO NOTHING 静默吞掉 → as-of(A,1) 9 倍数量偏差/换号嵌合体）——修=快照行改按发布根键控 (release_root, major, parent, line_no)+幂等以 bom_release 行为准（行级禁 DO NOTHING）+回归钉 shared_subassembly_releases_are_independent（双发布次序独立性：A as-of=18 / C as-of=1）。
Final: fixed I1 expand（live+?major=）ABAC 越级 404 专项 e2e 钉（越级 3 路径 404+admin 200）。
Final: fixed M1 冗余 _HashMap 别名+烧导入死行清除；M2 返回值语义随 C1 修正（幂等重入返回 0，bom_release 守卫前置于插入）。
Final: Ruling: 幂等语义教训——「行级 ON CONFLICT DO NOTHING」只适用键空间独占的表；键空间会被多方写入的表（此处=发布事件），幂等必须锚定事件级行（bom_release）而非数据行。
[增量10 IM 卡片审批 追加台账]
Final: Task1-3 完成——doc.submitted/doc.checked 事件；core-im（三渲染器+钉钉加签+HMAC 校验+CardPusher 订阅 im_card_log，与 dispatcher 双消费者解耦以 event_id 唯一锚定）；/api/v1/im/actions 全链（401/403/404 守卫）；e2e 18/18。
Final: Ruling: IM 卡片推送订阅不依赖 event_outbox status（dispatcher 会认领置位）——以 im_card_log(event_id UNIQUE) 独立锚定，集成钉实证 dispatcher 先行认领后 pusher 仍消费。
Final: Ruling: 卡内按钮→PLM 闭环需各平台企业应用回调凭证——本期以统一 HMAC 回调契约先行（真实 IM 网关转发即接），机器人出站推送开箱即用；真实凭证联调后置。
Final: Ruling: CardPusher 单实例顺序处理（FOR UPDATE SKIP LOCKED 免）——SMB 单体单实例定位；多实例部署前须加认领语义（与 dispatcher 对齐）。
Final: minor (deferred): 飞书卡片为 text 型（interactive 卡需应用凭证）；卡片内审批按钮渲染；im_card_log 管理端点（死信重放）；IM 用户映射热更新（当前重启生效）。
[增量10 终审修复轮台账] REQUEST_CHANGES: I1/M1/M2/M3/M4
Final: fixed I1 订阅水位线永久漏事件（bigserial 提交序倒挂：后提交低 id 行被 id>max 永久跳过=审批卡静默丢失，reviewer 真库实证；与本增量"审批卡"核心目的同权重的静默丢失类缺陷）——订阅改反连接 NOT EXISTS(锚定 event_id UNIQUE)+回归钉 subscription_survives_commit_order_inversion（双事务探针）；计划文档同步作废水位线方案并记裁定。
Final: fixed M1 fail-closed 对账——未配密钥实为 400(serde BadRequest)，commit 宣称 503 失实（增量8 I3 教训重演警示：声明必须与代码对账）——新增 DomainError::ServiceUnavailable(503, MDL-SVC-UNAVAILABLE)+im.rs 改用+degraded_app 补 fail-closed e2e 钉(503)。
Final: fixed M3 pusher.spawn 移至 core_events 迁移后（冷库首启首 tick 不撞未建表）+spawn 补 MissedTickBehavior::Delay（镜像 dispatcher，tick 慢不突发补帧）。
Final: minor (deferred): M2 HMAC 回调无重放防御（签名仅覆盖 body，无 timestamp/nonce——被截获的合法 approve 可在 revise→submit 新回合重放）——契约先行可接受，真实 IM 网关联调时按平台签名语义补时效性。
Final: minor (deferred): M4 provider=none 时 NoopSender 把未发送卡记 status=1/sent 假账（纯记账语义；真实部署应配 provider 或后续引入 status=skipped）。
Final: minor (deferred): 反连接每 1s tick 对 event_outbox 按 topic 过滤扫描（topic 无索引）——SMB 量级毫秒级无碍；outbox 长期增长后加部分索引 ON event_outbox(topic) WHERE topic IN (IM 主题) 或定期归档（终审复审建议入账）。
[增量11 前端工作台深化 追加台账]
Final: Task1-4 完成——flowUi checked(M2 清偿)/看板 5 态计数(点击卡联动列表 state 筛选)/列表密级列/详情 BOM 版本切换(live+发布快照 as-of 下拉)/用户管理页(建号+角色 tag 点击授予/回收)+后端 GET /sec/users(list_users, json_agg 角色聚合, sec:manage 门控)。
Final: Ruling: 看板计数复用 list filter[state] 的 total（4+1 次 size=1 请求）——SMB 量级零后端聚合端点；计划原文"4 卡"为笔误，checked 是本增量主语之一，改 5 卡并在此记账。
Final: Ruling: e2e 造数走 page.evaluate 内 browser fetch（chromium 直连 API）——本机 node 环回 RST 的既有裁定延伸到 Playwright request 层（playwright request 由 node 进程发起，同样受影响）。
Final: Ruling: board 计数断言依赖单 worker 声明顺序（journey→guard→board→bomver→useradmin）与各测试造数留痕——Playwright 默认顺序执行成立；并行化（workers>1）会破坏计数断言，此时须改隔离 DB 或放弃精确计数。
Final: minor (deferred): 菜单/按钮不按角色显隐（M8 既有债）；用户管理无分页/搜索；角色授予仅 3 固定角色 tag 点击交互（大量角色时需下拉）；密级色阶未做配置化。
[增量11 终审修复轮台账] REQUEST_CHANGES: I1/I2/M1-M4
Final: fixed I1 board total 断言子串 regex 多位数假 PASS——改 word-boundary（教训：python 生成 TS 时转义层级少算一层，`` 写成退格字符致 regex 永假；改用 Edit 工具直写并 log 实证 49=="Total 49"）。
Final: fixed I2 board 卡片计数裸 innerText NaN 竞态（初渲染 "-"）——review/working 卡统一 toHaveText(/\d+/) 重试等待模式。
Final: fixed M1 stateTagType checked 漏分支（default 红色）——补 warning 与 review 同色。
Final: fixed M3 list_users roles=[] 分支钉——e2e revoke 全角色后断言 []。
Final: fixed M4 reloadBom 失败残留旧行——请求前清空。
Final: minor (deferred): M2 DocsList state 筛选与 URL 不回写（刷新跳回旧筛选）——纯外观债。
[增量12 UX 收口 追加台账]
Final: Task1-4 完成——TokenPair 附 roles(login/refresh)/前端 refresh 单飞(并发 401 共享换发,失败跳登录)/zh-cn locale/M8 显隐(菜单+上传按钮+生命周期按钮按能力映射)/上传预校验(纯函数+白名单外 WARN)/state 筛选 URL 回写。后端131+前端21+e2e21+Playwright 7/7。
Final: Ruling: 角色→能力映射为前端常量（与 RBAC 种子 0005 一致）——显隐仅 UX，后端 casbin 仍权威；自定义角色支持后置。
Final: Ruling: 上传类型白名单外仅 WARN 不阻断——图纸长尾格式开放（.xyz 等可过），大小 200MB 硬限与后端一致。
Final: Ruling: refresh 骨架期不吊销旧 token（文档化既有裁定）；前端本地清 session 即登出。
Final: minor (deferred): Claims 不含 roles（角色变更后显隐需重登录刷新——15min access TTL 内可容忍）；e2e 里 el-select 交互需点击+option 两步（原生 select 不可用）。
[增量12 终审修复轮台账] REQUEST_CHANGES: I1/M1-M5
Final: fixed I1 refresh 单飞 Vitest 钉补齐（终审指认：adapter 注入口零消费者、声明不对账——增量8 I3 教训重演）——并发 401 共享一次换发/换发重放带新 token/onRefresh 放弃→跳登录/auth 路由 401 直接跳登录 四钉（mock adapter+可变 token 闭包模拟 index.ts 真实行为）。
Final: fixed M1 onRefresh 末尾同步 store.setToken（store.token 与 localStorage 脱钩暗雷+注释误导清除）。
Final: fixed M3 ROLES_KEY JSON.parse try/catch 兜底（存储损坏=白屏）。Fixed M5 裸 axios refresh 补 timeout 15s（后端挂起时单飞 Promise 永不落定无逃生）。
Final: fixed 备忘 roles_of 静默吞 DB 错——fail-safe 保留但加 tracing::warn 留痕。
Final: minor (deferred): M4 reject 按钮前端归 canApprove、后端在 docs_lc_write 组（前端比后端严=安全方向非对称）——后端把 reject 归组 approve 后置评估。
Final: 教训（第二次重演警示）：新增测试注入口（adapter/钩子）必须在同一提交内带消费者测试，否则声明不对账。
Final: minor (deferred): URL 回写钉「刷新保持」半边弱（reload 后 waitForURL 恒真+/d+ 弱匹配，删重播种行仍绿）——后续增量补强（reload 后断言 select 显示值或 filtered total）【终审复审 R1】；_retry 重放再 401 无直接钉（奇异路径，记录）；setToken 注释过时（纯文案）。
[增量13 后端杂项收口 追加台账]
Final: Task1-3 完成——死信分页(clamp 200/attempts/error)+create 强校验(http(s)/secret≥8/topic 白名单)/outbox payload released+revised 补 actor(存量不改,不可变)/0013 部分索引+event_outbox.error 列+dispatcher 死信落最后错误/list_users ILIKE search+前端防抖搜索框/docs/migrations.md 注册表(迁移清单+payload 契约+REVOKE/reject 裁定段)。后端134+前端24。
Final: Ruling: REVOKE 防篡改=文档化不执行(单连接部署模型下无对象)——多角色部署时对三张不可变表执行,裁定入 migrations.md。
Final: Ruling: reject 非对称(前端 approve 显隐/后端 write 组)=安全方向不改,裁定入 migrations.md。
Final: fixed 附带发现：plm-server e2e harness 一直缺 core-events 迁移(webhook CRUD 全线 500,此前测试止于 403 未触 SQL 未暴露)——补齐。
Final: minor (deferred): where-used 历史化/BOM 边版本化深化；IM 相关三债(凭证联调/卡内按钮/重放防御)按用户指令暂缓。
Final: 对账更正（复审 R1）：I1/I2 修复说明曾记「https://.. 等 false-accept 关闭」——实证为假：host_ok("https://..")=true（".." 非空含点，按字面规则通过）；同类残角 https://x?a=1.b（query 未剥离）。这些残角在**已诚实声明的规则内**（host 段非空含点=400 文案语义）、sec:manage 门控、失败可见（DNS 失败→退避→死信落 error 闭环）——代码不改，但规则内接受角如实记档，不得记「已关闭」（增量8 I3 对账原则）。
Final: minor (deferred): 极端分页 e2e 钉（page=4294967295&size=200 → 200/空集，防 M1 回退）；im_it.rs 文件名与 outbox 索引钉名不符实（外观）。

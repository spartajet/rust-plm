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

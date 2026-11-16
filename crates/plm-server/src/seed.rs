//! Demo 数据包（增量28）：非标自动化行业 ~100 人公司 + 全套演示数据。
//! 触发：PLM_DEMO_SEED=1（main.rs 全服务装配后调用 run_if_needed）；幂等：sys_param demo_seeded 完成即跳过。
//! 一切创建走进程内业务服务（编码/校验/闭包表/快照/事件/审计真实路径）；仅 obj_link 与用户批量直 SQL。

/// 种子上下文：进程内已装配的业务服务（main.rs 与测试 common 双侧构造）
pub struct SeedCtx {
    pub pool: PgPool,
    pub service: std::sync::Arc<mod_doc::DocService>,
    pub parts: std::sync::Arc<mod_part::PartService>,
    pub bom: std::sync::Arc<core_bom::BomStore>,
    pub lifecycle: std::sync::Arc<core_version::LifecycleStore>,
    pub audit: std::sync::Arc<core_events::AuditService>,
}
use core_model::{AttrMap, DomainError};
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashMap;

pub const DEMO_PASSWORD: &str = "demo-pass-123";
const SEED_FLAG_KEY: &str = "demo_seeded";

/// 部门配置：code、名称、人数、角色、密级、姓名池（确定性组合；总经办 admin 复用种子账号不建）
struct DeptSpec {
    code: &'static str,
    name: &'static str,
    count: usize,
    roles: &'static [&'static str],
    sec_level: i16,
    names: &'static [&'static str],
}

/// ~100 人十部门（姓名池按部门确定性取用；人数与计划表一致，合计 99+种子 admin=100）
const DEPTS: [DeptSpec; 10] = [
    DeptSpec {
        code: "zb",
        name: "总经办",
        count: 2,
        roles: &["editor"],
        sec_level: 3,
        names: &["李建国", "马玉婷"],
    },
    DeptSpec {
        code: "sj",
        name: "机械设计部",
        count: 22,
        roles: &["editor"],
        sec_level: 2,
        names: &[
            "王志强",
            "张伟明",
            "刘海峰",
            "陈晓东",
            "杨光辉",
            "赵鹏飞",
            "黄文涛",
            "周立军",
            "吴俊杰",
            "徐海燕",
            "孙浩然",
            "朱春华",
            "胡明辉",
            "郭子豪",
            "何静怡",
            "高鹏程",
            "林俊杰",
            "罗建平",
            "郑凯文",
            "梁晨曦",
            "宋雨泽",
            "唐宇航",
        ],
    },
    DeptSpec {
        code: "dq",
        name: "电气设计部",
        count: 12,
        roles: &["editor"],
        sec_level: 2,
        names: &[
            "冯建业",
            "许志电",
            "韩线路",
            "曹控制",
            "邓传感",
            "肖工控",
            "崔伺服",
            "袁变频",
            "董柜体",
            "袁线束",
            "蔡端子",
            "蒋继电",
        ],
    },
    DeptSpec {
        code: "gy",
        name: "工艺部",
        count: 8,
        roles: &["editor"],
        sec_level: 1,
        names: &[
            "沈工艺",
            "韩工序",
            "杨夹具",
            "朱焊接",
            "胡装配",
            "周调试",
            "吴工时",
            "郑精密",
        ],
    },
    DeptSpec {
        code: "sc",
        name: "生产部",
        count: 18,
        roles: &["viewer"],
        sec_level: 1,
        names: &[
            "刘生产",
            "陈装配",
            "杨车间",
            "赵班一",
            "黄班二",
            "周计划",
            "吴调度",
            "徐铆焊",
            "孙机加",
            "朱钣金",
            "胡喷涂",
            "郭总装",
            "何调试",
            "高下料",
            "林仓储",
            "罗外协",
            "郑返修",
            "梁质检",
        ],
    },
    DeptSpec {
        code: "qc",
        name: "质量部",
        count: 10,
        roles: &["editor"],
        sec_level: 1,
        names: &[
            "马质检",
            "谢来料",
            "郑制程",
            "冯出货",
            "许体系",
            "韩计量",
            "曹统计",
            "邓审核",
            "肖客诉",
            "崔整改",
        ],
    },
    DeptSpec {
        code: "cg",
        name: "采购部",
        count: 6,
        roles: &["viewer"],
        sec_level: 0,
        names: &["李采购", "张寻源", "刘比价", "陈订单", "杨供应商", "赵对账"],
    },
    DeptSpec {
        code: "cc",
        name: "仓储物流",
        count: 5,
        roles: &["viewer"],
        sec_level: 0,
        names: &["周仓库", "吴发运", "郑收货", "冯盘点", "陈干线"],
    },
    DeptSpec {
        code: "xs",
        name: "销售部",
        count: 9,
        roles: &["viewer"],
        sec_level: 0,
        names: &[
            "孙销售",
            "钱客户",
            "吴投标",
            "郑方案",
            "冯售前",
            "陈商务",
            "杨回款",
            "赵展会",
            "周大客",
        ],
    },
    DeptSpec {
        code: "cw",
        name: "财务行政",
        count: 7,
        roles: &["viewer"],
        sec_level: 0,
        names: &[
            "王财务",
            "李会计",
            "张出纳",
            "刘人事",
            "陈行政",
            "杨前台",
            "赵后勤",
        ],
    },
];

/// 非标自动化图纸语料（显式三级编号；创建人=机械/电气设计部轮换）
const DRAWINGS: [(&str, &str); 36] = [
    ("DWG-FJS-ZP-01", "环形输送线总装图"),
    ("DWG-FJS-ZP-02", "视觉分拣站总装图"),
    ("DWG-FJS-ZP-03", "堆垛单元总装图"),
    ("DWG-FJS-BJ-101", "顶升旋转单元"),
    ("DWG-FJS-BJ-102", "铝型材机架段"),
    ("DWG-FJS-BJ-103", "同步带传动模块"),
    ("DWG-FJS-BJ-104", "伺服电缸推料机构"),
    ("DWG-FJS-BJ-105", "视觉检测治具"),
    ("DWG-FJS-BJ-106", "链板线体"),
    ("DWG-FJS-BJ-107", "气路集成阀岛"),
    ("DWG-FJS-BJ-201", "提升机框架"),
    ("DWG-FJS-BJ-202", "移载机构"),
    ("DWG-FJS-BJ-203", "挡停单元"),
    ("DWG-FJS-BJ-204", "桁架机械手"),
    ("DWG-FJS-LJ-3001", "轴承座"),
    ("DWG-FJS-LJ-3002", "法兰盘"),
    ("DWG-FJS-LJ-3003", "导向轴"),
    ("DWG-FJS-LJ-3004", "张紧板"),
    ("DWG-FJS-LJ-3005", "连接梁"),
    ("DWG-FJS-LJ-3006", "支撑角码"),
    ("DWG-FJS-LJ-3007", "电机安装板"),
    ("DWG-FJS-LJ-3008", "皮带压板"),
    ("DWG-FJS-LJ-3009", "传感器支架"),
    ("DWG-FJS-LJ-3010", "挡块"),
    ("DWG-FJS-LJ-3011", "护罩板"),
    ("DWG-FJS-LJ-3012", "限位座"),
    ("DWG-FJS-LJ-3013", "联轴器护套"),
    ("DWG-FJS-LJ-3014", "拖链支架"),
    ("DWG-FJS-LJ-3015", "光电安装座"),
    ("DWG-FJS-LJ-3016", "气缸固定件"),
    ("DWG-FJS-LJ-3017", "调节螺栓座"),
    ("DWG-FJS-LJ-3018", "线缆桥架"),
    ("DWG-FJS-LJ-3019", "端子排安装条"),
    ("DWG-FJS-LJ-3020", "铭牌底板"),
    ("DWG-FJS-LJ-3021", "接地端子板"),
    ("DWG-FJS-LJ-3022", "线槽卡扣"),
];

/// 外购件语料（CLS-WG 自动赋码）
const WG_PARTS: [&str; 20] = [
    "直线导轨 HGR25",
    "滚珠丝杆 SFU3210",
    "伺服电机 750W",
    "减速机 PLE60",
    "气动三联件",
    "电磁阀组 5 通",
    "光电传感器",
    "接近开关 PNP",
    "PLC S7-1200",
    "触摸屏 7 寸",
    "变频器 2.2kW",
    "开关电源 24V",
    "轴承 6205",
    "同步带 5M-1000",
    "同步轮 5M-24T",
    "链条 C2082",
    "铝型材 4040",
    "拖链 25 系列",
    "端子排 16 位",
    "航空插头 12 芯",
];

/// 自制件语料（CLS-ZZ 自动赋码）
const ZZ_PARTS: [&str; 28] = [
    "机架型材组件",
    "安装面板",
    "轴承座总成",
    "治具板",
    "定位块",
    "支撑座",
    "导向块",
    "压紧机构座",
    "调节座",
    "连接板",
    "加强筋板",
    "电机座",
    "丝杆固定座",
    "皮带张紧座",
    "传感支架",
    "线槽支架",
    "端子安装条",
    "护罩骨架",
    "铰链座",
    "锁紧把手座",
    "法兰连接件",
    "转接板",
    "垫铁",
    "限位挡板",
    "拖链固定座",
    "接地桩",
    "铭牌托架",
    "缓冲垫块",
];

pub struct SeedResult {
    pub users: usize,
    pub drawings: usize,
    pub parts: usize,
    pub bom_tops: usize,
    pub review: usize,
    pub released: usize,
    pub locks: usize,
    pub links: usize,
}

/// 幂等入口：sys_param demo_seeded 存在即跳过（返回 None）
pub async fn run_if_needed(pool: &PgPool, ctx: &SeedCtx) -> anyhow::Result<Option<SeedResult>> {
    let done: Option<String> = sqlx::query_scalar("SELECT value FROM sys_param WHERE key = $1")
        .bind(SEED_FLAG_KEY)
        .fetch_optional(pool)
        .await?;
    if done.is_some() {
        return Ok(None);
    }
    let r = run(ctx).await?;
    sqlx::query(
        "INSERT INTO sys_param(key, value, updated_by) VALUES ($1, $2, NULL) \
         ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = now()",
    )
    .bind(SEED_FLAG_KEY)
    .bind("1")
    .execute(pool)
    .await?;
    Ok(Some(r))
}

/// 全量生成（调用方保证幂等前置检查已完成）
pub async fn run(ctx: &SeedCtx) -> anyhow::Result<SeedResult> {
    let t0 = std::time::Instant::now();
    let pool = &ctx.pool;
    let mut users = 0usize;

    // ---- ① 员工（argon2 一次散列复用统一口令；department 直填；幂等 upsert）----
    let hash = core_sec::hash_password_public(DEMO_PASSWORD)?;
    for dept in &DEPTS {
        for (i, name) in dept.names.iter().take(dept.count).enumerate() {
            let username = format!("{}{:02}", dept.code, i + 1);
            sqlx::query(
                "INSERT INTO sec_user(username, password_hash, display_name, max_secret_level, department) \
                 VALUES ($1, $2, $3, $4, $5) \
                 ON CONFLICT (lower(username)) DO UPDATE SET department = $5, display_name = $3",
            )
            .bind(&username)
            .bind(&hash)
            .bind(name)
            .bind(dept.sec_level)
            .bind(dept.name)
            .execute(pool)
            .await?;
            for r in dept.roles {
                sqlx::query(
                    "INSERT INTO sec_user_role(username, role_code) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                )
                .bind(&username)
                .bind(r)
                .execute(pool)
                .await?;
            }
            users += 1;
        }
    }

    // 用户 id 索引（按 username；含种子 admin）
    let mut ids: HashMap<String, uuid::Uuid> = HashMap::new();
    let rows: Vec<(String, uuid::Uuid)> = sqlx::query_as("SELECT username, id FROM sec_user")
        .fetch_all(pool)
        .await?;
    for (u, id) in rows {
        ids.insert(u, id);
    }
    let uid = |u: &str| -> uuid::Uuid { ids.get(u).copied().unwrap_or_else(|| ids["sj01"]) };
    let designers: Vec<String> = (5..=12).map(|i| format!("sj{i:02}")).collect();
    let buyers: Vec<String> = vec!["cg02".into(), "cg03".into(), "cc01".into(), "cc02".into()];
    let mut buyer_idx = 0usize;

    // ---- ② 图纸 36（working；显式编号；create 人=设计部轮换；stage_file 真实落盘可下载）----
    let mut drawings_created = 0usize;
    for (designer_idx, (no, name)) in DRAWINGS.into_iter().enumerate() {
        let creator = &designers[designer_idx % designers.len()];
        let content = format!("%DWG-DEMO-{no}-非标自动化演示数据");
        let mut reader = std::io::Cursor::new(content.into_bytes());
        let (hash, size) = ctx.service.stage_file(&mut reader).await?;
        let mut attrs = AttrMap::new();
        attrs.insert("name".into(), json!(name));
        attrs.insert("material".into(), json!("Q235"));
        match ctx
            .service
            .upload_staged(
                mod_doc::StagedUpload {
                    obj_no: no.to_string(),
                    attrs,
                    file_name: format!("{no}.dwg"),
                    mime: Some("application/octet-stream".into()),
                    hash,
                    size,
                },
                Some(uid(creator)),
            )
            .await
        {
            Ok(_) => drawings_created += 1,
            Err(DomainError::ObjNoDuplicate(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }

    // ---- ③ 物料 48（CLS-ZZ/CLS-WG 自动赋码；create 人=采购/仓储轮换）----
    let mut zz_nos: Vec<String> = Vec::new();
    let mut wg_nos: Vec<String> = Vec::new();
    for name in ZZ_PARTS {
        // 终审 I1-2：按名称查重跳过（自动赋码永不撞号——ObjNoDuplicate 死防御；
        // 部分失败重跑防物料翻倍）
        let exists: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM cm_object o JOIN cm_type t ON t.id=o.type_id              WHERE t.code='part' AND o.deleted_at IS NULL AND o.attrs->>'name' = $1",
        )
        .bind(name)
        .fetch_one(pool)
        .await?;
        if exists > 0 {
            continue;
        }
        let creator = &buyers[buyer_idx % buyers.len()];
        buyer_idx += 1;
        let mut attrs = AttrMap::new();
        attrs.insert("name".into(), json!(name));
        attrs.insert("material".into(), json!("Q235"));
        match ctx.parts.create("CLS-ZZ", attrs, Some(uid(creator))).await {
            Ok(r) => zz_nos.push(r.obj_no),
            Err(DomainError::ObjNoDuplicate(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    for name in WG_PARTS {
        let exists: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM cm_object o JOIN cm_type t ON t.id=o.type_id              WHERE t.code='part' AND o.deleted_at IS NULL AND o.attrs->>'name' = $1",
        )
        .bind(name)
        .fetch_one(pool)
        .await?;
        if exists > 0 {
            continue;
        }
        let creator = &buyers[buyer_idx % buyers.len()];
        buyer_idx += 1;
        let mut attrs = AttrMap::new();
        attrs.insert("name".into(), json!(name));
        match ctx.parts.create("CLS-WG", attrs, Some(uid(creator))).await {
            Ok(r) => wg_nos.push(r.obj_no),
            Err(DomainError::ObjNoDuplicate(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let parts_created = zz_nos.len() + wg_nos.len();

    // ---- ④ BOM 3 台设备（总装→部件→零件/物料；深度 3；set_lines 真实闭包路径）----
    let line = |n: usize, child: uuid::Uuid, qty: i64| core_bom::LineInput {
        line_no: n as i32,
        child,
        qty,
    };
    async fn id_of(pool: &PgPool, no: &str) -> anyhow::Result<uuid::Uuid> {
        Ok(sqlx::query_scalar(
            "SELECT id FROM cm_object WHERE obj_no = $1 AND deleted_at IS NULL \
             ORDER BY major_ver DESC LIMIT 1",
        )
        .bind(no)
        .fetch_one(pool)
        .await?)
    }
    let zp1 = id_of(pool, "DWG-FJS-ZP-01").await?;
    let zp2 = id_of(pool, "DWG-FJS-ZP-02").await?;
    let bj101 = id_of(pool, "DWG-FJS-BJ-101").await?;
    let bj102 = id_of(pool, "DWG-FJS-BJ-102").await?;
    let bj103 = id_of(pool, "DWG-FJS-BJ-103").await?;
    let lj3015 = id_of(pool, "DWG-FJS-LJ-3015").await?;
    let lj3016 = id_of(pool, "DWG-FJS-LJ-3016").await?;
    let lj3017 = id_of(pool, "DWG-FJS-LJ-3017").await?;
    let lj3013 = id_of(pool, "DWG-FJS-LJ-3013").await?;
    let lj3019 = id_of(pool, "DWG-FJS-LJ-3019").await?;
    let wg0 = id_of(pool, &wg_nos[0]).await?;
    let wg1 = id_of(pool, &wg_nos[1]).await?;
    let wg2 = id_of(pool, &wg_nos[2]).await?;
    let wg3 = id_of(pool, &wg_nos[3]).await?;
    let zz0 = id_of(pool, &zz_nos[0]).await?;
    let zz1 = id_of(pool, &zz_nos[1]).await?;
    let zz2 = id_of(pool, &zz_nos[2]).await?;
    // 线 1：环形输送线 = 顶升单元×4 + 机架段×6 + 光电支架×8 + 导轨×12
    ctx.bom
        .set_lines(
            zp1,
            vec![
                line(1, bj101, 4),
                line(2, bj102, 6),
                line(3, lj3015, 8),
                line(4, wg0, 12),
            ],
        )
        .await?;
    // 线 2：顶升单元 = 传动模块×1 + 轴承座总成×2 + 丝杆×2 + 电磁阀×1
    ctx.bom
        .set_lines(
            bj101,
            vec![
                line(1, bj103, 1),
                line(2, zz0, 2),
                line(3, wg1, 2),
                line(4, wg2, 1),
            ],
        )
        .await?;
    // 线 3：传动模块 = 联轴器护套×1 + 端子条×1
    ctx.bom
        .set_lines(bj103, vec![line(1, lj3013, 1), line(2, lj3019, 1)])
        .await?;
    // 线 4：视觉分拣站 = 治具板×2 + 光电×4 + 铝型材×4
    ctx.bom
        .set_lines(
            zp2,
            vec![
                line(1, lj3016, 4),
                line(2, wg3, 4),
                line(3, zz1, 4),
                line(4, zz2, 4),
            ],
        )
        .await?;
    // 线 5：机架段 = 调节座×3 + 拖链×2
    ctx.bom
        .set_lines(
            bj102,
            vec![
                line(1, lj3017, 3),
                line(2, id_of(pool, &wg_nos[17]).await?, 2),
            ],
        )
        .await?;
    let bom_tops = 3usize;

    // ---- ⑤ 生命周期分布：review ×8 / released ×6（含 2 件 revise v2）/ obsolete ×1 ----
    let to_review: [&str; 8] = [
        "DWG-FJS-LJ-3001",
        "DWG-FJS-LJ-3002",
        "DWG-FJS-LJ-3003",
        "DWG-FJS-LJ-3004",
        "DWG-FJS-LJ-3005",
        "DWG-FJS-BJ-201",
        "DWG-FJS-BJ-202",
        "DWG-FJS-BJ-203",
    ];
    let submitters = [
        "sj01", "sj02", "sj03", "sj04", "sj05", "dq01", "dq02", "sj06",
    ];
    for (no, u) in to_review.iter().zip(submitters.iter()) {
        ctx.service
            .transition(
                &ctx.lifecycle,
                core_version::LifecycleEvent::Submit,
                no,
                uid(u),
            )
            .await?;
    }
    let to_release: [&str; 6] = [
        "DWG-FJS-ZP-01",
        "DWG-FJS-BJ-101",
        "DWG-FJS-LJ-3006",
        "DWG-FJS-LJ-3007",
        "DWG-FJS-LJ-3008",
        "DWG-FJS-LJ-3009",
    ];
    for no in to_release {
        ctx.service
            .transition(
                &ctx.lifecycle,
                core_version::LifecycleEvent::Submit,
                no,
                uid("sj01"),
            )
            .await?;
        ctx.service
            .transition(
                &ctx.lifecycle,
                core_version::LifecycleEvent::Approve,
                no,
                uid("zb01"),
            )
            .await?;
    }
    // 两件 released 升版出 v2 working
    for no in ["DWG-FJS-LJ-3006", "DWG-FJS-LJ-3007"] {
        ctx.service
            .transition(
                &ctx.lifecycle,
                core_version::LifecycleEvent::Revise,
                no,
                uid("sj02"),
            )
            .await?;
    }
    ctx.service
        .transition(
            &ctx.lifecycle,
            core_version::LifecycleEvent::Submit,
            "DWG-FJS-LJ-3010",
            uid("sj03"),
        )
        .await?;
    ctx.service
        .transition(
            &ctx.lifecycle,
            core_version::LifecycleEvent::Approve,
            "DWG-FJS-LJ-3010",
            uid("zb02"),
        )
        .await?;
    ctx.service
        .transition(
            &ctx.lifecycle,
            core_version::LifecycleEvent::Obsolete,
            "DWG-FJS-LJ-3010",
            uid("zb01"),
        )
        .await?;

    // ---- ⑥ 检出锁 ×2（设计部工程师各检出自己部件图）----
    ctx.service
        .checkout("DWG-FJS-BJ-104", uid("sj07"), "黄文涛", 24)
        .await?;
    ctx.service
        .checkout("DWG-FJS-BJ-105", uid("sj08"), "周立军", 24)
        .await?;
    let locks = 2usize;

    // ---- ⑦ 关联 ×12（零件图 ↔ 自制物料；直插 obj_link ON CONFLICT DO NOTHING）----
    let mut links = 0usize;
    let pairs: [(&str, usize); 12] = [
        ("DWG-FJS-LJ-3001", 0),
        ("DWG-FJS-LJ-3002", 1),
        ("DWG-FJS-LJ-3003", 2),
        ("DWG-FJS-LJ-3006", 3),
        ("DWG-FJS-LJ-3007", 4),
        ("DWG-FJS-LJ-3008", 5),
        ("DWG-FJS-LJ-3009", 6),
        ("DWG-FJS-LJ-3011", 7),
        ("DWG-FJS-LJ-3012", 8),
        ("DWG-FJS-LJ-3013", 9),
        ("DWG-FJS-LJ-3014", 10),
        ("DWG-FJS-LJ-3018", 11),
    ];
    for (dno, zi) in pairs {
        let from = id_of(pool, dno).await?;
        let to = id_of(pool, &zz_nos[zi % zz_nos.len()]).await?;
        let r = sqlx::query(
            "INSERT INTO obj_link(from_id, to_id, kind, created_by) VALUES ($1, $2, 'doc_part', $3) ON CONFLICT DO NOTHING",
        )
        .bind(from)
        .bind(to)
        .bind(uid("sj01"))
        .execute(pool)
        .await?;
        links += r.rows_affected() as usize;
    }

    // ---- ⑧ 审计收尾 ----
    ctx.audit.record(
        "admin",
        "demo.seed",
        None,
        Some(json!({
            "users": users,
            "drawings": drawings_created,
            "parts": parts_created,
            "bom_tops": bom_tops,
            "review": to_review.len(),
            "released": to_release.len(),
            "locks": locks,
            "links": links,
            "elapsed_ms": t0.elapsed().as_millis() as u64,
        })),
        None,
        None,
    );
    Ok(SeedResult {
        users,
        drawings: drawings_created,
        parts: parts_created,
        bom_tops,
        review: to_review.len(),
        released: to_release.len(),
        locks,
        links,
    })
}

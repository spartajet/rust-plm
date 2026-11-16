//! Demo 数据包 e2e（增量28）：seed::run 全链真实业务路径 + 幂等 + 演示账号权限 + 数据地图断言。

// 独立 binary 引共享 common——其余 e2e 专用的辅助函数在此 target 内 dead_code（噪音豁免）
#![allow(dead_code)]

mod common;

use plm_server::seed::{run_if_needed, SeedCtx};

#[tokio::test]
async fn demo_seed_full_package_and_idempotent() {
    let ctx = common::TestCtx::setup("dseed", 8 * 1024 * 1024).await;
    let sctx = SeedCtx {
        pool: ctx.pool.clone(),
        service: ctx.demo_service.clone(),
        parts: ctx.demo_parts.clone(),
        bom: ctx.demo_bom.clone(),
        lifecycle: ctx.demo_lifecycle.clone(),
        audit: ctx.demo_audit.clone(),
    };

    // ① 首次生成：Some(SeedResult)
    let r = run_if_needed(&ctx.pool, &sctx).await.unwrap();
    let r = r.expect("首次生成必须 Some");

    // ② 人员：100（99 部门员工+种子 admin）；部门覆盖/计数
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM sec_user")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(total, 100, "99 部门员工+种子 admin");
    let covered: i64 = sqlx::query_scalar("SELECT count(*) FROM sec_user WHERE department <> ''")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(covered, 99, "部门覆盖除 admin 外全量");
    let sj: i64 = sqlx::query_scalar("SELECT count(*) FROM sec_user WHERE department='机械设计部'")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(sj, 22, "机械设计部 22 人");
    // 统一口令可登录 + 角色正确（sj01=editor 密级 2）
    let (s, body) = ctx.login_post("sj01", "demo-pass-123").await;
    assert_eq!(
        s,
        200,
        "统一口令登录 body={}",
        String::from_utf8_lossy(&body)
    );

    // ③ 数据地图：图纸 36 / 物料 48
    let drawings: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cm_object o JOIN cm_type t ON t.id=o.type_id WHERE t.code='drawing' AND o.deleted_at IS NULL",
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(drawings, 36, "图纸 36");
    let parts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cm_object o JOIN cm_type t ON t.id=o.type_id WHERE t.code='part' AND o.deleted_at IS NULL",
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(parts, 48, "物料 48");
    assert_eq!(r.drawings, 36);
    assert_eq!(r.parts, 48);

    // ④ 生命周期分布：review 8 / released 6 / obsolete 1 / v2 working 2
    async fn state_of(pool: &sqlx::PgPool, s: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM cm_object o JOIN cm_type t ON t.id=o.type_id \
             WHERE t.code='drawing' AND o.deleted_at IS NULL AND o.state = $1",
        )
        .bind(s)
        .fetch_one(pool)
        .await
        .unwrap()
    }
    assert_eq!(state_of(&ctx.pool, "review").await, 8);
    let released_total: i64 = sqlx::query_scalar("SELECT count(*) FROM cm_obj_snapshot")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(
        released_total, 7,
        "6 件 released + 1 件 obsolete 前 approve（3010）各一快照；revise 同 id 不新增"
    );
    assert_eq!(state_of(&ctx.pool, "obsolete").await, 1);
    let v2: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cm_object WHERE obj_no IN ('DWG-FJS-LJ-3006','DWG-FJS-LJ-3007') AND major_ver = 2",
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(v2, 2, "两件 released 升版出 v2");

    // ⑤ BOM：总装展开非空（深度 3 闭包）；部件级展开含外购件
    let zp1 = sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT id FROM cm_object WHERE obj_no='DWG-FJS-ZP-01' AND deleted_at IS NULL LIMIT 1",
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    let expand = ctx
        .demo_bom
        .expand(zp1, core_bom::ExpandMode::Full)
        .await
        .unwrap();
    assert!(
        expand.len() >= 8,
        "总装闭包含部件+跨级零件/物料，实际 {}",
        expand.len()
    );
    let wg_first =
        sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM cm_object WHERE obj_no = $1 LIMIT 1")
            .bind("WL-WG-00001")
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    let wg_in = expand.iter().any(|row| row.child == wg_first);
    assert!(wg_in, "跨级外购件在闭包内");

    // ⑥ 锁与关联
    let locks: i64 = sqlx::query_scalar("SELECT count(*) FROM doc_lock")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(locks, 2);
    let links: i64 = sqlx::query_scalar("SELECT count(*) FROM obj_link")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(links, 12);

    // ⑦ 部门筛选 API（list_users department 参数）
    let (s, body) = ctx
        .get_authed(
            "/api/v1/sec/users?department=%E7%94%B5%E6%B0%94%E8%AE%BE%E8%AE%A1%E9%83%A8",
            &ctx.admin_login().await,
        )
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 12, "电气设计部 12 人筛选");

    // ⑧ 幂等：二次 run_if_needed → None（标记）；计数不变
    let again = run_if_needed(&ctx.pool, &sctx).await.unwrap();
    assert!(again.is_none(), "demo_seeded 标记后跳过");
    let total2: i64 = sqlx::query_scalar("SELECT count(*) FROM sec_user")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(total2, 100, "幂等重跑计数不变");

    ctx.cleanup().await;
}

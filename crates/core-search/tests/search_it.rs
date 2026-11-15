//! core-search 集成测试（增量20 Task1）：临时真库 + 临时索引目录。
//! 覆盖：全量重建/聚类/子串（bigram）/编号精确/ABAC 密级双保险/增量扫描/多实例 degraded。

use core_search::{SearchService, SEARCH_TYPES};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(hint: &str) -> String {
    let hint: String = hint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(
        "cs_{}_{}",
        hint,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

async fn setup(hint: &str) -> (PgPool, String, String) {
    let admin_url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL");
    let db = unique_db(hint);
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    let url = format!("{}/{}", admin_url.trim_end_matches("/postgres"), db);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    core_model::migrator().run(&pool).await.unwrap();
    mod_doc::migrator().run(&pool).await.unwrap();
    (pool, url, db)
}

async fn insert_obj(pool: &PgPool, type_code: &str, obj_no: &str, name: &str, secret: i16) {
    sqlx::query(
        "INSERT INTO cm_object(type_id, obj_no, attrs, secret_level, created_by, updated_by) \
         SELECT id, $2, $3::jsonb, $4, NULL, NULL FROM cm_type WHERE code = $1",
    )
    .bind(type_code)
    .bind(obj_no)
    .bind(serde_json::json!({"name": name, "material": "Q235"}).to_string())
    .bind(secret)
    .execute(pool)
    .await
    .unwrap();
}

fn tmp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "cs_idx_{tag}_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test]
async fn rebuild_cluster_substring_abac_and_scan() {
    let (pool, _url, db) = setup("main").await;
    insert_obj(&pool, "drawing", "DWG-S1", "六角轴承座", 0).await;
    insert_obj(&pool, "drawing", "DWG-S2", "端盖", 2).await;
    insert_obj(&pool, "part", "P-1", "六角轴承", 0).await;
    insert_obj(&pool, "part", "P-2", "保密轴承", 2).await;

    let dir = tmp_dir("main");
    let svc = SearchService::open(&dir).unwrap();
    assert!(!svc.degraded());
    let (n, _wm) = svc.rebuild_full(&pool).await.unwrap();
    assert_eq!(n, 4);

    // 索引侧钉：中文词/拉丁词均入索引（小写化）
    assert_eq!(svc.raw_text_term_hits("轴承").unwrap(), 3);
    assert_eq!(svc.raw_text_term_hits("q235").unwrap(), 4);
    assert_eq!(svc.raw_text_term_hits("Q235").unwrap(), 0, "统一小写");
    // ① 聚类：双类型命中；端盖不命中
    let groups = svc.search_groups(&pool, "轴承", 3, 10).await.unwrap();
    let drawing = groups.iter().find(|g| g.type_code == "drawing").unwrap();
    let part = groups.iter().find(|g| g.type_code == "part").unwrap();
    assert_eq!(drawing.total, 1, "drawing 组恰 DWG-S1");
    assert_eq!(drawing.items[0].obj_no, "DWG-S1");
    assert_eq!(drawing.items[0].name.as_deref(), Some("六角轴承座"));
    assert!(
        !drawing.items[0].obj_id.is_empty(),
        "obj_id 随索引回填（增量21 编辑器选件依赖）"
    );
    assert_eq!(drawing.items[0].state, "working");
    assert_eq!(part.total, 2, "part 组 P-1/P-2");
    assert_eq!(groups.len(), SEARCH_TYPES.len());

    // ② 子串（jieba 未收录「承座」，bigram 兜底命中）
    let groups = svc.search_groups(&pool, "承座", 3, 10).await.unwrap();
    let drawing = groups.iter().find(|g| g.type_code == "drawing").unwrap();
    assert_eq!(drawing.total, 1);
    assert_eq!(drawing.items[0].obj_no, "DWG-S1");

    // ③ 编号精确（term）
    let (items, total) = svc
        .search_page(&pool, "DWG-S1", "drawing", 3, 1, 10)
        .await
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(items[0].obj_no, "DWG-S1");

    // ④ ABAC：上限 0 → 密级 2 的「保密轴承」不可见（查询侧过滤+stored 复核双保险）
    let groups = svc.search_groups(&pool, "轴承", 0, 10).await.unwrap();
    let part = groups.iter().find(|g| g.type_code == "part").unwrap();
    assert_eq!(part.total, 1, "上限 0 仅 P-1");
    assert_eq!(part.items[0].obj_no, "P-1");
    // 钻取分页同样过滤
    let (_it, t) = svc
        .search_page(&pool, "轴承", "part", 0, 1, 10)
        .await
        .unwrap();
    assert_eq!(t, 1);

    // ⑤ 增量扫描：新对象入库 → scan_once 回放 → 可检索
    insert_obj(&pool, "drawing", "DWG-S3", "同步带轮", 0).await;
    let mut wm = chrono::Utc::now() - chrono::Duration::hours(1);
    let replayed = svc.scan_once(&pool, &mut wm).await.unwrap();
    assert!(replayed >= 4, "水位早于全部行——至少回放既有 4 件");
    let (it, t) = svc
        .search_page(&pool, "带轮", "drawing", 3, 1, 10)
        .await
        .unwrap();
    assert_eq!(t, 1);
    assert_eq!(it[0].obj_no, "DWG-S3");
    // 水位推进后再扫：重叠窗内既有行幂等重放（≤4），结果不变
    let replayed2 = svc.scan_once(&pool, &mut wm).await.unwrap();
    assert!(
        replayed2 <= 5,
        "重叠窗幂等重放不扩大（4 既有+1 新增全在窗内）"
    );
    let (_it2, t2) = svc
        .search_page(&pool, "带轮", "drawing", 3, 1, 10)
        .await
        .unwrap();
    assert_eq!(t2, 1, "重放不改结果");

    // 确定性零回放：插入 updated_at 在水位 +10s 的行 → 扫描收编并推水位 → 再扫为 0
    sqlx::query(
        "INSERT INTO cm_object(type_id, obj_no, attrs, secret_level, updated_at, created_by, updated_by)          SELECT id, 'DWG-S4', '{\"name\":\"同步带轮2\"}'::jsonb, 0, $1, NULL, NULL FROM cm_type WHERE code='drawing'",
    )
    .bind(wm + chrono::Duration::seconds(10))
    .execute(&pool)
    .await
    .unwrap();
    let replayed3 = svc.scan_once(&pool, &mut wm).await.unwrap();
    assert!(replayed3 >= 1, "future 行收编（窗含既有重叠行）");
    let (it3, t3) = svc
        .search_page(&pool, "带轮2", "drawing", 3, 1, 10)
        .await
        .unwrap();
    assert_eq!(t3, 2, "OR 析取：同步带轮/同步带轮2 双命中");
    assert!(it3.iter().any(|i| i.obj_no == "DWG-S4"));
    let replayed4 = svc.scan_once(&pool, &mut wm).await.unwrap();
    assert_eq!(replayed4, 0, "水位越过全部行——零回放");

    pool.close().await;
    let _ = sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(
            &PgPoolOptions::new()
                .max_connections(1)
                .connect(&_url)
                .await
                .unwrap(),
        )
        .await;
}

#[tokio::test]
async fn multi_instance_second_open_is_degraded_readonly() {
    let (pool, _url, db) = setup("degraded").await;
    insert_obj(&pool, "drawing", "DWG-D1", "减速器壳体", 0).await;
    let dir = tmp_dir("degraded");
    let svc1 = SearchService::open(&dir).unwrap();
    svc1.rebuild_full(&pool).await.unwrap();

    // 同目录再开：写锁被占 → degraded；查询仍可（只读 reader）
    let svc2 = SearchService::open(&dir).unwrap();
    assert!(svc2.degraded(), "第二实例必须 degraded");
    assert!(
        svc2.rebuild_full(&pool).await.is_err(),
        "degraded 写入必须报错"
    );
    let groups = svc2.search_groups(&pool, "减速器", 3, 10).await.unwrap();
    let drawing = groups.iter().find(|g| g.type_code == "drawing").unwrap();
    assert_eq!(drawing.total, 1, "degraded 实例查询仍可用");
    assert_eq!(drawing.items[0].obj_no, "DWG-D1");

    pool.close().await;
    let _ = sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(
            &PgPoolOptions::new()
                .max_connections(1)
                .connect(&_url)
                .await
                .unwrap(),
        )
        .await;
}

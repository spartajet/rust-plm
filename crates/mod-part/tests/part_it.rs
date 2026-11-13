//! mod-part 集成测试（增量17 Task1）：真 PG18 库。覆盖 Review Focus #1/#2/#5。

use core_model::{HookRegistry, PgObjectStore, TypeRegistry};
use mod_part::PartService;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(hint: &str) -> String {
    let hint: String = hint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(
        "pt_{}_{}",
        hint,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

async fn setup(hint: &str) -> (Arc<PartService>, PgPool, String, String) {
    let admin_url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL");
    let db = unique_db(hint);
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE \"{db}\""))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let prefix = admin_url
        .rsplit_once('/')
        .map(|(p, _)| p)
        .unwrap()
        .to_string();
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect(&format!("{prefix}/{db}"))
        .await
        .unwrap();
    core_model::migrator().run(&pool).await.unwrap();
    mod_part::migrator().run(&pool).await.unwrap();
    let registry = TypeRegistry::load(pool.clone()).await.unwrap();
    let hooks = Arc::new(HookRegistry::new());
    let objects = Arc::new(PgObjectStore::new(pool.clone(), registry.clone(), hooks));
    let svc = Arc::new(PartService::new(pool.clone(), objects));
    (svc, pool, admin_url, db)
}

async fn drop_db(admin_url: &str, db: &str) {
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(admin_url)
        .await
        .unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn create_assigns_class_scoped_codes() {
    // Review Focus #1：不同 class 独立流水（ZZ 与 JGJ 各自 00001）
    let (svc, pool, admin_url, db) = setup("cc").await;
    let r1 = svc
        .create(
            "CLS-ZZ",
            serde_json::json!({"name":"支座","material":"Q235"})
                .as_object()
                .unwrap()
                .clone(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(r1.obj_no, "WL-ZZ-00001");
    assert_eq!(r1.class_code, "CLS-ZZ");
    let r2 = svc
        .create(
            "CLS-ZZ",
            serde_json::json!({"name":"支座2"})
                .as_object()
                .unwrap()
                .clone(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(r2.obj_no, "WL-ZZ-00002");
    // 子分类独立流水
    let r3 = svc
        .create(
            "CLS-ZZ-JGJ",
            serde_json::json!({"name":"螺栓"})
                .as_object()
                .unwrap()
                .clone(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(r3.obj_no, "WL-ZZ-JGJ-00001");
    // attrs 回填分类上下文
    let obj = svc.list(Some("支座"), 1, 50, 3).await.unwrap().0;
    let first = obj.iter().find(|o| o.obj_no == r1.obj_no).unwrap();
    assert_eq!(
        first.attrs.get("class_code").and_then(|v| v.as_str()),
        Some("CLS-ZZ")
    );
    pool.close().await;
    drop_db(&admin_url, &db).await;
}

#[tokio::test]
async fn duplicates_returns_similar_excluding_self() {
    // Review Focus #2：查重命中同名/同材，创建响应带 duplicates 且排除自身
    let (svc, pool, admin_url, db) = setup("dup").await;
    let r1 = svc
        .create(
            "CLS-ZZ",
            serde_json::json!({"name":"法兰盘","material":"304"})
                .as_object()
                .unwrap()
                .clone(),
            None,
        )
        .await
        .unwrap();
    assert!(r1.duplicates.is_empty(), "首个无相似");
    let r2 = svc
        .create(
            "CLS-ZZ",
            serde_json::json!({"name":"法兰盘加强型","material":"304"})
                .as_object()
                .unwrap()
                .clone(),
            None,
        )
        .await
        .unwrap();
    assert!(
        r2.duplicates.iter().any(|d| d.obj_no == r1.obj_no),
        "同名前缀须命中: {:?}",
        r2.duplicates
    );
    assert!(
        !r2.duplicates.iter().any(|d| d.obj_no == r2.obj_no),
        "排除自身"
    );
    let d = svc.duplicates("法兰", None).await.unwrap();
    assert!(d.len() >= 2);
    pool.close().await;
    drop_db(&admin_url, &db).await;
}

#[tokio::test]
async fn class_tree_nests_children() {
    // Review Focus #5：树形嵌套（自制件 → 紧固件/钣金件）
    let (svc, pool, admin_url, db) = setup("tree").await;
    let tree = svc.class_tree().await.unwrap();
    assert!(tree.len() >= 2, "两个根: {tree:?}");
    let zz = tree.iter().find(|n| n.code == "CLS-ZZ").unwrap();
    assert_eq!(zz.children.len(), 2, "两个子分类");
    assert!(zz
        .children
        .iter()
        .any(|c| c.code == "CLS-ZZ-JGJ" && c.has_rule));
    pool.close().await;
    drop_db(&admin_url, &db).await;
}

#[tokio::test]
async fn unknown_class_and_missing_rule_are_400() {
    let (svc, pool, admin_url, db) = setup("err").await;
    let e = svc
        .create(
            "CLS-NOPE",
            serde_json::json!({"name":"x"}).as_object().unwrap().clone(),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-BAD-REQUEST");
    // 无规则分类：造一个无规则分类
    sqlx::query("INSERT INTO cm_class(code, name) VALUES ('CLS-NR', '无规则')")
        .execute(&pool)
        .await
        .unwrap();
    let e = svc
        .create(
            "CLS-NR",
            serde_json::json!({"name":"x"}).as_object().unwrap().clone(),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-BAD-REQUEST");
    pool.close().await;
    drop_db(&admin_url, &db).await;
}

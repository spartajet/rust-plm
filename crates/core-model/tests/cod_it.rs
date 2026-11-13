//! COD 编码系统集成测试（增量16 Task1）：规则解析/渲染/并发取号/日期滚动/create 纵切。

use core_model::cod::{next_code, CodRule};
use core_model::ObjectStore;
use core_model::{CreateCmd, HookRegistry, PgObjectStore, TypeRegistry};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(hint: &str) -> String {
    let hint: String = hint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(
        "cod_{}_{}",
        hint,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

async fn setup(hint: &str, rule_json: &str) -> (PgPool, std::path::PathBuf, String, String) {
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
        .max_connections(8)
        .connect(&format!("{prefix}/{db}"))
        .await
        .unwrap();
    core_model::migrator().run(&pool).await.unwrap();
    sqlx::query("INSERT INTO cm_type(code, name, base_type, config) VALUES ('drawing','图纸','generic', $1::jsonb)")
        .bind(rule_json)
        .execute(&pool)
        .await
        .unwrap();
    let registry = TypeRegistry::load(pool.clone()).await.unwrap();
    let root = std::env::temp_dir().join(format!("cod-{db}"));
    let hooks = Arc::new(HookRegistry::new());
    let _store = PgObjectStore::new(pool.clone(), registry.clone(), hooks);
    (pool, root, admin_url, db)
}

// ---------- 纯函数：规则渲染 ----------

#[test]
fn rule_render_prefix_date_seq() {
    let rule = CodRule {
        prefix: "DWG-".into(),
        date_fmt: "yyyyMMdd".into(),
        seq_digits: 4,
    };
    let code = rule.render("20261113", 1);
    assert_eq!(code, "DWG-20261113-0001");
    assert_eq!(rule.render("20261113", 12345), "DWG-20261113-12345");
    let no_date = CodRule {
        prefix: "P".into(),
        date_fmt: "none".into(),
        seq_digits: 3,
    };
    assert_eq!(no_date.render("ignored", 7), "P007");
}

#[test]
fn rule_parse_from_config_json() {
    let v = serde_json::json!({"cod_rule":{"prefix":"DWG-","date_fmt":"yyyyMMdd","seq_digits":4}});
    let rule = CodRule::from_config(&v).unwrap();
    assert_eq!(rule.prefix, "DWG-");
    assert_eq!(rule.date_fmt, "yyyyMMdd");
    assert_eq!(rule.seq_digits, 4);
    // 缺 cod_rule 键 → None（类型不参与自动赋码）
    assert!(CodRule::from_config(&serde_json::json!({"prefix":"X"})).is_none());
    assert!(CodRule::from_config(&serde_json::json!({})).is_none());
}

// ---------- 集成：计数器/并发/日期滚动/create 纵切 ----------

#[tokio::test]
async fn concurrent_next_code_no_duplicates() {
    let (pool, _, admin_url, db) = setup(
        "cc",
        r#"{"cod_rule":{"prefix":"DWG-","date_fmt":"yyyyMMdd","seq_digits":4}}"#,
    )
    .await;
    let rule = CodRule {
        prefix: "DWG-".into(),
        date_fmt: "yyyyMMdd".into(),
        seq_digits: 4,
    };
    let today = "20261113";
    let handles: Vec<_> = (0..20)
        .map(|_| {
            let pool = pool.clone();
            let rule = rule.clone();
            tokio::spawn(async move { next_code(&pool, "drawing", &rule, today).await.unwrap() })
        })
        .collect();
    let mut codes = HashSet::new();
    for h in handles {
        codes.insert(h.await.unwrap());
    }
    assert_eq!(codes.len(), 20, "20 并发取号必须互异");
    // 连续无断号（1..=20）
    for i in 1..=20 {
        assert!(codes.contains(&rule.render(today, i)), "缺号 {i}");
    }
    pool.close().await;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await
        .unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn date_key_rolls_over_next_day() {
    // Review Focus #2：日期段滚动——次日 key 重起 0001
    let (pool, _, admin_url, db) = setup(
        "roll",
        r#"{"cod_rule":{"prefix":"DWG-","date_fmt":"yyyyMMdd","seq_digits":4}}"#,
    )
    .await;
    let rule = CodRule {
        prefix: "DWG-".into(),
        date_fmt: "yyyyMMdd".into(),
        seq_digits: 4,
    };
    let c1 = next_code(&pool, "drawing", &rule, "20261113")
        .await
        .unwrap();
    let c2 = next_code(&pool, "drawing", &rule, "20261113")
        .await
        .unwrap();
    assert_eq!(c1, "DWG-20261113-0001");
    assert_eq!(c2, "DWG-20261113-0002");
    let c3 = next_code(&pool, "drawing", &rule, "20261114")
        .await
        .unwrap();
    assert_eq!(c3, "DWG-20261114-0001", "次日重起 0001");
    pool.close().await;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await
        .unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn create_auto_codes_when_obj_no_empty() {
    // Review Focus #3：create 纵切——obj_no 空 → 按类型规则赋码
    let (pool, _, admin_url, db) = setup(
        "ac",
        r#"{"cod_rule":{"prefix":"DWG-","date_fmt":"yyyyMMdd","seq_digits":4}}"#,
    )
    .await;
    let registry = TypeRegistry::load(pool.clone()).await.unwrap();
    let hooks = std::sync::Arc::new(HookRegistry::new());
    let store = PgObjectStore::new(pool.clone(), registry.clone(), hooks);
    let cmd = CreateCmd {
        type_code: "drawing".into(),
        obj_no: String::new(), // 空 → 自动赋码
        attrs: serde_json::json!({"name":"N"}).as_object().unwrap().clone(),
        owner: None,
        secret_level: 0,
    };
    let o = store.create(cmd).await.unwrap();
    let today = chrono::Utc::now().format("%Y%m%d").to_string();
    assert_eq!(o.obj_no, format!("DWG-{today}-0001"), "首个自动码");
    // 第二个
    let cmd2 = CreateCmd {
        type_code: "drawing".into(),
        obj_no: String::new(),
        attrs: serde_json::json!({"name":"N2"})
            .as_object()
            .unwrap()
            .clone(),
        owner: None,
        secret_level: 0,
    };
    let o2 = store.create(cmd2).await.unwrap();
    assert_eq!(o2.obj_no, format!("DWG-{today}-0002"));
    pool.close().await;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await
        .unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn create_auto_code_without_rule_is_error() {
    let (pool, _, admin_url, db) = setup("nr", "{}").await;
    let registry = TypeRegistry::load(pool.clone()).await.unwrap();
    let hooks = std::sync::Arc::new(HookRegistry::new());
    let store = PgObjectStore::new(pool.clone(), registry.clone(), hooks);
    let cmd = CreateCmd {
        type_code: "drawing".into(),
        obj_no: String::new(),
        attrs: serde_json::json!({"name":"N"}).as_object().unwrap().clone(),
        owner: None,
        secret_level: 0,
    };
    let e = store.create(cmd).await.unwrap_err();
    assert_eq!(e.code(), "MDL-BAD-REQUEST");
    pool.close().await;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await
        .unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

use std::sync::Arc;

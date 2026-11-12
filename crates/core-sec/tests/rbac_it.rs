//! 授权域集成测试（计划 Task1）：enforce 矩阵 / 建号重复 / 授权回收即时生效 / 密级注入过滤。

use core_model::DomainError;
use core_sec::SecService;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(hint: &str) -> String {
    let hint: String = hint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(
        "rbac_{}_{}",
        hint,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

struct Ctx {
    sec: SecService,
    pool: PgPool,
    db: String,
    admin_url: String,
}

async fn setup(hint: &str) -> Ctx {
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
        .max_connections(4)
        .connect(&format!("{prefix}/{db}"))
        .await
        .unwrap();
    core_sec::migrator().run(&pool).await.unwrap();
    let sec = SecService::new(pool.clone(), "s".into());
    sec.ensure_admin(Some("pw".into())).await.unwrap();
    Ctx {
        sec,
        pool,
        db,
        admin_url,
    }
}

impl Ctx {
    async fn drop_it(self) {
        self.pool.close().await;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.admin_url)
            .await
            .unwrap();
        sqlx::query(&format!(
            "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
            self.db
        ))
        .execute(&admin)
        .await
        .unwrap();
        admin.close().await;
    }
}

#[tokio::test]
async fn enforce_matrix_three_roles() {
    let c = setup("matrix").await;
    c.sec
        .create_user("ed", "pw123456", Some("工".into()), &["editor"], 1)
        .await
        .unwrap();
    c.sec
        .create_user("vw", "pw123456", None, &["viewer"], 0)
        .await
        .unwrap();

    // admin：读写 + 用户管理
    assert!(c.sec.enforce("admin", "docs", "read").await.unwrap());
    assert!(c.sec.enforce("admin", "docs", "write").await.unwrap());
    assert!(c.sec.enforce("admin", "sec", "manage").await.unwrap());
    // editor：读写，不可管理
    assert!(c.sec.enforce("ed", "docs", "read").await.unwrap());
    assert!(c.sec.enforce("ed", "docs", "write").await.unwrap());
    assert!(!c.sec.enforce("ed", "sec", "manage").await.unwrap());
    // viewer：只读
    assert!(c.sec.enforce("vw", "docs", "read").await.unwrap());
    assert!(!c.sec.enforce("vw", "docs", "write").await.unwrap());
    // 未知用户全否
    assert!(!c.sec.enforce("ghost", "docs", "read").await.unwrap());
    c.drop_it().await;
}

#[tokio::test]
async fn duplicate_user_rejected() {
    let c = setup("dupu").await;
    c.sec
        .create_user("u1", "pw123456", None, &["viewer"], 0)
        .await
        .unwrap();
    let err = c
        .sec
        .create_user("U1", "pw123456", None, &["viewer"], 0)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "MDL-SEC-USER-EXISTS", "大小写归一查重");
    c.drop_it().await;
}

#[tokio::test]
async fn grant_revoke_flips_enforce_immediately() {
    let c = setup("flip").await;
    c.sec
        .create_user("tmp", "pw123456", None, &["viewer"], 0)
        .await
        .unwrap();
    assert!(!c.sec.enforce("tmp", "docs", "write").await.unwrap());
    c.sec.grant("tmp", "editor").await.unwrap();
    assert!(
        c.sec.enforce("tmp", "docs", "write").await.unwrap(),
        "授权后立即生效"
    );
    c.sec.revoke("tmp", "editor").await.unwrap();
    assert!(
        !c.sec.enforce("tmp", "docs", "write").await.unwrap(),
        "回收后立即失效"
    );
    c.drop_it().await;
}

#[tokio::test]
async fn unknown_role_rejected() {
    let c = setup("badrole").await;
    let err = c
        .sec
        .create_user("u", "pw123456", None, &["ninja"], 0)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "MDL-SEC-ROLE-NOT-FOUND");
    c.drop_it().await;
}

#[tokio::test]
async fn max_secret_level_stored_and_readable() {
    let c = setup("lvl").await;
    c.sec
        .create_user("low", "pw123456", None, &["viewer"], 0)
        .await
        .unwrap();
    let u_low = c.sec.user_id("low").await.unwrap();
    assert_eq!(c.sec.max_secret_level_of(u_low).await.unwrap(), 0);
    let u_admin = c.sec.user_id("admin").await.unwrap();
    assert_eq!(c.sec.max_secret_level_of(u_admin).await.unwrap(), 3);
    c.drop_it().await;
}

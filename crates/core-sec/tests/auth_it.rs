//! core-sec 集成/单元测试（计划 Task1）：真 PG18 库 + 密钥文件临时目录。

use core_sec::{Claims, SecService};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(hint: &str) -> String {
    let hint: String = hint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(
        "sec_{}_{}",
        hint,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

async fn fresh_db(hint: &str) -> (PgPool, String, String) {
    let admin_url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL 未设置");
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
    let url = format!("{prefix}/{db}");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    core_sec::migrator().run(&pool).await.unwrap();
    (pool, db, admin_url)
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
async fn login_roundtrip_and_expiry() {
    let (pool, db, admin_url) = fresh_db("rt").await;
    let sec = SecService::new(pool.clone(), "test-secret".into());
    sec.ensure_admin(Some("admin-pw-123".into())).await.unwrap();

    let pair = sec.login("admin", "admin-pw-123").await.unwrap();
    assert!(!pair.access_token.is_empty());
    assert!(!pair.refresh_token.is_empty());
    assert_eq!(pair.expires_in, 900);

    let claims = sec.parse_access(&pair.access_token).unwrap();
    assert_eq!(claims.username, "admin");
    assert_eq!(claims.typ, "access");

    // 过期 access（手工签 exp=-1）→ 401
    let expired = Claims {
        sub: claims.sub,
        username: "admin".into(),
        typ: "access".into(),
        exp: -1,
    };
    let t = sec.sign_claims(&expired).unwrap();
    assert_eq!(
        sec.parse_access(&t).unwrap_err().code(),
        "MDL-SEC-UNAUTHORIZED"
    );
    let _ = core_model::DomainError::SecUnauthorized;
    drop_db(&admin_url, &db).await;
}

#[tokio::test]
async fn refresh_token_cannot_become_access_and_rotates() {
    let (pool, db, admin_url) = fresh_db("rot").await;
    let sec = SecService::new(pool.clone(), "test-secret".into());
    sec.ensure_admin(Some("pw".into())).await.unwrap();
    let pair = sec.login("admin", "pw").await.unwrap();

    // typ=refresh 冒充 access → 401（Review Focus #1）
    assert_eq!(
        sec.parse_access(&pair.refresh_token).unwrap_err().code(),
        "MDL-SEC-UNAUTHORIZED"
    );

    // refresh 轮换：新对可用，旧 refresh 在骨架期不吊销（文档化）
    let p2 = sec.refresh(&pair.refresh_token).await.unwrap();
    assert!(sec.parse_access(&p2.access_token).is_ok());
    let _ = sec.refresh(&pair.refresh_token).await.unwrap();
    drop_db(&admin_url, &db).await;
}

#[tokio::test]
async fn ensure_admin_idempotent_keeps_password() {
    let (pool, db, admin_url) = fresh_db("admin").await;
    let sec = SecService::new(pool.clone(), "s".into());
    let first = sec.ensure_admin(Some("keep-me".into())).await.unwrap();
    assert!(first.created);
    let again = sec
        .ensure_admin(Some("should-not-apply".into()))
        .await
        .unwrap();
    assert!(!again.created);
    assert!(again.printed_password.is_none());
    // 旧密码仍可登录（未被覆盖）
    assert!(
        sec.login("ADMIN", "keep-me").await.is_ok(),
        "大小写不敏感 + 旧密码保留"
    );
    // 新密码不可登录
    assert_eq!(
        sec.login("admin", "should-not-apply")
            .await
            .unwrap_err()
            .code(),
        "MDL-SEC-BAD-CREDENTIALS"
    );
    drop_db(&admin_url, &db).await;
}

#[tokio::test]
async fn wrong_password_and_unknown_user() {
    let (pool, db, admin_url) = fresh_db("wrong").await;
    let sec = SecService::new(pool.clone(), "s".into());
    sec.ensure_admin(Some("right".into())).await.unwrap();
    assert_eq!(
        sec.login("admin", "wrong").await.unwrap_err().code(),
        "MDL-SEC-BAD-CREDENTIALS"
    );
    assert_eq!(
        sec.login("nobody", "x").await.unwrap_err().code(),
        "MDL-SEC-BAD-CREDENTIALS"
    );
    drop_db(&admin_url, &db).await;
}

#[tokio::test]
async fn jwt_key_file_persists_across_instances() {
    let dir = std::env::temp_dir().join(format!(
        "coresec-key-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    // env 缺省 → jwt.key 文件；同目录两次实例化密钥一致（Review Focus #3）
    let s1 = SecService::new_with_key_dir(None, &dir);
    let s2 = SecService::new_with_key_dir(None, &dir);
    let c = Claims {
        sub: uuid::Uuid::nil(),
        username: "u".into(),
        typ: "access".into(),
        exp: 9999999999,
    };
    let t = s1.sign_claims(&c).unwrap();
    assert!(
        s2.parse_access(&t).is_ok(),
        "跨实例密钥必须一致（jwt.key 复用）"
    );
    // 显式 env 覆盖文件
    let s3 = SecService::new_with_key_dir(Some("other-secret"), &dir);
    assert!(s3.parse_access(&t).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn random_password_printed_once() {
    let (pool, db, admin_url) = fresh_db("rand").await;
    let sec = SecService::new(pool.clone(), "s".into());
    let boot = sec.ensure_admin(None).await.unwrap();
    assert!(boot.created);
    let pw = boot.printed_password.expect("随机密码必须返回给调用方打印");
    assert!(pw.len() >= 20);
    assert!(sec.login("admin", &pw).await.is_ok());
    drop_db(&admin_url, &db).await;
}

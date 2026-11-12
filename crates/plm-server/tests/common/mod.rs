//! e2e 公共设施：与 mod-doc/tests/common 同构（库名前缀 plt_），装配完整 router。

use api_gateway::AppState;
use core_files::LocalFileStore;
use core_model::{HookRegistry, PgObjectStore, TypeRegistry};
use core_sec::SecService;
use core_version::LifecycleStore;

pub const TEST_ADMIN_PASSWORD: &str = "test-admin-pw";
use mod_doc::DocService;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TestCtx {
    pub app: axum::Router,
    pub files_root: PathBuf,
    pub pool: PgPool,
    db: String,
    admin_url: String,
}

fn unique_db(hint: &str) -> String {
    let hint: String = hint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(
        "plt_{}_{}",
        hint,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn swap_db(url: &str, db: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (url, None),
    };
    let prefix = base.rsplit_once('/').map(|(p, _)| p).unwrap_or(base);
    match query {
        Some(q) => format!("{prefix}/{db}?{q}"),
        None => format!("{prefix}/{db}"),
    }
}

/// 建唯一迁移库 → (好池, registry, files_root, db, admin_url)。两种 setup 共用。
pub async fn migrated(hint: &str) -> (PgPool, Arc<TypeRegistry>, PathBuf, String, String) {
    let admin_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        panic!("TEST_DATABASE_URL 未设置（例：postgres://postgres:root@localhost:5432/postgres）")
    });
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

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&swap_db(&admin_url, &db))
        .await
        .unwrap();
    core_model::migrator().run(&pool).await.unwrap();
    mod_doc::migrator().run(&pool).await.unwrap();
    core_sec::migrator().run(&pool).await.unwrap();
    let registry = TypeRegistry::load(pool.clone()).await.unwrap();
    let files_root = std::env::temp_dir().join(format!("plmsrv-{db}"));
    (pool, registry, files_root, db, admin_url)
}

impl TestCtx {
    /// max_upload 字节上限（oversize 用例传小值）。含 admin 引导（密码 test-admin-pw）。
    pub async fn setup(hint: &str, max_upload: u64) -> Self {
        let (pool, registry, files_root, db, admin_url) = migrated(hint).await;
        let files = Arc::new(LocalFileStore::new(&files_root));
        let hooks = Arc::new(HookRegistry::new());
        let objects = Arc::new(PgObjectStore::new(pool.clone(), registry.clone(), hooks));
        let service = Arc::new(DocService::new(
            pool.clone(),
            registry.clone(),
            objects,
            files,
        ));
        let sec = Arc::new(SecService::new(pool.clone(), "test-jwt-secret".to_string()));
        sec.ensure_admin(Some(TEST_ADMIN_PASSWORD.into()))
            .await
            .unwrap();
        let lifecycle = Arc::new(LifecycleStore::new(pool.clone()));
        let app = api_gateway::router(AppState {
            registry,
            service,
            pool: pool.clone(),
            max_upload,
            sec,
            lifecycle,
        });

        Self {
            app,
            files_root,
            db,
            admin_url,
            pool,
        }
    }

    /// admin 建号（密级+角色）并返回 (username, password)
    pub async fn create_user_as_admin(
        &self,
        admin_token: &str,
        username: &str,
        password: &str,
        roles: &[&str],
        max_secret_level: i16,
    ) {
        let body = serde_json::json!({
            "username": username, "password": password,
            "roles": roles, "max_secret_level": max_secret_level,
        });
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/sec/users")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {admin_token}"))
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        let (status, b, _) = send(&self.app, req).await;
        assert_eq!(status, 201, "建号必须成功: {}", String::from_utf8_lossy(&b));
    }

    /// 以任意账号登录
    pub async fn login_as(&self, username: &str, password: &str) -> String {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(format!(
                r#"{{"username":"{username}","password":"{password}"}}"#
            )))
            .unwrap();
        let (status, body, _) = send(&self.app, req).await;
        assert_eq!(status, 200);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        v["access_token"].as_str().unwrap().to_string()
    }

    /// 以 admin 登录并返回 access token（e2e 全链用）
    pub async fn login(&self) -> String {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(format!(
                r#"{{"username":"admin","password":"{TEST_ADMIN_PASSWORD}"}}"#
            )))
            .unwrap();
        let (status, body, _) = send(&self.app, req).await;
        assert_eq!(
            status,
            200,
            "测试登录必须成功: {}",
            String::from_utf8_lossy(&body)
        );
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        v["access_token"].as_str().unwrap().to_string()
    }

    /// 带 Bearer 的 GET
    pub async fn get_authed(&self, uri: &str, token: &str) -> (u16, bytes::Bytes) {
        let req = axum::http::Request::builder()
            .method("GET")
            .uri(uri)
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let (s, b, _) = send(&self.app, req).await;
        (s, b)
    }

    pub async fn cleanup(self) {
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
        let _ = std::fs::remove_dir_all(&self.files_root);
    }
}

/// 手工构造 multipart body：字段 = (name, filename?, bytes)
pub fn multipart_body(
    boundary: &str,
    fields: &[(&str, Option<&str>, Vec<u8>)],
) -> (axum::http::HeaderValue, Vec<u8>) {
    let mut body = Vec::new();
    for (name, filename, value) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(fname) => body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{fname}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .as_bytes(),
            ),
            None => body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes()),
        }
        body.extend_from_slice(value);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let ct =
        axum::http::HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
            .unwrap();
    (ct, body)
}

pub async fn send(
    app: &axum::Router,
    req: axum::http::Request<axum::body::Body>,
) -> (u16, bytes::Bytes, Option<axum::http::HeaderValue>) {
    use tower::ServiceExt;
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let trace = res.headers().get("x-trace-id").cloned();
    let body = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    (status, body, trace)
}

/// 终审 I1 专用：真迁移库出 registry（缓存热，Arc 共享）+ 坏池做业务/ping。
pub fn degraded_app(
    registry: Arc<TypeRegistry>,
    bad_pool: PgPool,
    max_upload: u64,
) -> axum::Router {
    let files = Arc::new(LocalFileStore::new(
        std::env::temp_dir().join("plmsrv-degraded"),
    ));
    let hooks = Arc::new(HookRegistry::new());
    let objects = Arc::new(PgObjectStore::new(
        bad_pool.clone(),
        registry.clone(),
        hooks,
    ));
    let service = Arc::new(DocService::new(
        bad_pool.clone(),
        registry.clone(),
        objects,
        files,
    ));
    let sec = Arc::new(SecService::new(bad_pool.clone(), "degraded-secret".into()));
    let lifecycle = Arc::new(LifecycleStore::new(bad_pool.clone()));
    api_gateway::router(AppState {
        registry,
        service,
        pool: bad_pool,
        max_upload,
        sec,
        lifecycle,
    })
}

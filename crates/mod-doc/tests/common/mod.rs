//! mod-doc 集成测试公共设施：TEST_DATABASE_URL + 每测试唯一库（plan 全局约束）。

use core_files::LocalFileStore;
use core_model::{HookRegistry, PgObjectStore, TypeRegistry};
use mod_doc::DocService;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TestCtx {
    pub service: Arc<DocService>,
    #[allow(dead_code)]
    pub files: Arc<LocalFileStore>,
    pub pool: PgPool,
    files_root: PathBuf,
    db: String,
    admin_url: String,
}

fn unique_db(hint: &str) -> String {
    let hint: String = hint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(
        "mdt_{}_{}",
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

impl TestCtx {
    pub async fn setup(hint: &str) -> Self {
        let admin_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            panic!(
                "TEST_DATABASE_URL 未设置（例：postgres://postgres:root@localhost:5432/postgres）"
            )
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

        let url = swap_db(&admin_url, &db);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .unwrap();

        core_model::migrator().run(&pool).await.unwrap();
        mod_doc::migrator().run(&pool).await.unwrap();

        let registry = TypeRegistry::load(pool.clone()).await.unwrap();
        let files_root = std::env::temp_dir().join(format!("moddoc-{db}"));
        let files = Arc::new(LocalFileStore::new(&files_root));
        let hooks = Arc::new(HookRegistry::new());
        let objects = Arc::new(PgObjectStore::new(pool.clone(), registry.clone(), hooks));
        let service = Arc::new(DocService::new(
            pool.clone(),
            registry,
            objects,
            files.clone(),
        ));

        Self {
            service,
            files,
            pool,
            files_root,
            db,
            admin_url,
        }
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

    /// 卷内实际落盘的文件数（Review Focus #1：孤儿文件钉子用）
    pub async fn vol_file_count(&self) -> usize {
        let mut n = 0;
        let vol = self.files_root.join("vol");
        let mut rd = match tokio::fs::read_dir(&vol).await {
            Ok(rd) => rd,
            Err(_) => return 0,
        };
        while let Ok(Some(bucket)) = rd.next_entry().await {
            if let Ok(mut inner) = tokio::fs::read_dir(bucket.path()).await {
                while let Ok(Some(_)) = inner.next_entry().await {
                    n += 1;
                }
            }
        }
        n
    }
}

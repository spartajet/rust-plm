//! core-version 生命周期集成测试（计划 Task1）：真 PG18 库 + core-model 全链。

use core_model::{HookRegistry, PgObjectStore, TypeRegistry};
use core_version::{LifecycleEvent, LifecycleStore};
use mod_doc::DocService;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::io::Cursor;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(hint: &str) -> String {
    let hint: String = hint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(
        "lc_{}_{}",
        hint,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

struct Ctx {
    svc: Arc<DocService>,
    store: LifecycleStore,
    pool: PgPool,
    db: String,
    admin_url: String,
    files_root: PathBuf,
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
    core_model::migrator().run(&pool).await.unwrap();
    mod_doc::migrator().run(&pool).await.unwrap();
    core_version::migrator().run(&pool).await.unwrap();
    let registry = TypeRegistry::load(pool.clone()).await.unwrap();
    let files_root = std::env::temp_dir().join(format!("lcver-{db}"));
    let files = Arc::new(core_files::LocalFileStore::new(&files_root));
    let hooks = Arc::new(HookRegistry::new());
    let objects = Arc::new(PgObjectStore::new(pool.clone(), registry.clone(), hooks));
    let svc = Arc::new(DocService::new(
        pool.clone(),
        registry.clone(),
        objects,
        files,
    ));
    let store = LifecycleStore::new(pool.clone());
    Ctx {
        svc,
        store,
        pool,
        db,
        admin_url,
        files_root,
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
        let _ = std::fs::remove_dir_all(&self.files_root);
    }

    async fn new_doc(&self, obj_no: &str) {
        let mut r = Cursor::new(b"LC-CONTENT".to_vec());
        self.svc
            .upload(
                obj_no.into(),
                serde_json::json!({"name": "N"})
                    .as_object()
                    .unwrap()
                    .clone(),
                "f.dwg".into(),
                None,
                &mut r,
            )
            .await
            .unwrap();
    }
}

use std::path::PathBuf;

#[tokio::test]
async fn happy_path_to_released_then_obsolete() {
    let c = setup("happy").await;
    c.new_doc("DWG-LC-1").await;
    let o = c
        .store
        .transition("drawing", "DWG-LC-1", LifecycleEvent::Submit, None)
        .await
        .unwrap();
    assert_eq!(o.state, "review");
    let o = c
        .store
        .transition("drawing", "DWG-LC-1", LifecycleEvent::Approve, None)
        .await
        .unwrap();
    assert_eq!(o.state, "released");
    let o = c
        .store
        .transition("drawing", "DWG-LC-1", LifecycleEvent::Obsolete, None)
        .await
        .unwrap();
    assert_eq!(o.state, "obsolete");
    c.drop_it().await;
}

#[tokio::test]
async fn illegal_transitions_409() {
    let c = setup("ill").await;
    c.new_doc("DWG-LC-2").await;
    // working→released 跳过审批
    let e = c
        .store
        .transition("drawing", "DWG-LC-2", LifecycleEvent::Release, None)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-LCS-TRANSITION");
    // released→submit
    c.store
        .transition("drawing", "DWG-LC-2", LifecycleEvent::Submit, None)
        .await
        .unwrap();
    c.store
        .transition("drawing", "DWG-LC-2", LifecycleEvent::Approve, None)
        .await
        .unwrap();
    let e = c
        .store
        .transition("drawing", "DWG-LC-2", LifecycleEvent::Submit, None)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-LCS-TRANSITION");
    c.drop_it().await;
}

#[tokio::test]
async fn reject_returns_to_working_and_editable() {
    let c = setup("rej").await;
    c.new_doc("DWG-LC-3").await;
    c.store
        .transition("drawing", "DWG-LC-3", LifecycleEvent::Submit, None)
        .await
        .unwrap();
    let o = c
        .store
        .transition("drawing", "DWG-LC-3", LifecycleEvent::Reject, None)
        .await
        .unwrap();
    assert_eq!(o.state, "working");
    // 回 working 后可再改（Review Focus #3）
    let updated = c
        .svc
        .update_object(
            "DWG-LC-3",
            &serde_json::json!({"name": "N2"})
                .as_object()
                .unwrap()
                .clone(),
            o.attrs_rev,
            uuid::Uuid::nil(),
        )
        .await
        .unwrap();
    assert_eq!(updated.attrs.get("name").unwrap(), "N2");
    // 再提交可达 review
    let o = c
        .store
        .transition("drawing", "DWG-LC-3", LifecycleEvent::Submit, None)
        .await
        .unwrap();
    assert_eq!(o.state, "review");
    c.drop_it().await;
}

#[tokio::test]
async fn release_snapshot_immutable_and_outbox_row() {
    let c = setup("snap").await;
    c.new_doc("DWG-LC-4").await;
    c.store
        .transition("drawing", "DWG-LC-4", LifecycleEvent::Submit, None)
        .await
        .unwrap();
    c.store
        .transition("drawing", "DWG-LC-4", LifecycleEvent::Approve, None)
        .await
        .unwrap();

    // 快照存在且内容==发布时点
    let snap: (serde_json::Value, String) = sqlx::query_as(
        "SELECT attrs, state FROM cm_obj_snapshot WHERE obj_no='DWG-LC-4' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&c.pool)
    .await
    .expect("发布必须落快照");
    assert_eq!(snap.1, "released");
    assert_eq!(snap.0.get("name").and_then(|v| v.as_str()), Some("N"));

    // 活对象后续变更不影响快照（Review Focus #4）——经 Revise 语义手动降版改活对象不可行（released 423），
    // 这里直接 SQL 证明快照行与活行解耦：改活行 attrs 不动快照
    sqlx::query("UPDATE cm_object SET attrs = attrs || '{\"name\":\"CHANGED\"}'::jsonb WHERE obj_no='DWG-LC-4'")
        .execute(&c.pool)
        .await
        .unwrap();
    let snap2: (serde_json::Value,) = sqlx::query_as(
        "SELECT attrs FROM cm_obj_snapshot WHERE obj_no='DWG-LC-4' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&c.pool)
    .await
    .unwrap();
    assert_eq!(snap2.0.get("name").and_then(|v| v.as_str()), Some("N"));

    // doc.released 事件行存在（增量4 外投的落库前提）
    let ev: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM event_outbox WHERE topic='doc.released' AND payload->>'obj_no'='DWG-LC-4'",
    )
    .fetch_one(&c.pool)
    .await
    .unwrap();
    assert_eq!(ev, 1, "发布必须落 doc.released 事件");
    c.drop_it().await;
}

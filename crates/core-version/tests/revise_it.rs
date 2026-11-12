//! core-version Revise 升版集成测试（增量8 Task1）：真 PG18 库 + core-model 全链。
//! Review Focus #1/#2/#3：升版事务/快照不变式/并发对拍。

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
        "rv_{}_{}",
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
    registry: Arc<TypeRegistry>,
    pool: PgPool,
    db: String,
    admin_url: String,
    files_root: std::path::PathBuf,
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
    let files_root = std::env::temp_dir().join(format!("rvver-{db}"));
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
        registry,
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
        let mut r = Cursor::new(b"RV-CONTENT".to_vec());
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

    /// 到 released（带发布件哈希——快照 file_hash 钉）
    async fn release_with_hash(&self, obj_no: &str, hash: &str) {
        self.store
            .transition("drawing", obj_no, LifecycleEvent::Submit, None)
            .await
            .unwrap();
        self.store
            .transition_with_file("drawing", obj_no, LifecycleEvent::Approve, None, Some(hash))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn revise_creates_next_working_major() {
    // Review Focus #1：major=2/minor=0/working；v1.0 快照仍在；doc.revised 事件；快照含 file_hash
    let c = setup("rev1").await;
    c.new_doc("DWG-RV-1").await;
    c.release_with_hash("DWG-RV-1", "hash-v1-content").await;

    let o = c
        .store
        .transition("drawing", "DWG-RV-1", LifecycleEvent::Revise, None)
        .await
        .unwrap();
    assert_eq!(o.state, "working");
    assert_eq!(o.major, 2, "升版必须 major+1");
    assert_eq!(o.minor, 0);

    // v1.0 released 快照仍在（revise 不写新快照）
    let snaps: Vec<(i32, i32, String)> = sqlx::query_as(
        "SELECT major_ver, minor_ver, state FROM cm_obj_snapshot WHERE obj_no='DWG-RV-1' ORDER BY id",
    )
    .fetch_all(&c.pool)
    .await
    .unwrap();
    assert_eq!(snaps.len(), 1, "revise 不新增快照");
    assert_eq!(
        (snaps[0].0, snaps[0].1, snaps[0].2.as_str()),
        (1, 0, "released")
    );
    // 发布件哈希随 approve 落快照（数据先存——旧版文件追溯出口在后续增量）
    let fh: Option<String> =
        sqlx::query_scalar("SELECT file_hash FROM cm_obj_snapshot WHERE obj_no='DWG-RV-1'")
            .fetch_one(&c.pool)
            .await
            .unwrap();
    assert_eq!(fh.as_deref(), Some("hash-v1-content"));

    // doc.revised 事件
    let ev_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM event_outbox WHERE topic='doc.revised' AND payload->>'obj_no'='DWG-RV-1'",
    )
    .fetch_one(&c.pool)
    .await
    .unwrap();
    assert_eq!(ev_count, 1, "revise 必须落 doc.revised 事件");
    let ev: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM event_outbox WHERE topic='doc.revised' AND payload->>'obj_no'='DWG-RV-1'",
    )
    .fetch_one(&c.pool)
    .await
    .unwrap();
    assert_eq!(ev.get("old_major").and_then(|v| v.as_i64()), Some(1));
    assert_eq!(ev.get("new_major").and_then(|v| v.as_i64()), Some(2));
    c.drop_it().await;
}

#[tokio::test]
async fn revise_on_working_is_409() {
    let c = setup("rev2").await;
    c.new_doc("DWG-RV-2").await;
    let e = c
        .store
        .transition("drawing", "DWG-RV-2", LifecycleEvent::Revise, None)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-LCS-TRANSITION");
    assert_eq!(e.http_status(), 409);
    c.drop_it().await;
}

#[tokio::test]
async fn revise_on_obsolete_is_409() {
    // anti-goal：作废件不得经 revise 复活
    let c = setup("rev3").await;
    c.new_doc("DWG-RV-3").await;
    c.store
        .transition("drawing", "DWG-RV-3", LifecycleEvent::Submit, None)
        .await
        .unwrap();
    c.store
        .transition("drawing", "DWG-RV-3", LifecycleEvent::Approve, None)
        .await
        .unwrap();
    c.store
        .transition("drawing", "DWG-RV-3", LifecycleEvent::Obsolete, None)
        .await
        .unwrap();
    let e = c
        .store
        .transition("drawing", "DWG-RV-3", LifecycleEvent::Revise, None)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-LCS-TRANSITION");
    c.drop_it().await;
}

#[tokio::test]
async fn concurrent_revise_exactly_one_winner() {
    // Review Focus #2：并发双 revise 对拍——一胜一 409，不得静默双升
    let c = setup("rev4").await;
    c.new_doc("DWG-RV-4").await;
    c.release_with_hash("DWG-RV-4", "h").await;

    let (r1, r2) = tokio::join!(
        c.store
            .transition("drawing", "DWG-RV-4", LifecycleEvent::Revise, None),
        c.store
            .transition("drawing", "DWG-RV-4", LifecycleEvent::Revise, None),
    );
    let ok = [&r1, &r2].iter().filter(|r| r.is_ok()).count();
    assert_eq!(ok, 1, "并发 revise 必须恰好一个成功");
    let loser = if r1.is_err() { &r1 } else { &r2 };
    assert_eq!(loser.as_ref().unwrap_err().code(), "MDL-LCS-TRANSITION");

    let major: i32 = sqlx::query_scalar("SELECT major_ver FROM cm_object WHERE obj_no='DWG-RV-4'")
        .fetch_one(&c.pool)
        .await
        .unwrap();
    assert_eq!(major, 2, "败者不得把 major 推到 3");
    c.drop_it().await;
}

#[tokio::test]
async fn revise_then_rerelease_versions_chain() {
    // v1 发布→升版→v2 再发布：快照两行、doc.released 两事件、版本链完整
    let c = setup("rev5").await;
    c.new_doc("DWG-RV-5").await;
    c.release_with_hash("DWG-RV-5", "h1").await;
    c.store
        .transition("drawing", "DWG-RV-5", LifecycleEvent::Revise, None)
        .await
        .unwrap();
    // working 2.0 期间改属性（就地编辑草稿）
    let d = c.svc.detail("DWG-RV-5").await.unwrap();
    assert_eq!(
        (d.object.major, d.object.minor),
        (2, 0),
        "M3'：detail 取最新版"
    );
    c.svc
        .update_object(
            "DWG-RV-5",
            &serde_json::json!({"name": "N2"})
                .as_object()
                .unwrap()
                .clone(),
            d.object.attrs_rev,
            uuid::Uuid::nil(),
        )
        .await
        .unwrap();
    c.release_with_hash("DWG-RV-5", "h2").await;

    let o = c.svc.detail("DWG-RV-5").await.unwrap();
    assert_eq!(o.object.state, "released");
    assert_eq!((o.object.major, o.object.minor), (2, 0));

    let snaps: Vec<(i32, i32, String, Option<String>)> = sqlx::query_as(
        "SELECT major_ver, minor_ver, state, file_hash FROM cm_obj_snapshot \
         WHERE obj_no='DWG-RV-5' ORDER BY id",
    )
    .fetch_all(&c.pool)
    .await
    .unwrap();
    assert_eq!(snaps.len(), 2, "两个 major 各一份 released 快照");
    assert_eq!((snaps[0].0, snaps[0].3.as_deref()), (1, Some("h1")));
    assert_eq!(
        (snaps[1].0, snaps[1].1, snaps[1].3.as_deref()),
        (2, 0, Some("h2"))
    );
    // v1 快照不被 v2 变更污染
    let v1_attrs: serde_json::Value = sqlx::query_scalar(
        "SELECT attrs FROM cm_obj_snapshot WHERE obj_no='DWG-RV-5' AND major_ver=1",
    )
    .fetch_one(&c.pool)
    .await
    .unwrap();
    assert_eq!(v1_attrs.get("name").and_then(|v| v.as_str()), Some("N"));

    let rel_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM event_outbox WHERE topic='doc.released' AND payload->>'obj_no'='DWG-RV-5'",
    )
    .fetch_one(&c.pool)
    .await
    .unwrap();
    assert_eq!(rel_events, 2);
    c.drop_it().await;
}

// ==================== 终审修复轮（C1 回归钉 / I1 / I2） ====================

#[tokio::test]
async fn revise_then_same_objno_create_is_409() {
    // 终审 C1：就地升版打破"同号唯一行必为(1,0)"——create 预检不得按 (1,0) 查，
    // 否则升版后同号可重复建档产生双活行、编号永久卡死。
    let c = setup("revc1").await;
    c.new_doc("DWG-RV-C1").await;
    c.release_with_hash("DWG-RV-C1", "h").await;
    c.store
        .transition("drawing", "DWG-RV-C1", LifecycleEvent::Revise, None)
        .await
        .unwrap();

    let e = {
        let mut r = Cursor::new(b"X".to_vec());
        c.svc
            .upload(
                "DWG-RV-C1".into(),
                serde_json::json!({"name": "N"})
                    .as_object()
                    .unwrap()
                    .clone(),
                "f.dwg".into(),
                None,
                &mut r,
            )
            .await
            .unwrap_err()
    };
    assert_eq!(e.code(), "MDL-OBJ-NO-DUPLICATE", "同号重建必须 409");

    // 双活行不得出现
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM cm_object WHERE obj_no='DWG-RV-C1'")
        .fetch_one(&c.pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "同号必须恒一活行");
    c.drop_it().await;
}

#[tokio::test]
async fn open_original_follows_latest_after_revise() {
    // 终审 I1：Review Focus #3 的第三枚钉——升版后文件读取跟随 2.0
    let c = setup("revo1").await;
    c.new_doc("DWG-RV-O1").await;
    c.release_with_hash("DWG-RV-O1", "h").await;
    c.store
        .transition("drawing", "DWG-RV-O1", LifecycleEvent::Revise, None)
        .await
        .unwrap();
    let (name, mut stream, size) = c.svc.open_original("DWG-RV-O1").await.unwrap();
    assert_eq!(name, "f.dwg");
    assert!(size > 0);
    use tokio::io::AsyncReadExt;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    assert_eq!(buf, b"RV-CONTENT");
    c.drop_it().await;
}

#[tokio::test]
async fn revise_reentry_minimal_direct_release() {
    // 终审 I2：minimal 档升版重入——revise→submit 一键直发 v2.0
    let c = setup("revm1").await;
    sqlx::query("UPDATE cm_type SET config = config || jsonb_build_object('flow_tier','minimal') WHERE code='drawing'")
        .execute(&c.pool)
        .await
        .unwrap();
    c.registry.reload_if_changed().await.unwrap();

    c.new_doc("DWG-RV-M1").await;
    let o = c
        .store
        .transition("drawing", "DWG-RV-M1", LifecycleEvent::Submit, None)
        .await
        .unwrap();
    assert_eq!(o.state, "released", "minimal submit 直发");
    let o = c
        .store
        .transition("drawing", "DWG-RV-M1", LifecycleEvent::Revise, None)
        .await
        .unwrap();
    assert_eq!((o.major, o.state.as_str()), (2, "working"));
    let o = c
        .store
        .transition("drawing", "DWG-RV-M1", LifecycleEvent::Submit, None)
        .await
        .unwrap();
    assert_eq!(o.state, "released", "重入后仍一键直发");
    assert_eq!(o.major, 2);

    let snaps: i64 =
        sqlx::query_scalar("SELECT count(*) FROM cm_obj_snapshot WHERE obj_no='DWG-RV-M1'")
            .fetch_one(&c.pool)
            .await
            .unwrap();
    assert_eq!(snaps, 2, "两个 major 各一快照");
    c.drop_it().await;
}

#[tokio::test]
async fn revise_reentry_strict_two_level_chain() {
    // 终审 I2：strict 档升版重入——v2.0 再走两级审批+会签发布
    let c = setup("revs1").await;
    sqlx::query("UPDATE cm_type SET config = config || jsonb_build_object('flow_tier','strict') WHERE code='drawing'")
        .execute(&c.pool)
        .await
        .unwrap();
    c.registry.reload_if_changed().await.unwrap();

    let drafter = uuid::Uuid::new_v4();
    let checker = uuid::Uuid::new_v4();
    c.new_doc("DWG-RV-S1").await;
    c.store
        .transition(
            "drawing",
            "DWG-RV-S1",
            LifecycleEvent::Submit,
            Some(drafter),
        )
        .await
        .unwrap();
    c.store
        .transition(
            "drawing",
            "DWG-RV-S1",
            LifecycleEvent::Approve,
            Some(checker),
        )
        .await
        .unwrap();
    c.store
        .transition(
            "drawing",
            "DWG-RV-S1",
            LifecycleEvent::Approve,
            Some(drafter),
        )
        .await
        .unwrap();
    let o = c
        .store
        .transition(
            "drawing",
            "DWG-RV-S1",
            LifecycleEvent::Revise,
            Some(drafter),
        )
        .await
        .unwrap();
    assert_eq!((o.major, o.state.as_str()), (2, "working"));

    // v2 重走两级：submit→review→approve→checked→approve→released（会签仍生效）
    c.store
        .transition(
            "drawing",
            "DWG-RV-S1",
            LifecycleEvent::Submit,
            Some(drafter),
        )
        .await
        .unwrap();
    let o = c
        .store
        .transition(
            "drawing",
            "DWG-RV-S1",
            LifecycleEvent::Approve,
            Some(checker),
        )
        .await
        .unwrap();
    assert_eq!(o.state, "checked");
    // 会签在 v2 重入时仍生效：第二级与第一级复核人同人 → 409
    let e = c
        .store
        .transition(
            "drawing",
            "DWG-RV-S1",
            LifecycleEvent::Approve,
            Some(checker),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-LCS-MAKER-CHECKER", "会签在 v2 重入时仍生效");
    let o = c
        .store
        .transition(
            "drawing",
            "DWG-RV-S1",
            LifecycleEvent::Approve,
            Some(drafter),
        )
        .await
        .unwrap();
    assert_eq!(o.state, "released");
    assert_eq!(o.major, 2);
    c.drop_it().await;
}

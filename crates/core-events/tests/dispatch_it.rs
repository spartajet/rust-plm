//! core-events 集成测试（计划 Task1）：in-test 接收端 + 签名验证 + 退避死信 + topic 过滤。

use axum::extract::State as AxState;
use axum::routing::post;
use axum::Router;
use core_events::{EventDispatcher, WebhookRepo};
use core_version::LifecycleStore;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::io::Cursor;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(hint: &str) -> String {
    let hint: String = hint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(
        "ev_{}_{}",
        hint,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

struct Ctx {
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
    core_model::migrator().run(&pool).await.unwrap();
    mod_doc::migrator().run(&pool).await.unwrap();
    core_version::migrator().run(&pool).await.unwrap();
    core_events::migrator().run(&pool).await.unwrap();
    Ctx {
        pool,
        db,
        admin_url,
    }
}

impl Ctx {
    /// lifecycle release（产生 doc.released outbox 行）
    async fn release_doc(&self, obj_no: &str) {
        let registry = core_model::TypeRegistry::load(self.pool.clone())
            .await
            .unwrap();
        let hooks = Arc::new(core_model::HookRegistry::new());
        let objects = Arc::new(core_model::PgObjectStore::new(
            self.pool.clone(),
            registry,
            hooks,
        ));
        let files = Arc::new(core_files::LocalFileStore::new(
            std::env::temp_dir().join(format!("corev-{db}", db = self.db)),
        ));
        let svc = Arc::new(DocService::new(
            self.pool.clone(),
            core_model::TypeRegistry::load(self.pool.clone())
                .await
                .unwrap(),
            objects,
            files,
        ));
        let mut r = Cursor::new(b"EV".to_vec());
        svc.upload(
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
        let store = LifecycleStore::new(self.pool.clone());
        store
            .transition(
                "drawing",
                obj_no,
                core_version::LifecycleEvent::Submit,
                None,
            )
            .await
            .unwrap();
        store
            .transition(
                "drawing",
                obj_no,
                core_version::LifecycleEvent::Approve,
                None,
            )
            .await
            .unwrap();
    }

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

use mod_doc::DocService;

/// in-test 接收端：记录 (signature, body)，可配置失败 N 次
#[derive(Clone, Default)]
struct Hits {
    inner: Arc<std::sync::Mutex<Vec<String>>>,
    counter: Arc<std::sync::atomic::AtomicUsize>,
    /// 前 N 次返回 500（死信用）；签名用例传 0
    fail_first: Arc<std::sync::atomic::AtomicUsize>,
}

async fn spawn_receiver(fail_first: usize) -> (SocketAddr, Hits) {
    let hits = Hits {
        inner: Default::default(),
        counter: Default::default(),
        fail_first: Arc::new(std::sync::atomic::AtomicUsize::new(fail_first)),
    };
    let st = hits.clone();
    let app = Router::new().route(
        "/hook",
        post(
            move |AxState(st): AxState<Hits>,
                  headers: axum::http::HeaderMap,
                  body: String| async move {
                use std::sync::atomic::Ordering;
                let n = st.counter.fetch_add(1, Ordering::SeqCst);
                if n < st.fail_first.load(Ordering::SeqCst) {
                    return axum::http::StatusCode::INTERNAL_SERVER_ERROR;
                }
                let sig = headers
                    .get("x-rustplm-signature")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                st.inner.lock().unwrap().push(format!("{sig}|{body}"));
                axum::http::StatusCode::OK
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = app.with_state(st.clone()); // 共享同一 Hits（否则 push 进空实例）
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (addr, hits)
}

#[tokio::test]
async fn webhook_delivered_with_valid_signature() {
    let c = setup("ok").await;
    c.release_doc("DWG-EV-1").await;

    let (addr, hits) = spawn_receiver(0).await;
    let repo = WebhookRepo::new(c.pool.clone());
    repo.create(
        &format!("http://{addr}/hook"),
        &["doc.released".into()],
        "sec-key",
    )
    .await
    .unwrap();

    let d = EventDispatcher::new(c.pool.clone()).with_backoff(vec![1]);
    let delivered = d.tick().await.unwrap();
    assert_eq!(delivered, 1);

    // 状态 CAS 到 dispatched
    let st: i16 = sqlx::query_scalar("SELECT status FROM event_outbox WHERE topic='doc.released'")
        .fetch_one(&c.pool)
        .await
        .unwrap();
    assert_eq!(st, 1);

    // Review Focus #5：事件体=签名体；#1：签名可验证（同 secret HMAC）
    let recorded = {
        let guard = hits.inner.lock().unwrap();
        guard.clone()
    };
    assert_eq!(recorded.len(), 1);
    let entry = &recorded[0];
    let (sig, body) = entry.split_once('|').unwrap();
    use hmac::Mac;
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(b"sec-key").unwrap();
    mac.update(body.as_bytes());
    let expect = hex::encode(mac.finalize().into_bytes());
    assert_eq!(sig, expect, "HMAC 签名必须可验证");
    assert!(body.contains("DWG-EV-1"));
    c.drop_it().await;
}

#[tokio::test]
async fn retry_then_dead_letter() {
    let c = setup("dead").await;
    c.release_doc("DWG-EV-2").await;
    let (addr, _hits) = spawn_receiver(usize::MAX).await; // 恒 500
    let repo = WebhookRepo::new(c.pool.clone());
    repo.create(
        &format!("http://{addr}/hook"),
        &["doc.released".into()],
        "k",
    )
    .await
    .unwrap();

    let d = EventDispatcher::new(c.pool.clone()).with_backoff(vec![0, 0, 0]);
    // 首发 + 3 次重试（MAX_ATTEMPTS=4）
    for _ in 0..4 {
        d.tick().await.unwrap();
    }
    let (st, attempts): (i16, i32) =
        sqlx::query_as("SELECT status, attempts FROM event_outbox WHERE topic='doc.released'")
            .fetch_one(&c.pool)
            .await
            .unwrap();
    assert_eq!(st, 2, "4 次尝试全败后必须死信");
    assert_eq!(attempts, 4);
    // 死信不再投
    d.tick().await.unwrap();
    let attempts2: i32 =
        sqlx::query_scalar("SELECT attempts FROM event_outbox WHERE topic='doc.released'")
            .fetch_one(&c.pool)
            .await
            .unwrap();
    assert_eq!(attempts2, 4, "死信行不再被 tick 处理");
    c.drop_it().await;
}

#[tokio::test]
async fn topic_filter_only_matching_subs() {
    let c = setup("filter").await;
    c.release_doc("DWG-EV-3").await;
    // 订阅另一个 topic：不应收到 doc.released
    let (addr, hits) = spawn_receiver(0).await;
    let repo = WebhookRepo::new(c.pool.clone());
    repo.create(&format!("http://{addr}/hook"), &["other.topic".into()], "k")
        .await
        .unwrap();

    let d = EventDispatcher::new(c.pool.clone()).with_backoff(vec![1]);
    d.tick().await.unwrap();
    // 事件被标 dispatched（无命中订阅者=消费掉）
    let st: i16 = sqlx::query_scalar("SELECT status FROM event_outbox WHERE topic='doc.released'")
        .fetch_one(&c.pool)
        .await
        .unwrap();
    assert_eq!(st, 1);
    assert!(
        hits.inner.lock().unwrap().is_empty(),
        "topic 不匹配不得投递"
    );
    c.drop_it().await;
}

#[tokio::test]
async fn c2_backoff_future_events_skipped_by_tick() {
    // 终审 C2：退避中的事件（next_retry_at 在未来）必须被 tick 跳过
    let c = setup("c2b").await;
    c.release_doc("DWG-C2-1").await;
    // 手动置为"刚失败一次，退避到 1 小时后"
    sqlx::query("UPDATE event_outbox SET attempts = 1, next_retry_at = now() + interval '1 hour'")
        .execute(&c.pool)
        .await
        .unwrap();

    let d = EventDispatcher::new(c.pool.clone()).with_backoff(vec![60, 300, 1800]);
    let delivered = d.tick().await.unwrap();
    assert_eq!(delivered, 0, "未到期事件不得被 tick 处理");
    let (attempts, st): (i32, i16) =
        sqlx::query_as("SELECT attempts, status FROM event_outbox WHERE topic='doc.released'")
            .fetch_one(&c.pool)
            .await
            .unwrap();
    assert_eq!(attempts, 1, "退避期 attempts 不变");
    assert_eq!(st, 0, "退避期保持 pending");
    c.drop_it().await;
}

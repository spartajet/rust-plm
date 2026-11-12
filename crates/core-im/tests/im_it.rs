//! core-im 集成测试（增量10 Task2）：真 PG18 库 + 本地 axum 假 IM。
//! Review Focus #2/#5：推送送达/退避死信/双消费者解耦。

use axum::routing::post;
use core_im::{sender_for, CardPusher};
use core_model::{HookRegistry, PgObjectStore, TypeRegistry};
use core_version::{LifecycleEvent, LifecycleStore};
use mod_doc::DocService;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(hint: &str) -> String {
    let hint: String = hint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(
        "im_{}_{}",
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
    core_im::migrator().run(&pool).await.unwrap();
    let registry = TypeRegistry::load(pool.clone()).await.unwrap();
    let files = Arc::new(core_files::LocalFileStore::new(
        std::env::temp_dir().join(format!("imver-{db}")),
    ));
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

    async fn submit(&self, obj_no: &str) {
        let mut r = Cursor::new(b"IM-CONTENT".to_vec());
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
        self.store
            .transition("drawing", obj_no, LifecycleEvent::Submit, None)
            .await
            .unwrap();
    }
}

/// 本地假 IM：捕获请求体（markdown text）
async fn fake_im() -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
    let captured: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
    let cap = captured.clone();
    let app = axum::Router::new().route(
        "/robot/send",
        post(move |axum::Json(v): axum::Json<serde_json::Value>| {
            let cap = cap.clone();
            async move {
                cap.lock().unwrap().push(v);
                axum::Json(serde_json::json!({"errcode": 0}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/robot/send"), captured)
}

#[tokio::test]
async fn pusher_delivers_card_and_marks_sent() {
    // Review Focus #2：submit → pusher tick → 假 IM 收到含 obj_no 的卡 → im_card_log sent
    let c = setup("push1").await;
    c.submit("DWG-PUSH-1").await;

    let (webhook, captured) = fake_im().await;
    let sender = sender_for("dingtalk", &webhook, "test-secret", reqwest::Client::new());
    let pusher = Arc::new(CardPusher::new(
        c.pool.clone(),
        sender,
        "http://plm/docs/{obj_no}".into(),
    ));
    pusher.tick().await.unwrap();

    {
        let got = captured.lock().unwrap();
        assert_eq!(got.len(), 1, "假 IM 须收到 1 张卡");
        let text = got[0]["markdown"]["text"].as_str().unwrap();
        assert!(text.contains("DWG-PUSH-1"), "text={text}");
        assert!(
            text.contains("http://plm/docs/DWG-PUSH-1"),
            "链接占位须替换"
        );
    }

    let row: (i16, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT status, sent_at FROM im_card_log WHERE obj_no='DWG-PUSH-1'")
            .fetch_one(&c.pool)
            .await
            .unwrap();
    assert_eq!(row.0, 1, "须标 sent");
    assert!(row.1.is_some());

    // 幂等：再 tick 不重发（event_id UNIQUE 已订阅）
    pusher.tick().await.unwrap();
    assert_eq!(captured.lock().unwrap().len(), 1, "同事件不得重发");

    // dispatcher 先行认领（status=1）后 pusher 仍消费新事件（Review Focus #5）
    let dispatcher = core_events::EventDispatcher::new(c.pool.clone());
    c.submit("DWG-PUSH-2").await;
    dispatcher.tick().await.unwrap(); // outbox 行被 webhook 消费者置位
    let claimed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM event_outbox WHERE status<>0 AND payload->>'obj_no'='DWG-PUSH-2'",
    )
    .fetch_one(&c.pool)
    .await
    .unwrap();
    assert_eq!(claimed, 1, "前置：dispatcher 已认领该行");
    drop(captured.lock().unwrap());
    pusher.tick().await.unwrap();
    {
        let got2 = captured.lock().unwrap();
        assert!(
            got2.iter().any(|v| v["markdown"]["text"]
                .as_str()
                .unwrap()
                .contains("DWG-PUSH-2")),
            "dispatcher 认领后 IM pusher 仍须独立消费"
        );
    }
    c.drop_it().await;
}

#[tokio::test]
async fn pusher_retries_then_dead_letters() {
    // Review Focus #2 后半：发不出去 → attempts 退避 → 死信 status=2
    let c = setup("push2").await;
    c.submit("DWG-PUSH-3").await;

    // 指向无监听端口（必败）
    let sender = sender_for(
        "dingtalk",
        "http://127.0.0.1:1/robot/send",
        "s",
        reqwest::Client::new(),
    );
    let pusher = Arc::new(CardPusher::new(c.pool.clone(), sender, String::new()));

    for round in 1..=3 {
        pusher.tick().await.unwrap();
        let (attempts, status): (i32, i16) =
            sqlx::query_as("SELECT attempts, status FROM im_card_log WHERE obj_no='DWG-PUSH-3'")
                .fetch_one(&c.pool)
                .await
                .unwrap();
        assert_eq!(attempts, round, "第 {round} 轮失败后 attempts={round}");
        assert_eq!(status, 0, "未达上限仍 pending");

        // 退避未到 → 再 tick 不重复计数（next_retry_at 挡住）
        pusher.tick().await.unwrap();
        let attempts2: i32 =
            sqlx::query_scalar("SELECT attempts FROM im_card_log WHERE obj_no='DWG-PUSH-3'")
                .fetch_one(&c.pool)
                .await
                .unwrap();
        assert_eq!(attempts2, round, "退避窗口内不得重试");
        // 快进退避窗口
        sqlx::query("UPDATE im_card_log SET next_retry_at = now() - interval '1s' WHERE obj_no='DWG-PUSH-3'")
            .execute(&c.pool)
            .await
            .unwrap();
    }
    // 第 4 次失败 → 死信
    pusher.tick().await.unwrap();
    let (status, error): (i16, Option<String>) =
        sqlx::query_as("SELECT status, error FROM im_card_log WHERE obj_no='DWG-PUSH-3'")
            .fetch_one(&c.pool)
            .await
            .unwrap();
    assert_eq!(status, 2, "MAX_ATTEMPTS 后必须死信");
    assert!(error.is_some());
    c.drop_it().await;
}

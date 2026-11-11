//! e2e（计划 Task4）：oneshot 打真 router + 真 PG18 库。覆盖 spec DoD#2 全链与 Review Focus #2/#3/#5。

mod common;

use common::{multipart_body, send, TestCtx};
use sqlx::PgPool;
use std::io::Cursor;

const FILE: &[u8] = b"%DWG-e2e-content-9876543210";

fn get(uri: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .body(axum::body::Body::empty())
        .unwrap()
}

#[tokio::test]
async fn healthz_ok() {
    let ctx = TestCtx::setup("health", 1024 * 1024).await;
    let (status, body, _) = send(&ctx.app, get("/healthz")).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["status"], "ok");
    ctx.cleanup().await;
}

#[tokio::test]
async fn full_flow_upload_list_detail_download() {
    let ctx = TestCtx::setup("flow", 1024 * 1024).await;
    let boundary = "XyZ123";

    // POST：中文名文件（Review Focus #2）
    let (ct, body) = multipart_body(
        boundary,
        &[
            ("obj_no", None, b"DWG-E2E-1".to_vec()),
            (
                "attrs",
                None,
                br#"{"name":"Bracket","material":"Q235"}"#.to_vec(),
            ),
            ("file", Some("零件.dwg"), FILE.to_vec()),
        ],
    );
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/docs")
        .header("content-type", ct)
        .header("content-length", body.len())
        .body(axum::body::Body::from(body))
        .unwrap();
    let (status, body, trace) = send(&ctx.app, req).await;
    assert_eq!(status, 201, "body={}", String::from_utf8_lossy(&body));
    assert!(trace.is_some(), "必须有 X-Trace-Id");
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["object"]["obj_no"], "DWG-E2E-1");
    assert_eq!(v["file"]["name"], "零件.dwg");
    let hash = v["file"]["hash"].as_str().unwrap().to_string();
    assert_eq!(hash.len(), 64);

    // LIST：GIN 路径（material 非提升列）
    let (status, body, _) = send(&ctx.app, get("/api/v1/docs?filter[material]=Q235")).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["obj_no"], "DWG-E2E-1");

    // DETAIL
    let (status, body, _) = send(&ctx.app, get("/api/v1/docs/DWG-E2E-1")).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["object"]["attrs"]["name"], "Bracket");

    // DOWNLOAD：字节相等 + Content-Disposition 原名（UTF-8 编码）
    let (status, body, headers_trace) = send(&ctx.app, get("/api/v1/docs/DWG-E2E-1/file")).await;
    assert_eq!(status, 200);
    assert_eq!(&body[..], FILE);
    let _ = headers_trace;
    let _ = Cursor::new(&body[..]);

    // 下载响应头在 send 里被丢弃，补一次带头的请求验证 disposition
    let req = get("/api/v1/docs/DWG-E2E-1/file");
    use tower::ServiceExt;
    let res = ctx.app.clone().oneshot(req).await.unwrap();
    let cd = res
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cd.contains("filename*"), "必须含 RFC5987 filename*：{cd}");
    let _ = hash;

    ctx.cleanup().await;
}

#[tokio::test]
async fn duplicate_conflict_with_trace() {
    let ctx = TestCtx::setup("dup", 1024 * 1024).await;
    let boundary = "XyZ123";
    let mk = || {
        let (ct, body) = multipart_body(
            boundary,
            &[
                ("obj_no", None, b"DWG-DUP-9".to_vec()),
                ("attrs", None, br#"{"name":"N"}"#.to_vec()),
                ("file", Some("a.dwg"), FILE.to_vec()),
            ],
        );
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/docs")
            .header("content-type", ct)
            .header("content-length", body.len())
            .body(axum::body::Body::from(body))
            .unwrap()
    };
    let (s1, _, _) = send(&ctx.app, mk()).await;
    assert_eq!(s1, 201);
    let (s2, body, trace) = send(&ctx.app, mk()).await;
    assert_eq!(s2, 409);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-OBJ-NO-DUPLICATE");
    let t = trace.unwrap();
    assert!(!t.is_empty(), "错误体与响应头必须携带 trace_id");
    ctx.cleanup().await;
}

#[tokio::test]
async fn oversize_rejected_no_201() {
    let ctx = TestCtx::setup("big", 1024).await; // 限额 1KB
    let big = vec![b'x'; 4096];
    let (ct, body) = multipart_body(
        "XyZ123",
        &[
            ("obj_no", None, b"DWG-BIG-1".to_vec()),
            ("attrs", None, br#"{"name":"N"}"#.to_vec()),
            ("file", Some("big.dwg"), big),
        ],
    );
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/docs")
        .header("content-type", ct)
        .header("content-length", body.len())
        .body(axum::body::Body::from(body))
        .unwrap();
    let (status, _, _) = send(&ctx.app, req).await;
    assert!(
        status == 400 || status == 413,
        "超限必须 400/413（Review Focus #5），得到 {status}"
    );
    ctx.cleanup().await;
}

#[tokio::test]
async fn healthz_degraded_when_db_unreachable() {
    // 终审 I1：DB ping 失败必须 503/degraded，而非恒 200。
    // registry 从真迁移库加载（缓存热），业务/ping 池换成坏库——模拟"DB 宕机但进程活着"
    use common::migrated;
    let (_good_pool, registry, _root, db, admin_url) = migrated("degraded").await;
    let bad: PgPool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://postgres:root@localhost:5432/no_such_db_e2e")
        .expect("lazy pool 创建不应失败");
    let app = common::degraded_app(registry, bad, 1024 * 1024);
    let (status, body, _) = send(&app, get("/healthz")).await;
    assert_eq!(status, 503, "DB 不可达时 healthz 必须 503");
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["status"], "degraded");

    let admin = PgPool::connect(&admin_url).await.unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

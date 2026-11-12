//! e2e（计划 Task4）：oneshot 打真 router + 真 PG18 库。
//! 覆盖 spec DoD#2 全链、骨架 Review Focus #2/#3/#5、鉴权钉子（本增量 Review Focus #1/#5）。

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

fn get_authed(uri: &str, token: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(axum::body::Body::empty())
        .unwrap()
}

fn post_docs_multipart(
    token: Option<&str>,
    obj_no: &str,
    attrs: &str,
    filename: Option<&str>,
    bytes: Vec<u8>,
) -> axum::http::Request<axum::body::Body> {
    let (ct, body) = multipart_body(
        "XyZ123",
        &[
            ("obj_no", None, obj_no.as_bytes().to_vec()),
            ("attrs", None, attrs.as_bytes().to_vec()),
            ("file", filename, bytes),
        ],
    );
    let mut b = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/docs")
        .header("content-type", ct)
        .header("content-length", body.len());
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    b.body(axum::body::Body::from(body)).unwrap()
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
async fn docs_require_bearer() {
    // 鉴权钉子（Review Focus #1/#5）：无 token / 非 Bearer / refresh 冒充 access → 401
    let ctx = TestCtx::setup("authz", 1024 * 1024).await;

    // 无 token
    let (status, body, _) = send(&ctx.app, get("/api/v1/docs")).await;
    assert_eq!(status, 401);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-SEC-UNAUTHORIZED");

    // 非 Bearer 前缀
    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/api/v1/docs")
        .header("authorization", "abc123")
        .body(axum::body::Body::empty())
        .unwrap();
    let (status, _, _) = send(&ctx.app, req).await;
    assert_eq!(status, 401);

    // login 拿两个 token
    let login_req = |pw: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(format!(
                r#"{{"username":"admin","password":"{pw}"}}"#
            )))
            .unwrap()
    };
    let (status, body, _) = send(&ctx.app, login_req("wrong-pw")).await;
    assert_eq!(status, 401);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-SEC-BAD-CREDENTIALS");

    let (status, body, _) = send(&ctx.app, login_req(common::TEST_ADMIN_PASSWORD)).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let access = v["access_token"].as_str().unwrap().to_string();
    let refresh = v["refresh_token"].as_str().unwrap().to_string();

    // refresh 冒充 access → 401
    let (status, _, _) = send(&ctx.app, get_authed("/api/v1/docs", &refresh)).await;
    assert_eq!(status, 401);

    // 正确 access → 200；refresh 接口轮换出新 access
    let (status, _, _) = send(&ctx.app, get_authed("/api/v1/docs", &access)).await;
    assert_eq!(status, 200);
    let rr = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/auth/refresh")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(format!(
            r#"{{"refresh_token":"{refresh}"}}"#
        )))
        .unwrap();
    let (status, body, _) = send(&ctx.app, rr).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(!v["access_token"].as_str().unwrap().is_empty());

    ctx.cleanup().await;
}

#[tokio::test]
async fn full_flow_upload_list_detail_download() {
    let ctx = TestCtx::setup("flow", 1024 * 1024).await;
    let token = ctx.login().await;

    // POST：中文名文件（Review Focus #2）
    let (status, body, trace) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&token),
            "DWG-E2E-1",
            r#"{"name":"Bracket","material":"Q235"}"#,
            Some("零件.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(status, 201, "body={}", String::from_utf8_lossy(&body));
    assert!(trace.is_some(), "必须有 X-Trace-Id");
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["object"]["obj_no"], "DWG-E2E-1");
    assert_eq!(v["file"]["name"], "零件.dwg");
    assert_eq!(v["file"]["hash"].as_str().unwrap().len(), 64);

    // LIST：GIN 路径（material 非提升列）
    let (status, body) = ctx
        .get_authed("/api/v1/docs?filter[material]=Q235", &token)
        .await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["obj_no"], "DWG-E2E-1");

    // DETAIL
    let (status, body) = ctx.get_authed("/api/v1/docs/DWG-E2E-1", &token).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["object"]["attrs"]["name"], "Bracket");

    // DOWNLOAD：字节相等 + Content-Disposition 原名（UTF-8 编码）
    let (status, body) = ctx.get_authed("/api/v1/docs/DWG-E2E-1/file", &token).await;
    assert_eq!(status, 200);
    assert_eq!(&body[..], FILE);
    let _ = Cursor::new(&body[..]);

    use tower::ServiceExt;
    let res = ctx
        .app
        .clone()
        .oneshot(get_authed("/api/v1/docs/DWG-E2E-1/file", &token))
        .await
        .unwrap();
    let cd = res
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cd.contains("filename*"), "必须含 RFC5987 filename*：{cd}");

    ctx.cleanup().await;
}

#[tokio::test]
async fn duplicate_conflict_with_trace() {
    let ctx = TestCtx::setup("dup", 1024 * 1024).await;
    let token = ctx.login().await;
    let (s1, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&token),
            "DWG-DUP-9",
            r#"{"name":"N"}"#,
            Some("a.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s1, 201);
    let (s2, body, trace) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&token),
            "DWG-DUP-9",
            r#"{"name":"N"}"#,
            Some("a.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
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
    let token = ctx.login().await;
    let big = vec![b'x'; 4096];
    let (status, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&token),
            "DWG-BIG-1",
            r#"{"name":"N"}"#,
            Some("big.dwg"),
            big,
        ),
    )
    .await;
    assert!(
        status == 400 || status == 413,
        "超限必须 400/413（Review Focus #5），得到 {status}"
    );
    ctx.cleanup().await;
}

#[tokio::test]
async fn healthz_degraded_when_db_unreachable() {
    // 终审 I1：DB ping 失败必须 503/degraded，而非恒 200。
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

// ==================== 授权增量钉子（计划 Review Focus #1–#4） ====================

#[tokio::test]
async fn rbac_viewer_cannot_write_403() {
    let ctx = TestCtx::setup("rbac1", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "vw", "vw-pass-123", &["viewer"], 0)
        .await;
    let vw = ctx.login_as("vw", "vw-pass-123").await;

    // viewer 读 → 200；写 → 403（已认证但无权限，而非 401）
    let (s, _) = ctx.get_authed("/api/v1/docs", &vw).await;
    assert_eq!(s, 200);
    let (s, body, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&vw),
            "DWG-VW-1",
            r#"{"name":"N"}"#,
            Some("v.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 403, "viewer 写入必须 403");
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-SEC-FORBIDDEN");

    // Review Focus #4：非 admin 调用户管理 → 403
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/sec/users")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {vw}"))
        .body(axum::body::Body::from(
            r#"{"username":"x","password":"12345678"}"#.to_string(),
        ))
        .unwrap();
    let (s, body, _) = send(&ctx.app, req).await;
    assert_eq!(s, 403);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-SEC-FORBIDDEN");
    ctx.cleanup().await;
}

#[tokio::test]
async fn rbac_revoke_role_immediate_403() {
    let ctx = TestCtx::setup("rbac2", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "ed", "ed-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("ed", "ed-pass-123").await;

    // 授权期内可写
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-ED-1",
            r#"{"name":"N"}"#,
            Some("e.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);

    // 回收 editor 角色 → 下一次请求立即 403（无重启）
    let req = axum::http::Request::builder()
        .method("DELETE")
        .uri("/api/v1/sec/users/ed/roles/editor")
        .header("authorization", format!("Bearer {admin}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let (s, _, _) = send(&ctx.app, req).await;
    assert_eq!(s, 204);

    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-ED-2",
            r#"{"name":"N"}"#,
            Some("e.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 403, "回收后必须立即 403");
    ctx.cleanup().await;
}

#[tokio::test]
async fn rbac_secret_level_filter_and_404() {
    let ctx = TestCtx::setup("rbac3", 1024 * 1024).await;
    let admin = ctx.login().await;
    // viewer max_secret_level=0；secret 系统列默认 0——用 SQL 提升该对象密级到 2（秘密）
    ctx.create_user_as_admin(&admin, "low", "low-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("low", "low-pass-123").await;
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-SEC-9",
            r#"{"name":"N"}"#,
            Some("s.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    sqlx::query("UPDATE cm_object SET secret_level = 2 WHERE obj_no = 'DWG-SEC-9'")
        .execute(&ctx.pool)
        .await
        .unwrap();

    // 密级 2 > viewer/editor 上限 0：list 不出现；detail 404（防存在性泄露）
    let (s, body) = ctx.get_authed("/api/v1/docs", &ed).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 0, "越密级对象不得出现在列表");

    let (s, _) = ctx.get_authed("/api/v1/docs/DWG-SEC-9", &ed).await;
    assert_eq!(s, 404, "越密级 detail 必须 404（不泄露存在性）");

    // admin（上限 3）可见
    let (s, body) = ctx.get_authed("/api/v1/docs", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 1);
    ctx.cleanup().await;
}

// ==================== 生命周期链钉子（增量3 Review Focus #1–#5） ====================

#[tokio::test]
async fn lifecycle_full_chain_and_permissions() {
    let ctx = TestCtx::setup("lc", 1024 * 1024).await;
    let admin = ctx.login().await;
    // editor 提交；approve 是 admin 专属（Review Focus #5）
    ctx.create_user_as_admin(&admin, "ed", "ed-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("ed", "ed-pass-123").await;

    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-LC-E1",
            r#"{"name":"N"}"#,
            Some("e.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);

    // editor submit → 200 review（write 权限）
    let sub = |t: &str, no: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/submit"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, _, _) = send(&ctx.app, sub(&ed, "DWG-LC-E1")).await;
    assert_eq!(s, 200);

    // editor approve → 403（docs:approve 仅 admin）
    let app = |t: &str, no: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/approve"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, body, _) = send(&ctx.app, app(&ed, "DWG-LC-E1")).await;
    assert_eq!(s, 403);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-SEC-FORBIDDEN");

    // admin approve → released
    let (s, body, _) = send(&ctx.app, app(&admin, "DWG-LC-E1")).await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["state"], "released");

    // Review Focus #1：released update → 423
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-LC-E1",
            r#"{"name":"N"}"#,
            Some("e.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    // 同号同版本重复上传 → 409（唯一预检真实命中）；released 属性修改 423 由 lifecycle_it::released_update_forbidden_423 钉住（终审 C1）
    assert_eq!(s, 409, "发布态同号重复上传必须 409");

    // 非法链：released submit → 409（Review Focus #2 同源）
    let (s, body, _) = send(&ctx.app, sub(&ed, "DWG-LC-E1")).await;
    assert_eq!(s, 409);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-LCS-TRANSITION");

    // obsolete 收尾
    let obs = |t: &str, no: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/obsolete"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, _, _) = send(&ctx.app, obs(&admin, "DWG-LC-E1")).await;
    assert_eq!(s, 200);
    ctx.cleanup().await;
}

#[tokio::test]
async fn lifecycle_reject_back_to_working_editable() {
    // Review Focus #3：reject 回 working 后可再改再提交
    let ctx = TestCtx::setup("lcr", 1024 * 1024).await;
    let admin = ctx.login().await;
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&admin),
            "DWG-LC-R1",
            r#"{"name":"V1"}"#,
            Some("r.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    let t0 = ctx.login().await;
    let sub = |no: &str, t: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/submit"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, _, _) = send(&ctx.app, sub("DWG-LC-R1", &t0)).await;
    assert_eq!(s, 200);
    let rej = |no: &str, t: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/reject"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let t = ctx.login().await;
    let (s, _, _) = send(&ctx.app, rej("DWG-LC-R1", &t)).await;
    assert_eq!(s, 200);

    // 回 working 后可再次提交
    let (s, body, _) = send(&ctx.app, sub("DWG-LC-R1", &t0)).await;
    assert_eq!(s, 200, "reject 后可再提交");
    let _ = body;
    ctx.cleanup().await;
}

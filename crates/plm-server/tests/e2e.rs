//! e2e（计划 Task4）：oneshot 打真 router + 真 PG18 库。
//! 覆盖 spec DoD#2 全链、骨架 Review Focus #2/#3/#5、鉴权钉子（本增量 Review Focus #1/#5）。

mod common;

use common::{multipart_body, send, TestCtx};
use sqlx::{PgPool, Row};
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
    let (_good_pool, registry, watcher, _root, db, admin_url) = migrated("degraded").await;
    let bad: PgPool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://postgres:root@localhost:5432/no_such_db_e2e")
        .expect("lazy pool 创建不应失败");
    let app = common::degraded_app(registry, bad, 1024 * 1024);
    let (status, body, _) = send(&app, get("/healthz")).await;
    assert_eq!(status, 503, "DB 不可达时 healthz 必须 503");
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["status"], "degraded");
    watcher.abort(); // 终审 M7：不留活跃任务阻塞进程退出

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

#[tokio::test]
async fn webhooks_editor_manage_403() {
    // 增量4 I5：非 admin 管理 webhook → 403（sec:manage 门控与 sec/users 同层，已验；此钉补 webhooks 面板）
    let ctx = TestCtx::setup("wh", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "vw", "vw-pass-123", &["viewer"], 0)
        .await;
    let vw = ctx.login_as("vw", "vw-pass-123").await;

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/sec/webhooks")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {vw}"))
        .body(axum::body::Body::from(
            r#"{"url":"http://127.0.0.1:1/h","topics":["doc.released"],"secret":"k"}"#.to_string(),
        ))
        .unwrap();
    let (s, body, _) = send(&ctx.app, req).await;
    assert_eq!(s, 403);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-SEC-FORBIDDEN");
    ctx.cleanup().await;
}

// ==================== 三档流程钉子（增量5 Review Focus） ====================

#[tokio::test]
async fn flow_tier_minimal_and_switch_and_permission() {
    let ctx = TestCtx::setup("tier", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "ed", "ed-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("ed", "ed-pass-123").await;

    // Review Focus #5：editor 改档位 → 403
    let req = axum::http::Request::builder()
        .method("PUT")
        .uri("/api/v1/meta/types/drawing/flow-tier")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {ed}"))
        .body(axum::body::Body::from(r#"{"tier":"minimal"}"#.to_string()))
        .unwrap();
    let (s, _, _) = send(&ctx.app, req).await;
    assert_eq!(s, 403, "档位修改须 docs:approve（admin）");

    // admin 切 minimal → 热生效
    let req = axum::http::Request::builder()
        .method("PUT")
        .uri("/api/v1/meta/types/drawing/flow-tier")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {admin}"))
        .body(axum::body::Body::from(r#"{"tier":"minimal"}"#.to_string()))
        .unwrap();
    let (s, _, _) = send(&ctx.app, req).await;
    assert_eq!(s, 204);
    // 等 TypeRegistry NOTIFY 热刷新（watcher NOTIFY 即时 / 兜底 1s；1.5s 覆盖两者）
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    // GET 档位确认
    let (s, body) = ctx.get_authed("/api/v1/meta/types/drawing", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["flow_tier"], "minimal");

    // Review Focus #1：submit 直达 released（editor 有 write 权限）
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-TIER-1",
            r#"{"name":"N"}"#,
            Some("t.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    let sub = |t: &str, no: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/submit"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, body, _) = send(&ctx.app, sub(&ed, "DWG-TIER-1")).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["state"], "released", "极简档提交即发布");

    // Review Focus #4：跨档 released 语义一致——obsolete 可用、423 保护
    let obs = |t: &str, no: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/obsolete"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, _, _) = send(&ctx.app, obs(&admin, "DWG-TIER-1")).await;
    assert_eq!(s, 200);

    ctx.cleanup().await;
}

#[tokio::test]
async fn flow_tier_strict_maker_checker_via_api() {
    let ctx = TestCtx::setup("tierstrict", 1024 * 1024).await;
    let admin = ctx.login().await;
    // strict 档两级审批需要两个有审批权的人：总工 chief（admin 角色）+ admin
    ctx.create_user_as_admin(&admin, "chief", "chief-pass-1", &["admin"], 0)
        .await;
    ctx.create_user_as_admin(&admin, "ed", "ed-pass-123", &["editor"], 0)
        .await;
    let chief = ctx.login_as("chief", "chief-pass-1").await;
    let ed = ctx.login_as("ed", "ed-pass-123").await;

    // admin 切 strict
    let req = axum::http::Request::builder()
        .method("PUT")
        .uri("/api/v1/meta/types/drawing/flow-tier")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {admin}"))
        .body(axum::body::Body::from(r#"{"tier":"strict"}"#.to_string()))
        .unwrap();
    let (s, _, _) = send(&ctx.app, req).await;
    assert_eq!(s, 204);
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await; // watcher 兜底 1s + 余量

    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-STRICT-1",
            r#"{"name":"N"}"#,
            Some("s.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);

    let post_lc = |t: &str, no: &str, action: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/{action}"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    // ed submit → review（write 权限）
    let (s, _, _) = send(&ctx.app, post_lc(&ed, "DWG-STRICT-1", "submit")).await;
    assert_eq!(s, 200);
    // editor 尝试 approve → 403（approve 权限语义钉）
    let (s, _, _) = send(&ctx.app, post_lc(&ed, "DWG-STRICT-1", "approve")).await;
    assert_eq!(s, 403);
    // chief 第一级 approve → checked
    let (s, _, _) = send(&ctx.app, post_lc(&chief, "DWG-STRICT-1", "approve")).await;
    assert_eq!(s, 200);
    // chief 同人第二级 → 409 MAKER-CHECKER（Review Focus #2）
    let (s, body, _) = send(&ctx.app, post_lc(&chief, "DWG-STRICT-1", "approve")).await;
    assert_eq!(s, 409);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-LCS-MAKER-CHECKER");
    // admin 异人第二级 → released
    let (s, body, _) = send(&ctx.app, post_lc(&admin, "DWG-STRICT-1", "approve")).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["state"], "released");
    ctx.cleanup().await;
}

// ==================== BOM 钉子（增量6 Review Focus #4/#5） ====================

#[tokio::test]
async fn bom_expand_whereused_and_permissions() {
    let ctx = TestCtx::setup("bom", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "vw", "vw-pass-123", &["viewer"], 0)
        .await;
    let vw = ctx.login_as("vw", "vw-pass-123").await;

    for (no, name) in [("DWG-BOM-R", "R"), ("DWG-BOM-M", "M"), ("DWG-BOM-L", "L")] {
        let (s, _, _) = send(
            &ctx.app,
            post_docs_multipart(
                Some(&admin),
                no,
                &format!(r#"{{"name":"{name}"}}"#),
                Some("f.dwg"),
                FILE.to_vec(),
            ),
        )
        .await;
        assert_eq!(s, 201);
    }
    let get_id = |app: axum::Router, t: String, no: &'static str| {
        Box::pin(async move {
            let req = axum::http::Request::builder()
                .method("GET")
                .uri(format!("/api/v1/docs/{no}"))
                .header("authorization", format!("Bearer {t}"))
                .body(axum::body::Body::empty())
                .unwrap();
            let (s, body, _) = send(&app, req).await;
            assert_eq!(s, 200);
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
            v["object"]["id"].as_str().unwrap().to_string()
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = String>>>
    };
    let mid_id = get_id(ctx.app.clone(), admin.clone(), "DWG-BOM-M").await;
    let leaf_id = get_id(ctx.app.clone(), admin.clone(), "DWG-BOM-L").await;
    let root_id = get_id(ctx.app.clone(), admin.clone(), "DWG-BOM-R").await;

    let post_lines = |t: &str, no: &str, body: String| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/boms/{no}/lines"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {t}"))
            .header("content-length", body.len())
            .body(axum::body::Body::from(body))
            .unwrap()
    };
    // root BOM：1 × mid (qty 2)（Review Focus #5：viewer 写 → 403）
    let (s, _, _) = send(
        &ctx.app,
        post_lines(
            &vw,
            "DWG-BOM-R",
            format!(r#"[{{"line_no":1,"child":"{mid_id}","qty":2}}]"#),
        ),
    )
    .await;
    assert_eq!(s, 403, "viewer 写 BOM 必须 403");

    // admin 写 mid BOM：1 × leaf (qty 3)
    let (s, _, _) = send(
        &ctx.app,
        post_lines(
            &admin,
            "DWG-BOM-M",
            format!(r#"[{{"line_no":1,"child":"{leaf_id}","qty":3}}]"#),
        ),
    )
    .await;
    assert_eq!(s, 204);
    // admin 写 root BOM：1 × mid (qty 2)
    let (s, _, _) = send(
        &ctx.app,
        post_lines(
            &admin,
            "DWG-BOM-R",
            format!(r#"[{{"line_no":1,"child":"{mid_id}","qty":2}}]"#),
        ),
    )
    .await;
    assert_eq!(s, 204);

    // 展开 full：leaf qty_total = 2*3 = 6
    let (s, body) = ctx
        .get_authed("/api/v1/boms/DWG-BOM-R/expand", &admin)
        .await;
    assert_eq!(s, 200);
    let rows: Vec<serde_json::Value> = serde_json::Deserializer::from_slice(&body)
        .into_iter::<serde_json::Value>()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(rows.len(), 2, "full 展开应有 mid+leaf 两行");
    let leaf_row = rows
        .iter()
        .find(|r| r["child"] == serde_json::json!(leaf_id))
        .unwrap();
    assert_eq!(leaf_row["qty_total"], 6, "leaf 数量 = 2*3");

    // leaf 模式：只有 leaf
    let (s, body) = ctx
        .get_authed("/api/v1/boms/DWG-BOM-R/expand?mode=leaf", &admin)
        .await;
    assert_eq!(s, 200);
    let rows: Vec<serde_json::Value> = serde_json::Deserializer::from_slice(&body)
        .into_iter::<serde_json::Value>()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["child"], serde_json::json!(leaf_id));

    // where-used：leaf 反查 root (qty 6) 与 mid (qty 3)
    let (s, body) = ctx
        .get_authed("/api/v1/boms/DWG-BOM-L/where-used", &admin)
        .await;
    assert_eq!(s, 200);
    let rows: Vec<serde_json::Value> = serde_json::Deserializer::from_slice(&body)
        .into_iter::<serde_json::Value>()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(rows.len(), 2);

    // 循环引用 → 409：mid 的 BOM 指回 root
    let (s, _, _) = send(
        &ctx.app,
        post_lines(
            &admin,
            "DWG-BOM-M",
            format!(r#"[{{"line_no":1,"child":"{root_id}","qty":1}}]"#),
        ),
    )
    .await;
    assert_eq!(s, 409, "循环引用必须 409");

    ctx.cleanup().await;
}

// ==================== Revise 升版（增量8 Review Focus #1/#5） ====================

#[tokio::test]
async fn revise_and_versions_via_api() {
    let ctx = TestCtx::setup("rv", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "rved", "rved-pass-123", &["editor"], 0)
        .await;
    ctx.create_user_as_admin(&admin, "rvview", "rvview-pass-123", &["viewer"], 0)
        .await;
    let ed = ctx.login_as("rved", "rved-pass-123").await;
    let view = ctx.login_as("rvview", "rvview-pass-123").await;

    let lc = |t: &str, no: &str, act: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/{act}"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };

    // ed 上传 → submit → admin approve → released v1.0
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-RV-E1",
            r#"{"name":"Rev件"}"#,
            Some("r.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    let (s, _, _) = send(&ctx.app, lc(&ed, "DWG-RV-E1", "submit")).await;
    assert_eq!(s, 200);
    let (s, _, _) = send(&ctx.app, lc(&admin, "DWG-RV-E1", "approve")).await;
    assert_eq!(s, 200);

    // versions：当前 v1.0 released + 1 份快照
    let (s, body) = ctx.get_authed("/api/v1/docs/DWG-RV-E1/versions", &ed).await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["current"]["major"], 1);
    assert_eq!(v["current"]["state"], "released");
    assert_eq!(v["snapshots"].as_array().unwrap().len(), 1);
    assert_eq!(v["snapshots"][0]["major"], 1);
    assert!(
        v["snapshots"][0]["file_hash"].is_string(),
        "快照须带发布件哈希"
    );

    // viewer 只读：versions 200，revise 403（Review Focus #5）
    let (s, _) = ctx
        .get_authed("/api/v1/docs/DWG-RV-E1/versions", &view)
        .await;
    assert_eq!(s, 200);
    let (s, body, _) = send(&ctx.app, lc(&view, "DWG-RV-E1", "revise")).await;
    assert_eq!(s, 403);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-SEC-FORBIDDEN");

    // ed revise → working v2.0
    let (s, body, _) = send(&ctx.app, lc(&ed, "DWG-RV-E1", "revise")).await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["state"], "working");
    assert_eq!(v["major"], 2);
    assert_eq!(v["minor"], 0);

    // 再 revise → 409
    let (s, body, _) = send(&ctx.app, lc(&ed, "DWG-RV-E1", "revise")).await;
    assert_eq!(s, 409);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-LCS-TRANSITION");

    // M3' 端到端钉：升版后 detail 跟随 2.0
    let (s, body) = ctx.get_authed("/api/v1/docs/DWG-RV-E1", &ed).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["object"]["major"], 2, "detail 必须取最新版");
    assert_eq!(v["object"]["minor"], 0);

    // versions：current 2.0 working + v1.0 快照链（revise 不写新快照）
    let (s, body) = ctx.get_authed("/api/v1/docs/DWG-RV-E1/versions", &ed).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["current"]["major"], 2);
    assert_eq!(v["current"]["state"], "working");
    let snaps = v["snapshots"].as_array().unwrap();
    assert_eq!(snaps.len(), 1, "revise 不新增快照");
    assert_eq!(snaps[0]["state"], "released");

    // ABAC 越级 404（不泄露存在性）：密级抬到 2，ed 上限 0
    sqlx::query("UPDATE cm_object SET secret_level = 2 WHERE obj_no = 'DWG-RV-E1'")
        .execute(&ctx.pool)
        .await
        .unwrap();
    let (s, _) = ctx.get_authed("/api/v1/docs/DWG-RV-E1/versions", &ed).await;
    assert_eq!(s, 404, "越密级 versions 必须 404");
    let (s, _, _) = send(&ctx.app, lc(&ed, "DWG-RV-E1", "revise")).await;
    assert_eq!(s, 404, "越密级 revise 必须 404");
    ctx.cleanup().await;
}

// ==================== BOM 版本链（增量9 Review Focus #3/#4） ====================

#[tokio::test]
async fn bom_version_chain_freeze_isolate_asof() {
    let ctx = TestCtx::setup("bvc", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "bved", "bved-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("bved", "bved-pass-123").await;

    // 三个对象：父 + 两子
    for no in ["DWG-BV-P", "DWG-BV-C1", "DWG-BV-C2"] {
        let (s, _, _) = send(
            &ctx.app,
            post_docs_multipart(
                Some(&ed),
                no,
                r#"{"name":"N"}"#,
                Some("b.dwg"),
                FILE.to_vec(),
            ),
        )
        .await;
        assert_eq!(s, 201, "{no}");
    }
    // 查父/子对象 id
    let mut ids: std::collections::HashMap<&str, String> = Default::default();
    for no in ["DWG-BV-P", "DWG-BV-C1", "DWG-BV-C2"] {
        let (s, body) = ctx.get_authed(&format!("/api/v1/docs/{no}"), &ed).await;
        assert_eq!(s, 200);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        ids.insert(no, v["object"]["id"].as_str().unwrap().to_string());
    }
    let (pid, c1, c2) = (
        ids["DWG-BV-P"].clone(),
        ids["DWG-BV-C1"].clone(),
        ids["DWG-BV-C2"].clone(),
    );

    let set_lines = |t: &str, no: &str, body: String| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/boms/{no}/lines"))
            .header("authorization", format!("Bearer {t}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap()
    };
    let expand = |t: &str, no: &str, q: &str| {
        axum::http::Request::builder()
            .method("GET")
            .uri(format!("/api/v1/boms/{no}/expand{q}"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let parse_nd = |b: &[u8]| -> Vec<(String, i64)> {
        String::from_utf8_lossy(b)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).unwrap();
                (
                    v["child"].as_str().unwrap().to_string(),
                    v["qty_total"].as_i64().unwrap(),
                )
            })
            .collect()
    };

    // v1 working：父 BOM = C1×2 + C2×1
    let (s, body, _) = send(
        &ctx.app,
        set_lines(
            &ed,
            "DWG-BV-P",
            format!(
                r#"[{{"line_no":1,"child":"{c1}","qty":2}},{{"line_no":2,"child":"{c2}","qty":1}}]"#
            ),
        ),
    )
    .await;
    assert_eq!(s, 204, "body={}", String::from_utf8_lossy(&body));

    // as-of 未发布 → 400
    let (s, body, _) = send(&ctx.app, expand(&ed, "DWG-BV-P", "?mode=full&major=1")).await;
    assert_eq!(s, 400, "未发布 major 必须 400");
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-BAD-REQUEST");

    // approve v1 → 冻结（hook 同事务快照）
    let lc = |t: &str, no: &str, act: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/{act}"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, _, _) = send(&ctx.app, lc(&ed, "DWG-BV-P", "submit")).await;
    assert_eq!(s, 200);
    let (s, _, _) = send(&ctx.app, lc(&admin, "DWG-BV-P", "approve")).await;
    assert_eq!(s, 200);

    // 快照表落行（Review Focus #4 前半）
    let snaps: i64 = sqlx::query_scalar("SELECT count(*) FROM bom_edge_snapshot WHERE parent=$1")
        .bind(uuid::Uuid::parse_str(&pid).unwrap())
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(snaps, 2, "v1 冻结 2 条边");
    let rel: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bom_release WHERE parent=$1 AND major=1")
            .bind(uuid::Uuid::parse_str(&pid).unwrap())
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert_eq!(rel, 1);

    // expand 缺省（live）与 ?major=1 一致
    let (s, body, _) = send(&ctx.app, expand(&ed, "DWG-BV-P", "?mode=full")).await;
    assert_eq!(s, 200);
    assert_eq!(parse_nd(&body).len(), 2);
    let (s, body1, _) = send(&ctx.app, expand(&ed, "DWG-BV-P", "?mode=full&major=1")).await;
    assert_eq!(s, 200);
    assert_eq!(parse_nd(&body1), parse_nd(&body), "as-of(1)==live(v1 时点)");

    // revise → working 2.0 → 改 live BOM = C2×5
    let (s, _, _) = send(&ctx.app, lc(&ed, "DWG-BV-P", "revise")).await;
    assert_eq!(s, 200);
    let (s, _, _) = send(
        &ctx.app,
        set_lines(
            &ed,
            "DWG-BV-P",
            format!(r#"[{{"line_no":1,"child":"{c2}","qty":5}}]"#),
        ),
    )
    .await;
    assert_eq!(s, 204);

    // 隔离钉：live=C2×5；major=1 仍=C1×2+C2×1（冻结未污染）
    let (s, bodyv2, _) = send(&ctx.app, expand(&ed, "DWG-BV-P", "?mode=full")).await;
    assert_eq!(s, 200);
    let live = parse_nd(&bodyv2);
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].0, c2);
    assert_eq!(live[0].1, 5);
    let (s, bodyv1, _) = send(&ctx.app, expand(&ed, "DWG-BV-P", "?mode=full&major=1")).await;
    assert_eq!(s, 200);
    let frozen = parse_nd(&bodyv1);
    assert_eq!(frozen.len(), 2, "v1 冻结形态不被 v2 编辑污染");
    assert!(frozen.contains(&(c1.clone(), 2)));
    assert!(frozen.contains(&(c2.clone(), 1)));

    // v2 发布 → major=2 as-of=新形态
    let (s, _, _) = send(&ctx.app, lc(&ed, "DWG-BV-P", "submit")).await;
    assert_eq!(s, 200);
    let (s, _, _) = send(&ctx.app, lc(&admin, "DWG-BV-P", "approve")).await;
    assert_eq!(s, 200);
    let (s, body2, _) = send(&ctx.app, expand(&ed, "DWG-BV-P", "?mode=onelevel&major=2")).await;
    assert_eq!(s, 200);
    assert_eq!(parse_nd(&body2), vec![(c2, 5)]);

    // 终审 I1：expand（live 与 ?major=）ABAC 越级 404 不泄露存在性
    sqlx::query("UPDATE cm_object SET secret_level = 2 WHERE obj_no = 'DWG-BV-P'")
        .execute(&ctx.pool)
        .await
        .unwrap();
    for q in ["?mode=full", "?mode=full&major=1", "?mode=onelevel&major=2"] {
        let (s, _, _) = send(&ctx.app, expand(&ed, "DWG-BV-P", q)).await;
        assert_eq!(s, 404, "越密级 expand{q} 必须 404");
    }
    let (s, _, _) = send(&ctx.app, expand(&admin, "DWG-BV-P", "?mode=full&major=1")).await;
    assert_eq!(s, 200, "admin（上限内）不受影响");
    ctx.cleanup().await;
}

// ==================== IM 卡片审批（增量10 Review Focus #4） ====================

fn im_sign_hex(secret: &str, body: &str) -> String {
    use hmac::Mac;
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn im_action(body: &str, sign: Option<&str>) -> axum::http::Request<axum::body::Body> {
    let mut b = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/im/actions")
        .header("content-type", "application/json")
        .header("content-length", body.len());
    if let Some(s) = sign {
        b = b.header("x-im-sign", s);
    }
    b.body(axum::body::Body::from(body.to_string())).unwrap()
}

#[tokio::test]
async fn im_actions_full_loop_and_guards() {
    let ctx = TestCtx::setup("imact", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "imapp", "imapp-pass-123", &["editor"], 0)
        .await;
    ctx.create_user_as_admin(&admin, "imapp2", "imapp2-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("imapp", "imapp-pass-123").await;

    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-IM-A1",
            r#"{"name":"N"}"#,
            Some("a.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    let sub = |t: &str, no: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/submit"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, _, _) = send(&ctx.app, sub(&ed, "DWG-IM-A1")).await;
    assert_eq!(s, 200);

    // app.harness 需注入 im 上下文——通过环境化配置装配（im_secret 与映射在 common）
    let secret = "im-test-secret";
    // 正确签名 + 映射 + 权限 → approve → released（Review Focus #4 主链）
    // 映射由 harness 的 im_user_map：{"im-ding-1":"imapp2"}，imapp2 仅 editor……
    // 故主链用 admin 映射 {"im-admin":"admin"}；bad-perm 场景用 editor 映射。
    let body = r#"{"obj_no":"DWG-IM-A1","action":"approve","user":"im-admin"}"#;
    let sign = im_sign_hex(secret, body);
    let (s, rbody, _) = send(&ctx.app, im_action(body, Some(&sign))).await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&rbody));
    let v: serde_json::Value = serde_json::from_slice(&rbody).unwrap();
    assert_eq!(v["state"], "released", "IM 审批必须生效");
    let (s, rbody) = ctx.get_authed("/api/v1/docs/DWG-IM-A1", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&rbody).unwrap();
    assert_eq!(v["object"]["state"], "released");

    // 坏签名 → 401
    let body2 = r#"{"obj_no":"DWG-IM-A1","action":"reject","user":"im-admin"}"#;
    let (s, rbody, _) = send(&ctx.app, im_action(body2, Some("deadbeef"))).await;
    assert_eq!(s, 401);
    let v: serde_json::Value = serde_json::from_slice(&rbody).unwrap();
    assert_eq!(v["code"], "MDL-SEC-UNAUTHORIZED");

    // 未映射用户 → 403
    let body3 = r#"{"obj_no":"DWG-IM-A1","action":"approve","user":"im-stranger"}"#;
    let sign3 = im_sign_hex(secret, body3);
    let (s, _, _) = send(&ctx.app, im_action(body3, Some(&sign3))).await;
    assert_eq!(s, 403);

    // 无 docs:approve 权限（editor）→ 403
    // （secret_level 5 重新提交：released→revise→submit）
    let lc = |t: &str, no: &str, act: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/{act}"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, _, _) = send(&ctx.app, lc(&admin, "DWG-IM-A1", "revise")).await;
    assert_eq!(s, 200);
    let (s, _, _) = send(&ctx.app, sub(&ed, "DWG-IM-A1")).await;
    assert_eq!(s, 200);
    let body4 = r#"{"obj_no":"DWG-IM-A1","action":"approve","user":"im-editor"}"#;
    let sign4 = im_sign_hex(secret, body4);
    let (s, rbody, _) = send(&ctx.app, im_action(body4, Some(&sign4))).await;
    assert_eq!(s, 403, "editor 无 docs:approve 必须 403");
    let v: serde_json::Value = serde_json::from_slice(&rbody).unwrap();
    assert_eq!(v["code"], "MDL-SEC-FORBIDDEN");

    // 越密级 → 404（不泄露存在性）：密级 9 > admin 上限 3
    sqlx::query("UPDATE cm_object SET secret_level = 9 WHERE obj_no = 'DWG-IM-A1'")
        .execute(&ctx.pool)
        .await
        .unwrap();
    let body5 = r#"{"obj_no":"DWG-IM-A1","action":"approve","user":"im-admin"}"#;
    let sign5 = im_sign_hex(secret, body5);
    let (s, _, _) = send(&ctx.app, im_action(body5, Some(&sign5))).await;
    assert_eq!(s, 404, "越密级 IM 动作必须 404");
    ctx.cleanup().await;
}

#[tokio::test]
async fn im_actions_fail_closed_when_unconfigured() {
    // 终审 M1：未配 PLM_IM_SECRET → 端点 503 fail-closed（degraded harness im_secret 为空）
    use common::migrated;
    let (_good_pool, registry, watcher, _root, db, admin_url) = migrated("imdead").await;
    let bad: PgPool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://postgres:root@localhost:5432/no_such_db_e2e")
        .expect("lazy pool 创建不应失败");
    let app = common::degraded_app(registry, bad, 1024 * 1024);
    let body = r#"{"obj_no":"X","action":"approve","user":"u"}"#;
    let (status, _, _) = send(&app, im_action(body, None)).await;
    assert_eq!(status, 503, "未配置密钥必须 503 fail-closed");
    watcher.abort();

    let admin = PgPool::connect(&admin_url).await.unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
}

// ==================== 用户管理 list_users（增量11 Review Focus #5） ====================

#[tokio::test]
async fn sec_users_list_requires_manage_and_aggregates_roles() {
    let ctx = TestCtx::setup("secu", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "secued", "secued-pass-123", &["editor"], 0)
        .await;
    ctx.create_user_as_admin(&admin, "secuview", "secuview-pass-123", &["viewer"], 0)
        .await;
    let ed = ctx.login_as("secued", "secued-pass-123").await;
    let view = ctx.login_as("secuview", "secuview-pass-123").await;

    // admin：200 + 角色聚合正确
    let (s, body) = ctx.get_authed("/api/v1/sec/users", &admin).await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = v["items"].as_array().expect("items 数组");
    assert!(v["page"] == 1 && v["size"] == 50, "分页信封");
    assert!(items.len() >= 3, "admin+editor+viewer 至少 3 用户");
    let find = |name: &str| {
        items
            .iter()
            .find(|u| u["username"] == name)
            .unwrap()
            .clone()
    };
    let a = find("admin");
    assert_eq!(a["roles"].as_array().unwrap().len(), 1);
    assert_eq!(a["roles"][0], "admin");
    let e = find("secued");
    assert_eq!(e["roles"][0], "editor");
    assert!(e["max_secret_level"].is_i64());
    assert!(e["created_at"].is_string());
    // 多角色聚合（editor+viewer 双角色）
    ctx.grant_role_as_admin(&admin, "secued", "viewer").await;
    let (s, body) = ctx.get_authed("/api/v1/sec/users", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let e2 = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == "secued")
        .unwrap()
        .clone();
    let roles: Vec<&str> = e2["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert!(
        roles.contains(&"editor") && roles.contains(&"viewer"),
        "roles={roles:?}"
    );

    // 终审 M3：回收全部角色 → roles=[] 分支（COALESCE 空数组）
    {
        let req = |r: &str| {
            axum::http::Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/sec/users/secued/roles/{r}"))
                .header("authorization", format!("Bearer {admin}"))
                .body(axum::body::Body::empty())
                .unwrap()
        };
        for r in ["editor", "viewer"] {
            let (s, _, _) = send(&ctx.app, req(r)).await;
            assert_eq!(s, 204, "revoke {r}");
        }
        let (s, body) = ctx.get_authed("/api/v1/sec/users", &admin).await;
        assert_eq!(s, 200);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let e3 = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["username"] == "secued")
            .unwrap()
            .clone();
        assert_eq!(
            e3["roles"].as_array().unwrap().len(),
            0,
            "无角色用户 roles 必须为 []"
        );
    }

    // editor/viewer：403（sec:manage 门控）
    for t in [&ed, &view] {
        let (s, rbody) = ctx.get_authed("/api/v1/sec/users", t).await;
        assert_eq!(s, 403);
        let v: serde_json::Value = serde_json::from_slice(&rbody).unwrap();
        assert_eq!(v["code"], "MDL-SEC-FORBIDDEN");
    }
    ctx.cleanup().await;
}

// ==================== 增量12：login/refresh 响应附 roles（Review Focus #1） ====================

#[tokio::test]
async fn login_and_refresh_responses_carry_roles() {
    let ctx = TestCtx::setup("roles", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "roleed", "roleed-pass-123", &["editor"], 0)
        .await;
    // 双角色（editor+viewer 聚合有序）
    ctx.grant_role_as_admin(&admin, "roleed", "viewer").await;

    let login = |u: &str, p: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(format!(
                r#"{{"username":"{u}","password":"{p}"}}"#
            )))
            .unwrap()
    };
    let (s, body, _) = send(&ctx.app, login("roleed", "roleed-pass-123")).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let roles: Vec<&str> = v["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert_eq!(roles, vec!["editor", "viewer"], "login roles 聚合有序");

    // admin 单角色
    let (_, body, _) = send(&ctx.app, login("admin", common::TEST_ADMIN_PASSWORD)).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let roles: Vec<&str> = v["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert_eq!(roles, vec!["admin"]);

    // refresh 轮换后 roles 仍在
    let refresh_token = v["refresh_token"].as_str().unwrap().to_string();
    let rr = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/auth/refresh")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(format!(
            r#"{{"refresh_token":"{refresh_token}"}}"#
        )))
        .unwrap();
    let (s, body, _) = send(&ctx.app, rr).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let roles: Vec<&str> = v["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert_eq!(roles, vec!["admin"], "refresh 响应必须仍携带 roles");
    ctx.cleanup().await;
}

// ==================== 增量13：死信分页 + webhook 强校验（Review Focus #1/#2） ====================

fn create_wh(body: &str, t: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/sec/webhooks")
        .header("authorization", format!("Bearer {t}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn dead_events_pagination_and_strong_validation() {
    let ctx = TestCtx::setup("dl", 1024 * 1024).await;
    let admin = ctx.login().await;

    // 强校验：无 http(s) 前缀 / secret<8 / 未知 topic 各 400
    for (body, why) in [
        (
            r#"{"url":"ftp://x.example/hook","topics":["doc.released"],"secret":"longsecret1"}"#,
            "非 http(s)",
        ),
        (
            r#"{"url":"https://x.example/hook","topics":["doc.released"],"secret":"short"}"#,
            "secret 过短",
        ),
        (
            r#"{"url":"https://x.example/hook","topics":["doc.published"],"secret":"longsecret1"}"#,
            "未知 topic",
        ),
    ] {
        let (s, rbody, _) = send(&ctx.app, create_wh(body, &admin)).await;
        assert_eq!(s, 400, "{why}: body={}", String::from_utf8_lossy(&rbody));
        let v: serde_json::Value = serde_json::from_slice(&rbody).unwrap();
        assert_eq!(v["code"], "MDL-BAD-REQUEST");
    }
    // 终审 I1 边界钉：曾致 panic 的畸形输入 → 400（非 5xx/断连）
    for body in [
        r#"{"url":"http://","topics":["doc.released"],"secret":"longsecret1"}"#,
        r#"{"url":"http://中","topics":["doc.released"],"secret":"longsecret1"}"#,
        r#"{"url":"https://localhost/hook","topics":["doc.released"],"secret":"longsecret1"}"#,
    ] {
        let (s, _, _) = send(&ctx.app, create_wh(body, &admin)).await;
        assert_eq!(s, 400, "畸形输入必须 400: {body}");
    }

    // 合法样本 → 201
    let (s, _, _) = send(
        &ctx.app,
        create_wh(
            r#"{"url":"https://ok.example/hook","topics":["doc.released"],"secret":"longsecret1"}"#,
            &admin,
        ),
    )
    .await;
    assert_eq!(s, 201);

    // 造 3 条死信（直插 outbox 并置 status=2——dispatcher 语义已另有钉）
    for i in 1..=3 {
        sqlx::query("INSERT INTO event_outbox(topic, payload, status, attempts, error) VALUES ('doc.released', $1::jsonb, 2, 4, 'boom')")
            .bind(serde_json::json!({"obj_no": format!("DWG-DL-{i}")}).to_string())
            .execute(&ctx.pool)
            .await
            .unwrap();
    }
    // size=2 → 第 1 页 2 行（id DESC）+ total=3；第 2 页 1 行
    let (s, body) = ctx
        .get_authed("/api/v1/sec/webhooks/dead?page=1&size=2", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 3);
    assert_eq!(v["page"], 1);
    assert_eq!(v["size"], 2);
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(
        items[0]["attempts"].is_i64() && items[0]["error"] == "boom",
        "死信须带排查字段"
    );

    let (s, body) = ctx
        .get_authed("/api/v1/sec/webhooks/dead?page=2&size=2", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 1);

    // size clamp：>200 → 上限（不 500）
    let (s, body) = ctx
        .get_authed("/api/v1/sec/webhooks/dead?page=1&size=9999", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["size"], 200, "size 须 clamp 至 200");
    // page 越界 → 空集
    let (s, body) = ctx
        .get_authed("/api/v1/sec/webhooks/dead?page=9&size=2", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 0);
    // 终审 R2：极端 page 防 M1 回退（u32::MAX × 200 不得溢出/5xx）
    let (s, body) = ctx
        .get_authed("/api/v1/sec/webhooks/dead?page=4294967295&size=200", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 0);
    ctx.cleanup().await;
}

// ==================== 增量13：list_users 搜索（Review Focus #5） ====================

#[tokio::test]
async fn sec_users_search_fuzzy_filter() {
    let ctx = TestCtx::setup("srch", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "zhangsan", "zs-pass-123", &["editor"], 0)
        .await;
    ctx.create_user_as_admin(&admin, "zhangsi", "zs2-pass-123", &["viewer"], 0)
        .await;
    ctx.create_user_as_admin(&admin, "lisi", "ls-pass-123", &["viewer"], 0)
        .await;

    // 前缀模糊命中 zhang*
    let (s, body) = ctx
        .get_authed("/api/v1/sec/users?search=zhang", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let names: Vec<&str> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["username"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "zhang 前缀须命中 2 人: {names:?}");
    assert!(names.contains(&"zhangsan") && names.contains(&"zhangsi"));
    // 未命中 → 空
    let (s, body) = ctx
        .get_authed("/api/v1/sec/users?search=nonexistent", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 0);

    // 增量14：分页 page=2/size=2 + clamp
    let (s, body) = ctx
        .get_authed("/api/v1/sec/users?page=2&size=2", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["page"], 2);
    assert_eq!(v["size"], 2);
    assert_eq!(v["total"], 4);
    assert_eq!(v["items"].as_array().unwrap().len(), 2);
    ctx.cleanup().await;
}

// ==================== 增量14：where-used as-of（Review Focus #2/#3） ====================

#[tokio::test]
async fn where_used_asof_via_api() {
    async fn id_of(ctx: &TestCtx, token: &str, no: &str) -> String {
        let (s, body) = ctx.get_authed(&format!("/api/v1/docs/{no}"), token).await;
        assert_eq!(s, 200);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        v["object"]["id"].as_str().unwrap().to_string()
    }
    async fn set_lines(ctx: &TestCtx, token: &str, lines: String) -> (u16, bytes::Bytes) {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/boms/DWG-WU-A/lines")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(lines))
            .unwrap();
        let (s, b, _) = send(&ctx.app, req).await;
        (s, b)
    }
    async fn lifecycle(ctx: &TestCtx, token: &str, act: &str) -> u16 {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/DWG-WU-A/{act}"))
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let (s, _, _) = send(&ctx.app, req).await;
        s
    }

    let ctx = TestCtx::setup("wua", 1024 * 1024).await;
    let admin = ctx.login().await;
    for no in ["DWG-WU-A", "DWG-WU-C"] {
        let (s, _, _) = send(
            &ctx.app,
            post_docs_multipart(
                Some(&admin),
                no,
                r#"{"name":"N"}"#,
                Some("w.dwg"),
                FILE.to_vec(),
            ),
        )
        .await;
        assert_eq!(s, 201, "{no}");
    }
    let (aid, cid) = (
        id_of(&ctx, &admin, "DWG-WU-A").await,
        id_of(&ctx, &admin, "DWG-WU-C").await,
    );
    let (s, b) = set_lines(
        &ctx,
        &admin,
        format!(r#"[{{"line_no":1,"child":"{cid}","qty":2}}]"#),
    )
    .await;
    assert_eq!(s, 204, "{}", String::from_utf8_lossy(&b));
    for act in ["submit", "approve", "revise"] {
        assert_eq!(lifecycle(&ctx, &admin, act).await, 200);
    }
    let (s, b) = set_lines(
        &ctx,
        &admin,
        format!(r#"[{{"line_no":1,"child":"{cid}","qty":9}}]"#),
    )
    .await;
    assert_eq!(s, 204, "{}", String::from_utf8_lossy(&b));
    // v2 发布 → 冻结 (A,2) 快照
    for act in ["submit", "approve"] {
        assert_eq!(lifecycle(&ctx, &admin, act).await, 200);
    }
    let _ = aid;

    // live：ancestor 字段兼容
    let (s, body) = ctx
        .get_authed("/api/v1/boms/DWG-WU-C/where-used", &admin)
        .await;
    assert_eq!(s, 200);
    assert!(body.windows(8).any(|w| w == b"ancestor"), "live 字段名兼容");
    // asof：两发布事件各一行，含回填 obj_no
    let (s, body) = ctx
        .get_authed("/api/v1/boms/DWG-WU-C/where-used?asof=1", &admin)
        .await;
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&body));
    let text = String::from_utf8_lossy(&body);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "两发布事件各一行: {text}");
    let rows: Vec<serde_json::Value> = lines
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(rows.iter().any(|r| r["release_root_no"] == "DWG-WU-A"
        && r["major"] == 1
        && r["parent_no"] == "DWG-WU-A"
        && r["qty_total"] == 2));
    assert!(rows.iter().any(|r| r["release_root_no"] == "DWG-WU-A"
        && r["major"] == 2
        && r["parent_no"] == "DWG-WU-A"
        && r["qty_total"] == 9));

    // 终审 C1：越密级 parent 不得经反查泄露（child 低密级合法，parent 抬密级 3=恰为 admin 上限的边界）
    ctx.create_user_as_admin(&admin, "wued", "wued-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("wued", "wued-pass-123").await;
    sqlx::query("UPDATE cm_object SET secret_level = 3 WHERE obj_no = 'DWG-WU-A'")
        .execute(&ctx.pool)
        .await
        .unwrap();
    // ed 对 parent（密级3）基础可见性=404（模型语义基线；ed 上限 0）
    let (s, _) = ctx.get_authed("/api/v1/docs/DWG-WU-A", &ed).await;
    assert_eq!(s, 404, "前置基线：越密级 parent detail 404");
    // 反查（live 与 asof）不得泄露 parent 的存在性/件号
    let (s, body) = ctx
        .get_authed("/api/v1/boms/DWG-WU-C/where-used?asof=1", &ed)
        .await;
    assert_eq!(s, 200, "child 合法，反查 200");
    let text = String::from_utf8_lossy(&body);
    assert!(
        !text.contains("DWG-WU-A"),
        "越密级 parent 件号不得出现: {text}"
    );
    // child 自身越级 → 404（I1：两形态钉）
    sqlx::query("UPDATE cm_object SET secret_level = 5 WHERE obj_no = 'DWG-WU-C'")
        .execute(&ctx.pool)
        .await
        .unwrap();
    let (s, _) = ctx
        .get_authed("/api/v1/boms/DWG-WU-C/where-used", &ed)
        .await;
    assert_eq!(s, 404, "越密级 child live 反查必须 404");
    let (s, _) = ctx
        .get_authed("/api/v1/boms/DWG-WU-C/where-used?asof=1", &ed)
        .await;
    assert_eq!(s, 404, "越密级 child asof 反查必须 404");

    // ===== 增量15：live 对称过滤 + ancestor_no（终审 R2 镜像） =====
    // 复位：child 回密级 0；parent 保持密级 3（越密级 ancestor，恰为 admin 上限的边界）
    sqlx::query("UPDATE cm_object SET secret_level = 0 WHERE obj_no = 'DWG-WU-C'")
        .execute(&ctx.pool)
        .await
        .unwrap();
    // ed 视角 live 反查：ancestor(A, 密级3) 整行丢弃（对称过滤；ed 上限 0）
    let (s, body) = ctx
        .get_authed("/api/v1/boms/DWG-WU-C/where-used", &ed)
        .await;
    assert_eq!(s, 200, "child 合法，live 反查 200");
    let text = String::from_utf8_lossy(&body);
    assert!(
        !text.contains(aid.as_str()),
        "越密级 ancestor UUID 不得出现（对称过滤）: {text}"
    );
    assert!(
        !text.contains("DWG-WU-A"),
        "越密级 ancestor 件号不得出现: {text}"
    );
    // admin 视角：可见且带 ancestor_no 回填（件号而非仅 UUID）
    let (s, body) = ctx
        .get_authed("/api/v1/boms/DWG-WU-C/where-used", &admin)
        .await;
    assert_eq!(s, 200);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains(aid.as_str()), "admin 可见 ancestor UUID");
    assert!(
        text.contains("ancestor_no") && text.contains("DWG-WU-A"),
        "ancestor_no 回填件号: {text}"
    );
    ctx.cleanup().await;
}

// ==================== 增量16：COD 自动赋码（Review Focus #3/#4/#6） ====================

#[tokio::test]
async fn cod_auto_code_and_preview() {
    let ctx = TestCtx::setup("cod", 1024 * 1024).await;
    let admin = ctx.login().await;
    // drawing 类型配 cod_rule（registry 热生效——先创建类型时未配）
    sqlx::query(
        "UPDATE cm_type SET config = config || '{\"cod_rule\":{\"prefix\":\"DWG-\",\"date_fmt\":\"yyyyMMdd\",\"seq_digits\":4}}'::jsonb WHERE code='drawing'",
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    ctx.registry.reload_if_changed().await.unwrap();

    // preview 不烧号：3 次预览后首个 create 仍为 0001（Review Focus #4）
    let (s, body) = ctx
        .get_authed("/api/v1/meta/types/drawing/cod-preview?n=3", &admin)
        .await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let codes = v["codes"].as_array().unwrap();
    assert_eq!(codes.len(), 3);
    let today = chrono::Utc::now().format("%Y%m%d").to_string();
    assert_eq!(
        codes[0],
        format!("DWG-{today}-0001"),
        "preview 不烧号：首个 create 仍应为 0001"
    );

    // multipart auto_code=1（obj_no 空）
    let (ct, body) = common::multipart_body(
        "XyZabc",
        &[
            ("obj_no", None, b"".to_vec()),
            (
                "attrs",
                None,
                r#"{"name":"自动码件"}"#.to_string().into_bytes(),
            ),
            ("file", Some("c.dwg"), FILE.to_vec()),
            ("auto_code", None, b"1".to_vec()),
        ],
    );
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/docs")
        .header("content-type", ct)
        .header("content-length", body.len())
        .header("authorization", format!("Bearer {admin}"))
        .body(axum::body::Body::from(body))
        .unwrap();
    let (s, rbody, _) = send(&ctx.app, req).await;
    assert_eq!(s, 201, "body={}", String::from_utf8_lossy(&rbody));
    let v: serde_json::Value = serde_json::from_slice(&rbody).unwrap();
    let obj_no = v["object"]["obj_no"].as_str().unwrap().to_string();
    // 终审 I1：精确到 seq——preview 不烧号（若回归烧号，此处会是 -0004）
    let today = chrono::Utc::now().format("%Y%m%d").to_string();
    assert_eq!(
        obj_no,
        format!("DWG-{today}-0001"),
        "preview 后首个 create 必须仍为 0001"
    );

    // 无规则类型（bomtest 未建——用 second 类型验证 400）：
    // mod_doc::DocService 只认 drawing；改走 mod 层…… 网关层用另一个类型不可行——
    // 这里钉"registry 热生效"：移除 cod_rule → auto_code 再 create → 400
    sqlx::query("UPDATE cm_type SET config = config - 'cod_rule' WHERE code='drawing'")
        .execute(&ctx.pool)
        .await
        .unwrap();
    ctx.registry.reload_if_changed().await.unwrap();
    let (ct2, body2) = common::multipart_body(
        "XyZdef",
        &[
            ("obj_no", None, b"".to_vec()),
            (
                "attrs",
                None,
                r#"{"name":"无规则件"}"#.to_string().into_bytes(),
            ),
            ("file", Some("c.dwg"), FILE.to_vec()),
            ("auto_code", None, b"1".to_vec()),
        ],
    );
    let req2 = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/docs")
        .header("content-type", ct2)
        .header("content-length", body2.len())
        .header("authorization", format!("Bearer {admin}"))
        .body(axum::body::Body::from(body2))
        .unwrap();
    let (s, rbody2, _) = send(&ctx.app, req2).await;
    assert_eq!(
        s,
        400,
        "无规则 auto_code 必须 400: {}",
        String::from_utf8_lossy(&rbody2)
    );
    ctx.cleanup().await;
}

// ==================== 增量17：PART 物料（Review Focus #1/#2/#3/#5） ====================

#[tokio::test]
async fn parts_create_duplicates_tree_and_permissions() {
    let ctx = TestCtx::setup("parts", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "ptview", "ptview-pass-123", &["viewer"], 0)
        .await;
    let view = ctx.login_as("ptview", "ptview-pass-123").await;

    let post_part = |t: &str, body: String| {
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/parts")
            .header("authorization", format!("Bearer {t}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap()
    };

    // 创建：分类赋码（ZZ 与 JGJ 独立流水）
    let (s, body, _) = send(
        &ctx.app,
        post_part(
            &admin,
            r#"{"class_code":"CLS-ZZ","attrs":{"name":"支座A","material":"Q235"}}"#.into(),
        ),
    )
    .await;
    assert_eq!(s, 201, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["obj_no"], "WL-ZZ-00001");
    let (s, body, _) = send(
        &ctx.app,
        post_part(
            &admin,
            r#"{"class_code":"CLS-ZZ-JGJ","attrs":{"name":"螺栓B","material":"304"}}"#.into(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    let v2: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v2["obj_no"], "WL-ZZ-JGJ-00001");

    // 查重：同名相似命中（admin）
    let (s, body) = ctx
        .get_authed("/api/v1/parts/duplicates?name=支座", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(v
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["obj_no"] == "WL-ZZ-00001"));

    // viewer：无 docs:write → 创建 403；列表 200（docs:read）
    let (s, rbody, _) = send(
        &ctx.app,
        post_part(
            &view,
            r#"{"class_code":"CLS-ZZ","attrs":{"name":"x"}}"#.into(),
        ),
    )
    .await;
    assert_eq!(s, 403);
    let v: serde_json::Value = serde_json::from_slice(&rbody).unwrap();
    assert_eq!(v["code"], "MDL-SEC-FORBIDDEN");
    let (s, body) = ctx.get_authed("/api/v1/parts", &view).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 2);

    // ABAC：抬密级 → viewer 列表不见
    sqlx::query("UPDATE cm_object SET secret_level = 2 WHERE obj_no = 'WL-ZZ-00001'")
        .execute(&ctx.pool)
        .await
        .unwrap();
    let (s, body) = ctx.get_authed("/api/v1/parts", &view).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 1, "越密级物料不得出现在 viewer 列表");

    // 终审 C1：admin（真实上限 3）仍可见密级 2 物料（require_perm 回填生效——非硬编码 0）
    let (s, body) = ctx.get_authed("/api/v1/parts", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 2, "admin 真实密级上限必须生效（C1）");
    let (s, body) = ctx
        .get_authed("/api/v1/parts/duplicates?name=支座", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        v.as_array()
            .unwrap()
            .iter()
            .any(|d| d["obj_no"] == "WL-ZZ-00001"),
        "admin duplicates 也须可见（C1）"
    );

    // 终审 C1：零角色用户（经 revoke 端点回收——casbin reload 生效路径）→ 403
    ctx.create_user_as_admin(&admin, "seconly", "seconly-pass-123", &["editor"], 0)
        .await;
    for r in ["editor"] {
        let req = axum::http::Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/sec/users/seconly/roles/{r}"))
            .header("authorization", format!("Bearer {admin}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let (s, _, _) = send(&ctx.app, req).await;
        assert_eq!(s, 204, "revoke {r}");
    }
    let sec_user = ctx.login_as("seconly", "seconly-pass-123").await;
    let (s, rbody) = ctx.get_authed("/api/v1/parts", &sec_user).await;
    assert_eq!(s, 403, "零角色用户无 docs:read 必须 403");
    let v: serde_json::Value = serde_json::from_slice(&rbody).unwrap();
    assert_eq!(v["code"], "MDL-SEC-FORBIDDEN");

    // 分类树
    let (s, body) = ctx.get_authed("/api/v1/classes", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let roots = v.as_array().unwrap();
    assert!(roots
        .iter()
        .any(|n| n["code"] == "CLS-ZZ" && n["children"].as_array().unwrap().len() == 2));
    ctx.cleanup().await;
}

// ==================== 增量18：审计日志（Review Focus #1/#2/#3） ====================

#[tokio::test]
async fn audit_log_records_and_searches() {
    let ctx = TestCtx::setup("aud", 1024 * 1024).await;
    let admin = ctx.login().await;

    // 登录失败 → login.fail 留痕（安全事件）
    let bad = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            r#"{"username":"admin","password":"wrong"}"#,
        ))
        .unwrap();
    let (s, _, _) = send(&ctx.app, bad).await;
    assert_eq!(s, 401);

    // 兜底埋点：authed GET /docs → http 行
    let _ = ctx.get_authed("/api/v1/docs", &admin).await;
    // 显式埋点：flow-tier 变更（含新旧值+操作人）
    let ft = axum::http::Request::builder()
        .method("PUT")
        .uri("/api/v1/meta/types/drawing/flow-tier")
        .header("authorization", format!("Bearer {admin}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(r#"{"tier":"minimal"}"#))
        .unwrap();
    let (s, _, _) = send(&ctx.app, ft).await;
    assert_eq!(s, 204);

    // spawn 异步写入——轮询等落库（最多 2s）
    let mut ok = false;
    for _ in 0..20 {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_log WHERE username='admin' AND action IN ('login.fail','flow-tier.change','http')",
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
        if n >= 3 {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(ok, "兜底+显式埋点须落库");

    // flow-tier 行语义：含 from/to + 操作人 admin
    let row: (String, serde_json::Value) =
        sqlx::query_as("SELECT username, detail FROM audit_log WHERE action='flow-tier.change'")
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert_eq!(row.0, "admin");
    assert_eq!(row.1["from"], "standard");
    assert_eq!(row.1["to"], "minimal");

    // login.fail 行存在
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action='login.fail'")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // 检索端点：action 过滤 + 权限（viewer 403）
    ctx.create_user_as_admin(&admin, "audview", "audview-pass-123", &["viewer"], 0)
        .await;
    let view = ctx.login_as("audview", "audview-pass-123").await;
    let (s, body) = ctx
        .get_authed("/api/v1/audit?action=flow-tier.change", &admin)
        .await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["action"], "flow-tier.change");

    // 终审 I1：多维检索——username 组合
    let (s, body) = ctx
        .get_authed("/api/v1/audit?username=admin&action=http", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        v["total"].as_i64().unwrap() >= 1,
        "admin 的 http 兜底行存在"
    );
    let (s, body) = ctx
        .get_authed("/api/v1/audit?username=audview&action=http", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 0, "audview 未发 authed 请求，http 行为 0");

    // 时间窗：from=未来 → 空
    let (s, body) = ctx
        .get_authed("/api/v1/audit?from=2099-01-01T00:00:00Z", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 0, "未来时间窗为空");

    // 分页：size=1 两页不同行（offset 语义）
    let (s, body) = ctx.get_authed("/api/v1/audit?page=1&size=1", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    assert!(v["total"].as_i64().unwrap() >= 2);
    let p1 = v["items"][0]["id"].as_i64().unwrap();
    let (s, body) = ctx.get_authed("/api/v1/audit?page=2&size=1", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let p2 = v["items"][0]["id"].as_i64().unwrap();
    assert_ne!(p1, p2, "page=2 不得与 page=1 同行");

    // RF1 强化：GET /docs 兜底行 path 值级归因
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action='http' AND detail->>'path'='/api/v1/docs'",
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(n >= 1, "GET /docs 兜底行 path 归因");

    let (s, rbody) = ctx.get_authed("/api/v1/audit", &view).await;
    assert_eq!(s, 403, "viewer 无 sec:manage 必须 403");
    let v: serde_json::Value = serde_json::from_slice(&rbody).unwrap();
    assert_eq!(v["code"], "MDL-SEC-FORBIDDEN");

    // append-only 静态守卫：全仓无 audit_log UPDATE/DELETE
    ctx.cleanup().await;
}

// ==================== 增量19 WF-03 待办中心 ====================

#[tokio::test]
async fn todos_center_three_boxes_and_abac() {
    let ctx = TestCtx::setup("todo", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "todoed", "todoed-pass-123", &["editor"], 0)
        .await;
    ctx.create_user_as_admin(&admin, "todolow", "todolow-pass-123", &["editor"], 0)
        .await;
    // peer：无任何自身提交的纯观察者——待办可见性只由他人提交+密级决定（ABAC 钉去混淆）
    ctx.create_user_as_admin(&admin, "todopeer", "todopeer-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("todoed", "todoed-pass-123").await;
    let low = ctx.login_as("todolow", "todolow-pass-123").await;
    let peer = ctx.login_as("todopeer", "todopeer-pass-123").await;

    // todoed 上传 level-0 件并提交；todolow 上传密级 2 件并提交
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-TODO-1",
            r#"{"name":"轴承座"}"#,
            Some("t1.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&low),
            "DWG-TODO-S",
            r#"{"name":"保密件"}"#,
            Some("ts.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);

    let sub = |t: &str, no: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/submit"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, _, _) = send(&ctx.app, sub(&ed, "DWG-TODO-1")).await;
    assert_eq!(s, 200);
    let (s, _, _) = send(&ctx.app, sub(&low, "DWG-TODO-S")).await;
    assert_eq!(s, 200);
    // 提交后再提密（fail-closed ABAC 下低权限者无法操作越密对象；密级变更受控属 SYS 域）
    sqlx::query("UPDATE cm_object SET secret_level = 2 WHERE obj_no = 'DWG-TODO-S'")
        .execute(&ctx.pool)
        .await
        .unwrap();

    // 待办盒（admin）：两件都在，提交人/名称/状态齐
    let (s, body) = ctx.get_authed("/api/v1/todos?box=todo", &admin).await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 2);
    let items = v["items"].as_array().unwrap();
    let t1 = items
        .iter()
        .find(|i| i["obj_no"] == "DWG-TODO-1")
        .expect("待办含 TODO-1");
    assert_eq!(t1["submitter"], "todoed");
    assert_eq!(t1["name"], "轴承座");
    assert_eq!(t1["state"], "review");
    let ts = items
        .iter()
        .find(|i| i["obj_no"] == "DWG-TODO-S")
        .expect("待办含 TODO-S");
    assert_eq!(ts["submitter"], "todolow");

    // 待办盒（editor）：自己的提交不算待办；密级 2 件越权不可见 → 空
    let (s, body) = ctx.get_authed("/api/v1/todos?box=todo", &ed).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 0, "editor 待办应为空");

    // ABAC 钉（peer 无自身提交）：只见 level-0 的 TODO-1，level-2 的 TODO-S 被密级过滤
    let (s, body) = ctx.get_authed("/api/v1/todos?box=todo", &peer).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 1, "peer 待办恰一件");
    assert_eq!(v["items"][0]["obj_no"], "DWG-TODO-1");

    // 待办盒（todolow）：TODO-1 可见（密级 0、他人提交）；自己的 TODO-S 不入待办
    let (s, body) = ctx.get_authed("/api/v1/todos?box=todo", &low).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 1, "low 待办恰一件");
    assert_eq!(v["items"][0]["obj_no"], "DWG-TODO-1");

    // 我发起盒：editor 见自己的 DWG-TODO-1；low 无
    let (s, body) = ctx.get_authed("/api/v1/todos?box=mine", &ed).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["obj_no"], "DWG-TODO-1");
    let (s, body) = ctx.get_authed("/api/v1/todos?box=mine", &low).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 0);

    // admin 批准 TODO-1（无 body——向后兼容）→ 显式审计行（from/to）
    let app = |t: &str, no: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/approve"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, body, _) = send(&ctx.app, app(&admin, "DWG-TODO-1")).await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));

    // spawn 异步写审计——轮询等落库
    let mut ok = false;
    for _ in 0..20 {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_log WHERE action='approve' AND object='DWG-TODO-1'",
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
        if n == 1 {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(ok, "approve 显式审计行须落库");
    let row: serde_json::Value =
        sqlx::query("SELECT detail FROM audit_log WHERE action='approve' AND object='DWG-TODO-1'")
            .fetch_one(&ctx.pool)
            .await
            .unwrap()
            .get("detail");
    assert_eq!(row["from"], "review");
    assert_eq!(row["to"], "released");

    // 待办消失、已办出现（admin 视角 approve 行；editor 视角 submit 行）
    let (s, body) = ctx.get_authed("/api/v1/todos?box=todo", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 1, "仅剩 TODO-S");
    let (s, body) = ctx.get_authed("/api/v1/todos?box=done", &admin).await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(v["total"].as_i64().unwrap() >= 1, "admin 已办含 approve 行");
    let done = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["obj_no"] == "DWG-TODO-1" && i["action"] == "approve")
        .expect("已办含 approve 行");
    assert_eq!(done["state"], "released");
    let (s, body) = ctx.get_authed("/api/v1/todos?box=done", &ed).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let ed_done = v["items"].as_array().unwrap();
    assert!(
        ed_done
            .iter()
            .any(|i| i["obj_no"] == "DWG-TODO-1" && i["action"] == "submit"),
        "editor 已办含自己的提交行"
    );
    assert!(
        !ed_done.iter().any(|i| i["action"] == "approve"),
        "editor 无审批动作"
    );

    // 非法盒名 → 400
    let (s, _) = ctx.get_authed("/api/v1/todos?box=bogus", &admin).await;
    assert_eq!(s, 400);

    ctx.cleanup().await;
}

#[tokio::test]
async fn lifecycle_comment_record_and_require_switch() {
    let ctx = TestCtx::setup("cmt", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "cmted", "cmted-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("cmted", "cmted-pass-123").await;

    let upload = |t: &str, no: &str| {
        send(
            &ctx.app,
            post_docs_multipart(Some(t), no, r#"{"name":"N"}"#, Some("c.dwg"), FILE.to_vec()),
        )
    };
    let sub = |t: &str, no: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/submit"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let act_json = |t: &str, no: &str, act: &str, body: Option<&str>| {
        let bld = axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/{act}"))
            .header("authorization", format!("Bearer {t}"));
        match body {
            Some(s) => bld
                .header("content-type", "application/json")
                .body(axum::body::Body::from(s.to_string()))
                .unwrap(),
            None => bld.body(axum::body::Body::empty()).unwrap(),
        }
    };

    // ① approve 带 JSON 意见 → 200；audit 显式行含 comment/from/to
    let (s, _, _) = upload(&ed, "DWG-CMT-1").await;
    assert_eq!(s, 201);
    let (s, _, _) = send(&ctx.app, sub(&ed, "DWG-CMT-1")).await;
    assert_eq!(s, 200);
    let (s, body, _) = send(
        &ctx.app,
        act_json(
            &admin,
            "DWG-CMT-1",
            "approve",
            Some(r#"{"comment":"质量合格，准予发布"}"#),
        ),
    )
    .await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let mut ok = false;
    for _ in 0..20 {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_log WHERE action='approve' AND object='DWG-CMT-1'",
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
        if n == 1 {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(ok, "approve 审计行须落库");
    let row: serde_json::Value =
        sqlx::query("SELECT detail FROM audit_log WHERE action='approve' AND object='DWG-CMT-1'")
            .fetch_one(&ctx.pool)
            .await
            .unwrap()
            .get("detail");
    assert_eq!(row["comment"], "质量合格，准予发布");
    assert_eq!(row["from"], "review");
    assert_eq!(row["to"], "released");

    // ② reject 带意见 → working；audit reject 行含 comment
    let (s, _, _) = upload(&ed, "DWG-CMT-2").await;
    assert_eq!(s, 201);
    let (s, _, _) = send(&ctx.app, sub(&ed, "DWG-CMT-2")).await;
    assert_eq!(s, 200);
    let (s, body, _) = send(
        &ctx.app,
        act_json(
            &admin,
            "DWG-CMT-2",
            "reject",
            Some(r#"{"comment":"尺寸标注缺失"}"#),
        ),
    )
    .await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["state"], "working");
    for _ in 0..20 {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_log WHERE action='reject' AND object='DWG-CMT-2' \
             AND detail->>'comment' = '尺寸标注缺失'",
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
        if n == 1 {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(ok, "reject 审计行须含意见");

    // ③ 开启 require_comment（tier+开关同请求生效）→ 无意见 approve 400 CommentRequired
    let (s, _, _) = upload(&ed, "DWG-CMT-3").await;
    assert_eq!(s, 201);
    let (s, _, _) = send(&ctx.app, sub(&ed, "DWG-CMT-3")).await;
    assert_eq!(s, 200);
    let sw = axum::http::Request::builder()
        .method("PUT")
        .uri("/api/v1/meta/types/drawing/flow-tier")
        .header("authorization", format!("Bearer {admin}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            r#"{"tier":"standard","require_comment":true}"#,
        ))
        .unwrap();
    let (s, _, _) = send(&ctx.app, sw).await;
    assert_eq!(s, 204);
    // 热刷新等待（registry NOTIFY/兜底轮询 ≤3s；与 flow_tier 测试同约定）
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    // submit 不受开关约束（提交无意见输入）
    let (s, _, _) = upload(&ed, "DWG-CMT-4").await;
    assert_eq!(s, 201);
    let (s, _, _) = send(&ctx.app, sub(&ed, "DWG-CMT-4")).await;
    assert_eq!(s, 200);
    // 无意见 approve → 400 + 稳定错误码
    let (s, body, _) = send(&ctx.app, act_json(&admin, "DWG-CMT-3", "approve", None)).await;
    assert_eq!(s, 400, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-LCS-COMMENT-REQUIRED");
    // 带意见 → 200
    let (s, body, _) = send(
        &ctx.app,
        act_json(&admin, "DWG-CMT-3", "approve", Some(r#"{"comment":"ok"}"#)),
    )
    .await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    // 空串意见同样视为缺失
    let (s, body, _) = send(
        &ctx.app,
        act_json(&admin, "DWG-CMT-4", "approve", Some(r#"{"comment":"  "}"#)),
    )
    .await;
    assert_eq!(s, 400, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-LCS-COMMENT-REQUIRED");

    // ④ IM 回调豁免（HMAC 可信路径，卡片无意见输入）——开关开启下无意见仍 200
    let (s, _, _) = upload(&ed, "DWG-CMT-5").await;
    assert_eq!(s, 201);
    let (s, _, _) = send(&ctx.app, sub(&ed, "DWG-CMT-5")).await;
    assert_eq!(s, 200);
    let body = serde_json::json!({"user": "im-admin", "obj_no": "DWG-CMT-5", "action": "approve"});
    let (s, _, _) = send(
        &ctx.app,
        im_action(
            &body.to_string(),
            Some(&im_sign_hex("im-test-secret", &body.to_string())),
        ),
    )
    .await;
    assert_eq!(s, 200, "IM 回调无意见须豁免开关");

    // ⑤ 关闭开关 → 无意见 approve 照旧（可逆）
    let sw = axum::http::Request::builder()
        .method("PUT")
        .uri("/api/v1/meta/types/drawing/flow-tier")
        .header("authorization", format!("Bearer {admin}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(r#"{"require_comment":false}"#))
        .unwrap();
    let (s, _, _) = send(&ctx.app, sw).await;
    assert_eq!(s, 204);
    // 热刷新等待（关闭开关同 ≤3s）
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let (s, _, _) = upload(&ed, "DWG-CMT-6").await;
    assert_eq!(s, 201);
    let (s, _, _) = send(&ctx.app, sub(&ed, "DWG-CMT-6")).await;
    assert_eq!(s, 200);
    let (s, _, _) = send(&ctx.app, act_json(&admin, "DWG-CMT-6", "approve", None)).await;
    assert_eq!(s, 200);

    ctx.cleanup().await;
}

#[tokio::test]
async fn batch_approve_partial_results_and_guards() {
    let ctx = TestCtx::setup("bat", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "bated", "bated-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("bated", "bated-pass-123").await;

    let upload = |t: &str, no: &str| {
        send(
            &ctx.app,
            post_docs_multipart(Some(t), no, r#"{"name":"N"}"#, Some("b.dwg"), FILE.to_vec()),
        )
    };
    let sub = |t: &str, no: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/submit"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    for no in ["DWG-BAT-1", "DWG-BAT-2", "DWG-BAT-3"] {
        let (s, _, _) = upload(&ed, no).await;
        assert_eq!(s, 201);
        let (s, _, _) = send(&ctx.app, sub(&ed, no)).await;
        assert_eq!(s, 200);
    }

    let batch = |t: &str, body: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/docs/batch-approve")
            .header("authorization", format!("Bearer {t}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap()
    };

    // ① 混合批次：两件成功 + 不存在编号逐项报错（不中断批次）
    let (s, body, _) = send(
        &ctx.app,
        batch(
            &admin,
            r#"{"obj_nos":["DWG-BAT-1","DWG-BAT-2","DWG-NOPE-404"]}"#,
        ),
    )
    .await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    let find = |no: &str| items.iter().find(|i| i["obj_no"] == no).unwrap();
    assert_eq!(find("DWG-BAT-1")["ok"], true);
    assert_eq!(find("DWG-BAT-1")["state"], "released");
    assert_eq!(find("DWG-BAT-2")["ok"], true);
    assert_eq!(find("DWG-NOPE-404")["ok"], false);
    assert_eq!(find("DWG-NOPE-404")["error"], "MDL-OBJ-NOT-FOUND");
    // 审计：两件 approve 行（含 from/to）
    for _ in 0..20 {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_log WHERE action='approve' AND object LIKE 'DWG-BAT-%'",
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
        if n == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action='approve' AND object LIKE 'DWG-BAT-%'",
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(n, 2, "批量审批两件须各留审计行");

    // ② editor 无 docs:approve → 403
    let (s, _, _) = send(&ctx.app, batch(&ed, r#"{"obj_nos":["DWG-BAT-3"]}"#)).await;
    assert_eq!(s, 403);

    // ③ 超上限 50 → 400
    let many: Vec<String> = (0..51).map(|i| format!("\"DWG-X{i}\"")).collect();
    let (s, _, _) = send(
        &ctx.app,
        batch(&admin, &format!("{{\"obj_nos\":[{}]}}", many.join(","))),
    )
    .await;
    assert_eq!(s, 400);

    // ④ 空批次 → 400
    let (s, _, _) = send(&ctx.app, batch(&admin, r#"{"obj_nos":[]}"#)).await;
    assert_eq!(s, 400);

    // ⑤ require_comment 开启：批量无意见 → 逐项 ok:false CommentRequired；带意见 → 成功
    let sw = axum::http::Request::builder()
        .method("PUT")
        .uri("/api/v1/meta/types/drawing/flow-tier")
        .header("authorization", format!("Bearer {admin}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(r#"{"require_comment":true}"#))
        .unwrap();
    let (s, _, _) = send(&ctx.app, sw).await;
    assert_eq!(s, 204);
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let (s, body, _) = send(&ctx.app, batch(&admin, r#"{"obj_nos":["DWG-BAT-3"]}"#)).await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["items"][0]["ok"], false);
    assert_eq!(v["items"][0]["error"], "MDL-LCS-COMMENT-REQUIRED");
    let (s, body, _) = send(
        &ctx.app,
        batch(&admin, r#"{"obj_nos":["DWG-BAT-3"],"comment":"批量放行"}"#),
    )
    .await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["items"][0]["ok"], true);
    assert_eq!(v["items"][0]["state"], "released");

    ctx.cleanup().await;
}

// ==================== 增量20 RPT-01 全局搜索 ====================

#[tokio::test]
async fn search_global_clusters_abac_and_near_realtime() {
    let ctx = TestCtx::setup("srch", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "srched", "srched-pass-123", &["editor"], 0)
        .await;
    ctx.create_user_as_admin(&admin, "srchlow", "srchlow-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("srched", "srched-pass-123").await;
    let low = ctx.login_as("srchlow", "srchlow-pass-123").await;

    let upload = |t: &str, no: &str, name: &str| {
        send(
            &ctx.app,
            post_docs_multipart(
                Some(t),
                no,
                &format!(r#"{{"name":"{name}"}}"#),
                Some("s.dwg"),
                FILE.to_vec(),
            ),
        )
    };
    let (s, _, _) = upload(&ed, "DWG-SRCH-1", "六角轴承座").await;
    assert_eq!(s, 201);
    let (s, _, _) = upload(&admin, "DWG-SRCH-2", "保密轴承").await;
    assert_eq!(s, 201);
    sqlx::query("UPDATE cm_object SET secret_level = 2 WHERE obj_no = 'DWG-SRCH-2'")
        .execute(&ctx.pool)
        .await
        .unwrap();

    // 物料（POST /parts，分类码种子）——RPT-01 一处搜一切含物料
    let (s, _, _) = send(
        &ctx.app,
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/parts")
            .header("authorization", format!("Bearer {ed}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                r#"{"class_code":"CLS-ZZ","attrs":{"name":"六角轴承","material":"45钢"}}"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(s, 201, "建物料须成功（CLS-ZZ 种子分类）");

    // 近实时：上传后先 0 命中 → scan_once 增量 → 命中（扫描器生产态 <5s 的机制钉）
    let mut wm = chrono::DateTime::<chrono::Utc>::UNIX_EPOCH;
    let (s, body) = ctx.get_authed("/api/v1/search?q=同步带轮", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 0, "未索引前零命中");
    let (s, _, _) = upload(&ed, "DWG-SRCH-3", "同步带轮").await;
    assert_eq!(s, 201);
    let replayed = ctx.search.scan_once(&ctx.pool, &mut wm).await.unwrap();
    assert!(replayed >= 4, "首轮扫描回放全部既有行，实际 {replayed}");
    let (s, body) = ctx.get_authed("/api/v1/search?q=同步带轮", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 1, "扫描后近实时可见");

    // ① 聚类：图纸+物料双组命中
    let (s, body) = ctx.get_authed("/api/v1/search?q=轴承", &admin).await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let groups = v["groups"].as_array().unwrap();
    let drawing = groups.iter().find(|g| g["type_code"] == "drawing").unwrap();
    let part = groups.iter().find(|g| g["type_code"] == "part").unwrap();
    assert_eq!(drawing["total"], 2, "SRCH-1/SRCH-2 双件");
    assert_eq!(part["total"], 1, "物料六角轴承");
    let names: Vec<&str> = drawing["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["name"].as_str())
        .collect();
    assert!(
        names.contains(&"六角轴承座"),
        "BM25 序不定——集合断言: {names:?}"
    );
    assert!(names.contains(&"保密轴承"), "{names:?}");
    assert_eq!(drawing["items"][0]["state"], "working");

    // ② 子串（jieba 未收录「承座」——bigram 兜底）
    let (s, body) = ctx.get_authed("/api/v1/search?q=承座", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["groups"][0]["items"][0]["obj_no"], "DWG-SRCH-1");

    // ③ 编号精确
    let (s, body) = ctx.get_authed("/api/v1/search?q=DWG-SRCH-1", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 1);

    // ④ ABAC：上限 0 用户不见密级 2 的「保密轴承」
    let (s, body) = ctx.get_authed("/api/v1/search?q=轴承", &low).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let groups = v["groups"].as_array().unwrap();
    let drawing = groups.iter().find(|g| g["type_code"] == "drawing").unwrap();
    assert_eq!(drawing["total"], 1, "低上限只见 SRCH-1");
    let all_nos: Vec<&str> = groups
        .iter()
        .flat_map(|g| g["items"].as_array().unwrap())
        .filter_map(|i| i["obj_no"].as_str())
        .collect();
    assert!(!all_nos.contains(&"DWG-SRCH-2"), "越密级绝不出现");

    // ④' 终审 I1-1 硬化钉：带外提密（SQL 直改、updated_at 不动——索引陈旧窗口模拟）
    // → DB 权威复核立即剔除（低上限用户 items 空；admin 不受影响）
    let (s, _, _) = upload(&ed, "DWG-SRCH-4", "普通垫片").await;
    assert_eq!(s, 201);
    let _ = ctx.search.scan_once(&ctx.pool, &mut wm).await.unwrap();
    let (s, body) = ctx.get_authed("/api/v1/search?q=普通垫片", &low).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v["groups"][0]["items"].as_array().unwrap().len(),
        1,
        "提密前可见"
    );
    sqlx::query("UPDATE cm_object SET secret_level = 2 WHERE obj_no = 'DWG-SRCH-4'")
        .execute(&ctx.pool)
        .await
        .unwrap();
    let (s, body) = ctx.get_authed("/api/v1/search?q=普通垫片", &low).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v["groups"][0]["items"].as_array().unwrap().len(),
        0,
        "带外提密后 DB 复核立即剔除（不等索引自愈）"
    );
    let (s, body) = ctx.get_authed("/api/v1/search?q=普通垫片", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v["groups"][0]["items"].as_array().unwrap().len(),
        1,
        "admin 上限内不受复核影响"
    );

    // ⑤ 钻取分页 + 非法 type
    let (s, body) = ctx
        .get_authed("/api/v1/search?q=轴承&type=drawing&page=1&size=1", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["groups"][0]["total"], 2);
    assert_eq!(v["groups"][0]["items"].as_array().unwrap().len(), 1);
    assert_eq!(v["page"], 1);
    let (s, _) = ctx
        .get_authed("/api/v1/search?q=x&type=bogus", &admin)
        .await;
    assert_eq!(s, 400);

    // ⑥ 空 q 400 / 无 token 401
    let (s, _) = ctx.get_authed("/api/v1/search?q=%20%20", &admin).await;
    assert_eq!(s, 400);
    let r = axum::http::Request::builder()
        .method("GET")
        .uri("/api/v1/search?q=轴承")
        .body(axum::body::Body::empty())
        .unwrap();
    let (s, _, _) = send(&ctx.app, r).await;
    assert_eq!(s, 401);

    ctx.cleanup().await;
}

// ==================== 增量21 BOM-01 富行 + BOM-07 汇总 ====================

#[tokio::test]
async fn bom_lines_info_summary_csv_and_abac() {
    let ctx = TestCtx::setup("boms21", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "b21ed", "b21ed-pass-123", &["editor"], 0)
        .await;
    ctx.create_user_as_admin(&admin, "b21low", "b21low-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("b21ed", "b21ed-pass-123").await;
    let low = ctx.login_as("b21low", "b21low-pass-123").await;

    let upload = |t: &str, no: &str| {
        send(
            &ctx.app,
            post_docs_multipart(Some(t), no, r#"{"name":"N"}"#, Some("b.dwg"), FILE.to_vec()),
        )
    };
    let part_create = |t: &str, class: &str, name: &str| {
        send(
            &ctx.app,
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/v1/parts")
                .header("authorization", format!("Bearer {t}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
                    r#"{{"class_code":"{class}","attrs":{{"name":"{name}","material":"Q235"}}}}"#
                )))
                .unwrap(),
        )
    };

    // 父件（图纸）+ 子件：自制件/外购件物料 + 图纸
    let (s, _, _) = upload(&ed, "DWG-B21-P").await;
    assert_eq!(s, 201);
    let (s, body, _) = part_create(&ed, "CLS-ZZ", "支架锻件").await;
    assert_eq!(s, 201, "body={}", String::from_utf8_lossy(&body));
    let zz_no: String = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["obj_no"]
        .as_str()
        .unwrap()
        .to_string();
    let (s, body, _) = part_create(&ed, "CLS-WG", "标准螺栓").await;
    assert_eq!(s, 201);
    let wg_no: String = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["obj_no"]
        .as_str()
        .unwrap()
        .to_string();
    let (s, _, _) = upload(&ed, "DWG-B21-C").await;
    assert_eq!(s, 201);

    // 子件 uuid（pool 直查；set_lines/前端编辑器提交均用 id）
    let id_of = |no: String| {
        let pool = ctx.pool.clone();
        async move {
            sqlx::query_scalar(
                "SELECT id::text FROM cm_object WHERE obj_no = $1 AND deleted_at IS NULL \
                 ORDER BY major_ver DESC LIMIT 1",
            )
            .bind(no)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    let zz_id: String = id_of(zz_no.clone()).await;
    let wg_id: String = id_of(wg_no.clone()).await;
    let dwg_id: String = id_of("DWG-B21-C".into()).await;

    let set_lines = |t: &str, no: &str, body: String| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/boms/{no}/lines"))
            .header("authorization", format!("Bearer {t}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap()
    };

    // 结构：父→{自制×2, 外购×3, 图纸C×1}；图纸 C 作子装配下挂外购×5（多路径：3+5×1=8）。
    // BOM 路由父件限 drawing（docs detail 门控）——part 作父件（部件级装配）后置（台账记）
    let (s, _, _) = send(
        &ctx.app,
        set_lines(
            &ed,
            "DWG-B21-P",
            format!(
                r#"[{{"line_no":1,"child":"{zz_id}","qty":2}},{{"line_no":2,"child":"{wg_id}","qty":3}},{{"line_no":3,"child":"{dwg_id}","qty":1}}]"#
            ),
        ),
    )
    .await;
    assert_eq!(s, 204, "父件结构写入");
    let (s, _, _) = send(
        &ctx.app,
        set_lines(
            &ed,
            "DWG-B21-C",
            format!(r#"[{{"line_no":1,"child":"{wg_id}","qty":5}}]"#),
        ),
    )
    .await;
    assert_eq!(s, 204, "子装配结构写入（drawing 父件）");

    // ① 富行：三行齐（line_no 序/obj_no/name/qty）
    let (s, body) = ctx.get_authed("/api/v1/boms/DWG-B21-P/lines", &admin).await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let lines = v.as_array().unwrap();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["child_obj_no"], zz_no);
    assert_eq!(lines[0]["child_name"], "支架锻件");
    assert_eq!(lines[0]["qty"], 2);
    assert_eq!(lines[1]["child_obj_no"], wg_no);
    assert_eq!(lines[2]["child_obj_no"], "DWG-B21-C");
    assert_eq!(lines[2]["qty"], 1);

    // ② 汇总：多路径用量（外购=直挂3 + 经图纸C 5×1=8）
    let (s, body) = ctx
        .get_authed("/api/v1/boms/DWG-B21-P/summary", &admin)
        .await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = v.as_array().unwrap();
    // C 有子边→非叶；叶=ZZ/WG → 汇总恰 2 行
    assert_eq!(rows.len(), 2, "叶=ZZ(自制)/WG(外购)");
    let find = |no: &str| -> serde_json::Value {
        rows.iter()
            .find(|r| r["obj_no"] == no)
            .unwrap_or_else(|| panic!("缺 {no}"))
            .clone()
    };
    assert_eq!(find(&zz_no)["qty_total"], 2, "自制件（叶）");
    assert_eq!(find(&zz_no)["class_name"], "自制件");
    assert_eq!(
        find(&wg_no)["qty_total"],
        8,
        "外购=直挂3+经图纸C 5×1（多路径和）"
    );
    assert_eq!(find(&wg_no)["class_code"], "CLS-WG");
    assert!(
        !rows.iter().any(|r| r["obj_no"] == "DWG-B21-C"),
        "非叶子件不入汇总"
    );

    // ③ ABAC：外购叶件提密 2 → 低上限用户富行/汇总均不见（且不泄露存在性）
    sqlx::query("UPDATE cm_object SET secret_level = 2 WHERE obj_no = $1")
        .bind(&wg_no)
        .execute(&ctx.pool)
        .await
        .unwrap();
    let (s, body) = ctx.get_authed("/api/v1/boms/DWG-B21-P/lines", &low).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2, "低上限富行过滤越密级行");
    let (s, body) = ctx.get_authed("/api/v1/boms/DWG-B21-P/summary", &low).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v.as_array().unwrap().len(),
        1,
        "汇总仅剩 ZZ（越密级叶剔除）"
    );

    // ④ CSV：BOM 头 + 行齐全
    let (s, body) = ctx
        .get_authed("/api/v1/boms/DWG-B21-P/summary?format=csv", &admin)
        .await;
    assert_eq!(s, 200);
    let text = String::from_utf8_lossy(&body);
    assert!(text.starts_with('\u{FEFF}'), "UTF-8 BOM 前缀");
    assert!(text.contains("编号,名称,分类码,分类,单台用量"));
    assert!(text.contains(&zz_no));
    assert!(text.contains("自制件"));

    // ⑤ as-of：发布 → revise → major=1 快照富行/汇总（live 已改不回溯）
    let approve = |t: &str, no: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/docs/{no}/approve"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let sub = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/docs/DWG-B21-P/submit")
        .header("authorization", format!("Bearer {admin}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let (s, _, _) = send(&ctx.app, sub).await;
    assert_eq!(s, 200, "提交评审");
    let (s, _, _) = send(&ctx.app, approve(&admin, "DWG-B21-P")).await;
    assert_eq!(s, 200, "父件发布（快照冻结）");
    let revise = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/docs/DWG-B21-P/revise")
        .header("authorization", format!("Bearer {admin}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let (s, _, _) = send(&ctx.app, revise).await;
    assert_eq!(s, 200, "升版 v2 working");
    // live 清空（v2 无结构）→ major=1 仍有快照
    let (s, _, _) = send(&ctx.app, set_lines(&admin, "DWG-B21-P", "[]".into())).await;
    assert_eq!(s, 204, "v2 清空 live 行");
    let (s, body) = ctx
        .get_authed("/api/v1/boms/DWG-B21-P/lines?major=1", &admin)
        .await;
    assert_eq!(s, 200, "as-of 富行");
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 3, "快照行仍在");
    let (s, body) = ctx
        .get_authed("/api/v1/boms/DWG-B21-P/summary?major=1", &admin)
        .await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v.as_array().unwrap().len(),
        2,
        "as-of 汇总叶=ZZ/WG（C 非叶）"
    );
    let (s, _) = ctx
        .get_authed("/api/v1/boms/DWG-B21-P/summary?major=9", &admin)
        .await;
    assert_eq!(s, 400, "无快照 major → 400");

    ctx.cleanup().await;
}

// ==================== 增量22 DOC-05 在线预览 ====================

#[tokio::test]
async fn doc_preview_inline_whitelist_and_abac() {
    let ctx = TestCtx::setup("prev", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "pved", "pved-pass-123", &["editor"], 0)
        .await;
    ctx.create_user_as_admin(&admin, "pvlow", "pvlow-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("pved", "pved-pass-123").await;
    let low = ctx.login_as("pvlow", "pvlow-pass-123").await;

    let pdf_body = b"%PDF-1.4 fake-preview-content".to_vec();
    let png_body = b"\x89PNG\r\n\x1a\nfakepng".to_vec();
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-PRE-PDF",
            r#"{"name":"说明书"}"#,
            Some("a.pdf"),
            pdf_body.clone(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-PRE-PNG",
            r#"{"name":"示意图"}"#,
            Some("b.png"),
            png_body.clone(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-PRE-DWG",
            r#"{"name":"图纸"}"#,
            Some("c.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);

    // ① PDF → 200 inline + application/pdf + nosniff（send 不回 headers——oneshot 直取）
    use tower::ServiceExt as _;
    let req = |t: &str, no: &str| {
        axum::http::Request::builder()
            .method("GET")
            .uri(format!("/api/v1/docs/{no}/preview"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let res = ctx
        .app
        .clone()
        .oneshot(req(&admin, "DWG-PRE-PDF"))
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "pdf preview");
    let headers = res.headers().clone();
    let body = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(
        headers.get("content-type").and_then(|v| v.to_str().ok()),
        Some("application/pdf")
    );
    let cd = headers
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(cd.starts_with("inline"), "inline 直开: {cd}");
    assert_eq!(
        headers
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(body.to_vec(), pdf_body, "内容一致（流式）");

    // ② PNG → 200 image/png
    let res = ctx
        .app
        .clone()
        .oneshot(req(&admin, "DWG-PRE-PNG"))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let ct = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert_eq!(ct, "image/png");

    // ③ DWG → 415 预览不支持（前端提示走下载）
    let (s, _, _) = send(&ctx.app, req(&admin, "DWG-PRE-DWG")).await;
    assert_eq!(s, 415);
    // 终审 M2 补钉：SVG 双路径均 415（mime=image/svg+xml 直传；octet-stream+.svg 扩展名兜底）
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-PRE-SVG1",
            r#"{"name":"矢量"}"#,
            Some("s.svg"),
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    // multipart content-type 由文件扩展名推断不可控——直接 SQL 置 image/svg+xml 钉 mime 路径
    sqlx::query(
        "UPDATE file_object SET mime = 'image/svg+xml' WHERE hash =          (SELECT fr.hash FROM file_ref fr JOIN cm_object o ON o.id = fr.object_id WHERE o.obj_no = 'DWG-PRE-SVG1')",
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    let (s, _, _) = send(&ctx.app, req(&admin, "DWG-PRE-SVG1")).await;
    assert_eq!(s, 415, "SVG mime 路径排除");
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-PRE-SVG2",
            r#"{"name":"矢量2"}"#,
            Some("t.svg"),
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    sqlx::query(
        "UPDATE file_object SET mime = NULL WHERE hash =          (SELECT fr.hash FROM file_ref fr JOIN cm_object o ON o.id = fr.object_id WHERE o.obj_no = 'DWG-PRE-SVG2')",
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    let (s, _, _) = send(&ctx.app, req(&admin, "DWG-PRE-SVG2")).await;
    assert_eq!(s, 415, "SVG 扩展名兜底路径排除");

    // ④ ABAC：提密 → 低上限用户 404 不泄露存在性
    sqlx::query("UPDATE cm_object SET secret_level = 2 WHERE obj_no = 'DWG-PRE-PDF'")
        .execute(&ctx.pool)
        .await
        .unwrap();
    let (s, _, _) = send(&ctx.app, req(&low, "DWG-PRE-PDF")).await;
    assert_eq!(s, 404);
    let (s, _, _) = send(&ctx.app, req(&admin, "DWG-PRE-PDF")).await;
    assert_eq!(s, 200, "admin 不受影响");

    // 终审 M2 补钉：无附件对象 → 404（detail 200 但 file_ref 缺失）
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-PRE-NOFILE",
            r#"{"name":"空壳"}"#,
            Some("n.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    sqlx::query(
        "DELETE FROM file_ref WHERE object_id =          (SELECT id FROM cm_object WHERE obj_no = 'DWG-PRE-NOFILE' AND deleted_at IS NULL)",
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    let (s, _, _) = send(&ctx.app, req(&admin, "DWG-PRE-NOFILE")).await;
    assert_eq!(s, 404, "无附件 preview 404");

    ctx.cleanup().await;
}

// ==================== 增量23 防爆破锁定 + SYS-04 参数 ====================

#[tokio::test]
async fn bruteforce_lockout_and_password_policy() {
    let ctx = TestCtx::setup("bf", 1024 * 1024).await;
    let admin = ctx.login().await;
    // 密码 8 位满足缺省策略 min_length=8
    ctx.create_user_as_admin(&admin, "bfed", "bf-pass-123", &["editor"], 0)
        .await;

    let try_login = |pw: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(format!(
                r#"{{"username":"bfed","password":"{pw}"}}"#
            )))
            .unwrap()
    };
    let code_of = |b: &bytes::Bytes| -> String {
        serde_json::from_slice::<serde_json::Value>(b)
            .ok()
            .and_then(|v| v["code"].as_str().map(String::from))
            .unwrap_or_default()
    };

    // ① 密码策略：建 7 位密码用户 → 400 MDL-SEC-PASSWORD-POLICY
    let short = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/sec/users")
        .header("authorization", format!("Bearer {admin}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            r#"{"username":"shortpw","password":"a7chars","roles":["editor"]}"#,
        ))
        .unwrap();
    let (s, body, _) = send(&ctx.app, short).await;
    assert_eq!(s, 400, "7 位密码须拒");
    assert_eq!(code_of(&body), "MDL-SEC-PASSWORD-POLICY");

    // ② 连败 4 次（阈值 5-1）→ 仍 401
    for _ in 0..4 {
        let (s, _, _) = send(&ctx.app, try_login("wrong-pass")).await;
        assert_eq!(s, 401);
    }
    // 第 5 次 → 触发锁定；本次响应仍为 401（计数在验证失败路径累加）
    let (s, _, _) = send(&ctx.app, try_login("wrong-pass")).await;
    assert_eq!(s, 401);
    // 第 6 次（锁定期内）→ 423 MDL-SEC-ACCOUNT-LOCKED（正确密码也拒）
    let (s, body, _) = send(&ctx.app, try_login("bf-pass-123")).await;
    assert_eq!(s, 423, "锁定期内正确密码也拒");
    assert_eq!(code_of(&body), "MDL-SEC-ACCOUNT-LOCKED");
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let mins = v["message"].as_str().unwrap_or("");
    assert!(mins.contains("分钟"), "提示含剩余分钟: {mins}");

    // ③ 锁定参数热生效验证：SQL 模拟到期解锁（locked_until 过去）→ 正确密码成功 + 清零
    sqlx::query(
        "UPDATE sec_user SET locked_until = now() - interval '1 min' WHERE username = 'bfed'",
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    let (s, _, _) = send(&ctx.app, try_login("bf-pass-123")).await;
    assert_eq!(s, 200, "到期解锁后正确密码成功");
    let n: i32 = sqlx::query_scalar("SELECT failed_attempts FROM sec_user WHERE username = 'bfed'")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "成功登录清零");

    // ④ 改阈值参数热生效：threshold=2 → 两轮失败即锁
    sqlx::query("UPDATE sys_param SET value = '2' WHERE key = 'lock_threshold'")
        .execute(&ctx.pool)
        .await
        .unwrap();
    let (s, _, _) = send(&ctx.app, try_login("wrong-1")).await;
    assert_eq!(s, 401);
    let (s, _, _) = send(&ctx.app, try_login("wrong-2")).await;
    assert_eq!(s, 401);
    let (s, body, _) = send(&ctx.app, try_login("bf-pass-123")).await;
    assert_eq!(s, 423, "阈值 2 生效——两败即锁");
    assert_eq!(code_of(&body), "MDL-SEC-ACCOUNT-LOCKED");

    // ⑤ 未知用户不触发锁定语义（无锁定行 → 恒 401）
    let unknown = |pw: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(format!(
                r#"{{"username":"ghost-user","password":"{pw}"}}"#
            )))
            .unwrap()
    };
    for _ in 0..3 {
        let (s, _, _) = send(&ctx.app, unknown("x")).await;
        assert_eq!(s, 401, "未知用户恒 401");
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn sec_params_manage_and_audit_trail() {
    let ctx = TestCtx::setup("sp", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "spview", "spview-pass-123", &["viewer"], 0)
        .await;
    let view = ctx.login_as("spview", "spview-pass-123").await;

    // ① GET：三种子参数齐
    let (s, body) = ctx.get_authed("/api/v1/sec/params", &admin).await;
    assert_eq!(s, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // handler 返回 Vec<SysParamDto> 直接序列化为顶层数组
    let arr = v.as_array().expect("顶层数组");
    assert_eq!(arr.len(), 3, "三种子参数");
    // 系统内部键过滤（增量24 checklist 借 sys_param 持久化 impl_checklist_done——不混入面板）
    sqlx::query("INSERT INTO sys_param(key, value) VALUES ('impl_checklist_done', '[\"training\"]') ON CONFLICT (key) DO NOTHING")
        .execute(&ctx.pool)
        .await
        .unwrap();
    let (_, body) = ctx.get_authed("/api/v1/sec/params", &admin).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 3, "内部键不混入参数面板");
    for k in ["password_min_length", "lock_threshold", "lock_minutes"] {
        assert!(arr.iter().any(|r| r["key"] == k), "含 {k}");
    }
    let min_row = arr
        .iter()
        .find(|r| r["key"] == "password_min_length")
        .unwrap();
    assert_eq!(min_row["value"], "8");

    // ② viewer 403
    let (s, _) = ctx.get_authed("/api/v1/sec/params", &view).await;
    assert_eq!(s, 403);

    // ③ PUT 改 min_length=12 → 生效（建 8 位密码用户 400）
    let put = |key: &str, val: i64| {
        axum::http::Request::builder()
            .method("PUT")
            .uri(format!("/api/v1/sec/params/{key}"))
            .header("authorization", format!("Bearer {admin}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(format!(r#"{{"value":{val}}}"#)))
            .unwrap()
    };
    let (s, _, _) = send(&ctx.app, put("password_min_length", 12)).await;
    assert_eq!(s, 204);
    // 8 位密码（此前合法）现 400；12 位成功
    let mk = |pw: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/sec/users")
            .header("authorization", format!("Bearer {admin}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(format!(
                r#"{{"username":"{pw}u","password":"{pw}","roles":["editor"]}}"#
            )))
            .unwrap()
    };
    let (s, body, _) = send(&ctx.app, mk("eight8pw!")).await;
    assert_eq!(s, 400, "8 位低于新阈值");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["code"],
        "MDL-SEC-PASSWORD-POLICY"
    );
    let (s, _, _) = send(&ctx.app, mk("twelve12pw!!")).await;
    assert_eq!(s, 201, "12 位满足新阈值");

    // ④ 值域校验：越界 400 / 未知键 400 / 非法 tier 类值
    let (s, _, _) = send(&ctx.app, put("password_min_length", 7)).await;
    assert_eq!(s, 400, "低于下界 8");
    let (s, _, _) = send(&ctx.app, put("password_min_length", 65)).await;
    assert_eq!(s, 400, "高于上界 64");
    let (s, _, _) = send(&ctx.app, put("bogus_key", 1)).await;
    assert_eq!(s, 400, "未知键");

    // ⑤ audit 留痕：sec-param.change from=8 to=12
    let mut ok = false;
    for _ in 0..20 {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_log WHERE action='sec-param.change' AND object='password_min_length'",
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
        if n == 1 {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(ok, "sec-param.change 留痕须落库");
    let row: serde_json::Value =
        sqlx::query("SELECT detail FROM audit_log WHERE action='sec-param.change' AND object='password_min_length'")
            .fetch_one(&ctx.pool)
            .await
            .unwrap()
            .get("detail");
    assert_eq!(row["from"], "8");
    assert_eq!(row["to"], "12");

    // ⑥ 还原（避免影响同库其他断言；cleanup 后库删无谓但语义完整）
    let (s, _, _) = send(&ctx.app, put("password_min_length", 8)).await;
    assert_eq!(s, 204);

    ctx.cleanup().await;
}

// ==================== 增量24 AGL-05 模板包 + AGL-09 实施向导 ====================

#[tokio::test]
async fn template_export_starter_import_roundtrip() {
    let ctx = TestCtx::setup("tpl", 1024 * 1024).await;
    let admin = ctx.login().await;
    ctx.create_user_as_admin(&admin, "tplview", "tplview-pass-123", &["viewer"], 0)
        .await;
    let view = ctx.login_as("tplview", "tplview-pass-123").await;

    let post = |t: &str, uri: &str, body: String| {
        axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("authorization", format!("Bearer {t}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap()
    };

    // ① 内置 Starter 包可取（随二进制分发）且 format/schema 自洽
    let (s, body) = ctx
        .get_authed("/api/v1/meta/template/starter", &admin)
        .await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let starter: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(starter["format"], "rustplm-template");
    assert_eq!(starter["version"], 1);
    assert!(starter["types"].as_array().unwrap().len() >= 2);
    assert!(starter["classes"].as_array().unwrap().len() >= 2);

    // ② 导出：含种子类型/属性/分类（ABAC 无关——sec:manage 全量配置面）
    let (s, body) = ctx.get_authed("/api/v1/meta/template/export", &admin).await;
    assert_eq!(s, 200);
    let exported: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(exported["format"], "rustplm-template");
    let ex_types = exported["types"].as_array().unwrap();
    assert!(ex_types.iter().any(|t| t["code"] == "drawing"));
    assert!(ex_types.iter().any(|t| t["code"] == "part"));
    let part_t = ex_types.iter().find(|t| t["code"] == "part").unwrap();
    assert!(part_t["attrs"].as_array().unwrap().len() >= 4);
    let ex_classes = exported["classes"].as_array().unwrap();
    assert!(ex_classes.iter().any(|c| c["code"] == "CLS-ZZ"));

    // ③ viewer 403（sec:manage 门控）
    let (s, _) = ctx.get_authed("/api/v1/meta/template/export", &view).await;
    assert_eq!(s, 403);

    // ④ dry_run：对已有种子导入 starter → 全冲突报告，不落库（无新增类型）
    let dry_body = serde_json::json!({"template": starter, "mode": "dry_run"});
    let (s, body, _) = send(
        &ctx.app,
        post(&admin, "/api/v1/meta/template/import", dry_body.to_string()),
    )
    .await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let dry: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(dry["applied_types"].as_array().unwrap().is_empty());
    let confs = dry["conflicts"].as_array().unwrap();
    assert!(confs
        .iter()
        .any(|c| c["kind"] == "type_exists" && c["key"] == "drawing"));
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM cm_type")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    let _ = before;

    // ⑤ merge 新内容：新类型+属性+新分类 → applied；再 merge 一次 → 全冲突（幂等 skip）
    let mut new_pkg = starter.clone();
    new_pkg["types"] = serde_json::json!([
        {
            "code": "fixture",
            "name": "工装",
            "base_type": "document",
            "config": {"flow_tier": "minimal", "cod_rule": {"prefix": "GZ-", "date_fmt": "yyyyMMdd", "seq_digits": 3}},
            "attrs": [
                {"code": "name", "name": "名称", "data_type": "text", "required": true, "sort_no": 1},
                {"code": "venue", "name": "工位", "data_type": "text", "required": false, "sort_no": 2}
            ]
        }
    ]);
    new_pkg["classes"] = serde_json::json!([
        {"code": "CLS-GZ", "name": "工装夹具", "parent_code": null,
         "cod_rule": {"prefix": "GZ-FL-", "date_fmt": "none", "seq_digits": 4}}
    ]);
    let merge_body = serde_json::json!({"template": new_pkg, "mode": "merge"});
    let (s, body, _) = send(
        &ctx.app,
        post(
            &admin,
            "/api/v1/meta/template/import",
            merge_body.to_string(),
        ),
    )
    .await;
    assert_eq!(s, 200, "body={}", String::from_utf8_lossy(&body));
    let merged: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(merged["applied_types"], serde_json::json!(["fixture"]));
    assert_eq!(merged["applied_classes"], serde_json::json!(["CLS-GZ"]));
    assert!(merged["conflicts"].as_array().unwrap().is_empty());
    // 落库实证：类型+属性+分类+config 全带入
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cm_attr WHERE type_id = (SELECT id FROM cm_type WHERE code='fixture')",
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(n, 2);
    let rule: serde_json::Value =
        sqlx::query_scalar("SELECT config->'cod_rule' FROM cm_type WHERE code = 'fixture'")
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert_eq!(rule["prefix"], "GZ-");
    let (s, body, _) = send(
        &ctx.app,
        post(
            &admin,
            "/api/v1/meta/template/import",
            merge_body.to_string(),
        ),
    )
    .await;
    assert_eq!(s, 200);
    let again: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(again["applied_types"].as_array().unwrap().is_empty());
    assert!(
        again["conflicts"].as_array().unwrap().len() >= 2,
        "重复导入全冲突"
    );

    // ⑥ audit 留痕（merge 才记；dry_run 不记）
    let mut ok = false;
    for _ in 0..20 {
        let n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action='template.import'")
                .fetch_one(&ctx.pool)
                .await
                .unwrap();
        if n >= 1 {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(ok, "template.import 留痕落库（两次 merge ≥1）");

    // ⑦ 坏包 400
    let (s, body, _) = send(
        &ctx.app,
        post(
            &admin,
            "/api/v1/meta/template/import",
            r#"{"template":{"format":"other","version":9,"types":[],"classes":[]},"mode":"merge"}"#
                .into(),
        ),
    )
    .await;
    assert_eq!(s, 400);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["code"], "MDL-BAD-REQUEST");
    let (s, _, _) = send(
        &ctx.app,
        post(
            &admin,
            "/api/v1/meta/template/import",
            r#"{"template":{"format":"rustplm-template","version":1,"types":[],"classes":[]},"mode":"bogus"}"#.into(),
        ),
    )
    .await;
    assert_eq!(s, 400);

    // ⑧ viewer 导入 403
    let (s, _, _) = send(
        &ctx.app,
        post(
            &view,
            "/api/v1/meta/template/import",
            merge_body.to_string(),
        ),
    )
    .await;
    assert_eq!(s, 403);

    ctx.cleanup().await;
}

#[tokio::test]
async fn impl_checklist_auto_manual_and_order() {
    let ctx = TestCtx::setup("ck", 1024 * 1024).await;
    let admin = ctx.login().await;

    // ① 初始：org=1（仅 admin）→ org/perms/data 未 done；coding/types 种子即 done（CLS-ZZ 挂规则/attr=5 种子）
    let (s, body) = ctx.get_authed("/api/v1/meta/impl-checklist", &admin).await;
    assert_eq!(s, 200);
    let steps: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let arr = steps.as_array().unwrap();
    assert_eq!(arr.len(), 8, "八步");
    let get = |k: &str| -> serde_json::Value {
        arr.iter()
            .find(|r| r["key"] == k)
            .unwrap_or_else(|| panic!("缺 {k}"))
            .clone()
    };
    assert_eq!(get("org")["done"], false, "仅 admin 一人");
    assert_eq!(get("perm")["done"], false);
    assert_eq!(get("coding")["done"], true, "种子分类挂规则");
    assert_eq!(get("data")["done"], false);
    assert_eq!(get("training")["done"], false);
    assert_eq!(get("training")["auto"], false, "培训为人工步");
    assert!(get("org")["hint"].as_str().unwrap().len() > 4, "每步含指引");

    // ② manual 步按序约束：跳过 training 直接 cutover → 400
    let complete = |t: &str, step: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/v1/meta/impl-checklist/{step}/complete"))
            .header("authorization", format!("Bearer {t}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, body, _) = send(&ctx.app, complete(&admin, "cutover")).await;
    assert_eq!(s, 400, "前置 training 未完成");
    assert!(String::from_utf8_lossy(&body).contains("training"));

    // ③ auto 步标记 complete → 400（系统自动判定）
    let (s, _, _) = send(&ctx.app, complete(&admin, "org")).await;
    assert_eq!(s, 400);

    // ④ 造数据翻转 auto 步：建用户→org+perm done；上传→data done
    ctx.create_user_as_admin(&admin, "cked", "cked-pass-123", &["editor"], 0)
        .await;
    let ed = ctx.login_as("cked", "cked-pass-123").await;
    let (s, _, _) = send(
        &ctx.app,
        post_docs_multipart(
            Some(&ed),
            "DWG-CK-1",
            r#"{"name":"N"}"#,
            Some("k.dwg"),
            FILE.to_vec(),
        ),
    )
    .await;
    assert_eq!(s, 201);
    // flow 步：显式设档位（种子 config 无 flow_tier——该步语义即"确认审批档位"）
    let ft = axum::http::Request::builder()
        .method("PUT")
        .uri("/api/v1/meta/types/drawing/flow-tier")
        .header("authorization", format!("Bearer {admin}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(r#"{"tier":"standard"}"#))
        .unwrap();
    let (s, _, _) = send(&ctx.app, ft).await;
    assert_eq!(s, 204);
    let (s, body) = ctx.get_authed("/api/v1/meta/impl-checklist", &admin).await;
    assert_eq!(s, 200);
    let steps: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let arr = steps.as_array().unwrap();
    let get =
        |k: &str| -> serde_json::Value { arr.iter().find(|r| r["key"] == k).unwrap().clone() };
    assert_eq!(get("org")["done"], true, "建成员后 org 翻转");
    assert_eq!(get("perm")["done"], true, "授角色后 perm 翻转");
    assert_eq!(get("data")["done"], true, "上传后 data 翻转");
    assert_eq!(get("flow")["done"], true, "显式设档后 flow 翻转");

    // ⑤ manual 链：training → cutover（按序成功；audit 留痕+sys_param 持久化）
    let (s, _, _) = send(&ctx.app, complete(&admin, "training")).await;
    assert_eq!(s, 204);
    let (s, _, _) = send(&ctx.app, complete(&admin, "cutover")).await;
    assert_eq!(s, 204);
    let (_, body) = ctx.get_authed("/api/v1/meta/impl-checklist", &admin).await;
    let steps: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let arr = steps.as_array().unwrap();
    assert_eq!(
        arr.iter().filter(|r| r["done"] == true).count(),
        8,
        "全 done"
    );
    let persisted: String =
        sqlx::query_scalar("SELECT value FROM sys_param WHERE key='impl_checklist_done'")
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert!(persisted.contains("training") && persisted.contains("cutover"));
    let mut ok = false;
    for _ in 0..20 {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_log WHERE action='impl-checklist.complete'",
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
        if n >= 2 {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(ok, "complete 留痕两行");

    // ⑥ 无 token → 401（GET 认证即可）
    let r = axum::http::Request::builder()
        .method("GET")
        .uri("/api/v1/meta/impl-checklist")
        .body(axum::body::Body::empty())
        .unwrap();
    let (s, _, _) = send(&ctx.app, r).await;
    assert_eq!(s, 401);

    // ⑦ viewer 也能 GET（全员可见）；但 complete 需 sec:manage → 403
    ctx.create_user_as_admin(&admin, "ckview", "ckview-pass-123", &["viewer"], 0)
        .await;
    let view = ctx.login_as("ckview", "ckview-pass-123").await;
    let (s, _) = ctx.get_authed("/api/v1/meta/impl-checklist", &view).await;
    assert_eq!(s, 200, "向导全员可见");
    let (s, _, _) = send(&ctx.app, complete(&view, "training")).await;
    assert_eq!(s, 403, "complete 需 sec:manage");

    ctx.cleanup().await;
}

// ==================== 增量25 TQ-02 OpenAPI 契约快照门禁 ====================

/// 结构化契约面：paths→methods、schemas→必填字段集+属性集（破坏面覆盖：
/// 删端点/删 method/删 schema/删属性/删必填；类型变更不在面内——台账裁定）
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct ContractFace {
    paths: std::collections::BTreeMap<String, Vec<String>>,
    schemas: std::collections::BTreeMap<String, ContractSchema>,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug, Default)]
struct ContractSchema {
    required: Vec<String>,
    properties: Vec<String>,
}

fn extract_face(spec: &serde_json::Value) -> ContractFace {
    let mut face = ContractFace {
        paths: Default::default(),
        schemas: Default::default(),
    };
    if let Some(paths) = spec["paths"].as_object() {
        for (p, item) in paths {
            let methods = item
                .as_object()
                .map(|m| {
                    let mut ms: Vec<String> = m
                        .keys()
                        .filter(|k| {
                            matches!(k.as_str(), "get" | "post" | "put" | "delete" | "patch")
                        })
                        .cloned()
                        .collect();
                    ms.sort();
                    ms
                })
                .unwrap_or_default();
            face.paths.insert(p.clone(), methods);
        }
    }
    if let Some(comps) = spec["components"]["schemas"].as_object() {
        for (name, sch) in comps {
            let required = sch["required"]
                .as_array()
                .map(|a| {
                    let mut v: Vec<String> = a
                        .iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect();
                    v.sort();
                    v
                })
                .unwrap_or_default();
            let mut properties: Vec<String> = sch["properties"]
                .as_object()
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default();
            properties.sort();
            face.schemas.insert(
                name.clone(),
                ContractSchema {
                    required,
                    properties,
                },
            );
        }
    }
    face
}

const SNAPSHOT_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/openapi-snapshot.json");

#[tokio::test]
async fn openapi_snapshot_gate() {
    let ctx = TestCtx::setup("oas", 1024 * 1024).await;
    let r = axum::http::Request::builder()
        .method("GET")
        .uri("/api/docs/openapi.json")
        .body(axum::body::Body::empty())
        .unwrap();
    let (s, body, _) = send(&ctx.app, r).await;
    assert_eq!(s, 200);
    let spec: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let face = extract_face(&spec);

    // 更新模式：重写基线（开发者有意变更契约时一键刷新；diff 走 git review）
    if std::env::var("UPDATE_OPENAPI_SNAPSHOT").as_deref() == Ok("1") {
        std::fs::write(
            SNAPSHOT_PATH,
            serde_json::to_string_pretty(&face).unwrap() + "\n",
        )
        .unwrap();
        eprintln!("openapi-snapshot.json 已更新（git diff 审阅契约变更）");
        ctx.cleanup().await;
        return;
    }

    let baseline_raw = std::fs::read_to_string(SNAPSHOT_PATH)
        .expect("契约基线缺失——跑 UPDATE_OPENAPI_SNAPSHOT=1 cargo test -p plm-server --test e2e openapi_snapshot 生成");
    let baseline: ContractFace = serde_json::from_str(&baseline_raw).unwrap();

    let mut breaking: Vec<String> = Vec::new();
    let mut stale: Vec<String> = Vec::new();

    // paths 面
    for (p, methods) in &baseline.paths {
        match face.paths.get(p) {
            None => breaking.push(format!("端点被删除: {p}")),
            Some(m) => {
                for meth in methods {
                    if !m.contains(meth) {
                        breaking.push(format!("端点 {p} 失去方法 {meth}"));
                    }
                }
                for meth in m {
                    if !methods.contains(meth) {
                        stale.push(format!("端点 {p} 新增方法 {meth}"));
                    }
                }
            }
        }
    }
    for p in face.paths.keys() {
        if !baseline.paths.contains_key(p) {
            stale.push(format!("新增端点: {p}"));
        }
    }
    // schemas 面
    for (name, bs) in &baseline.schemas {
        match face.schemas.get(name) {
            None => breaking.push(format!("schema 被删除: {name}")),
            Some(fs) => {
                for f in &bs.required {
                    if !fs.required.contains(f) {
                        breaking.push(format!("schema {name} 必填字段被移除: {f}"));
                    }
                }
                for f in &bs.properties {
                    if !fs.properties.contains(f) {
                        breaking.push(format!("schema {name} 属性被移除: {f}"));
                    }
                }
                for f in &fs.required {
                    if !bs.required.contains(f) {
                        stale.push(format!("schema {name} 新增必填: {f}"));
                    }
                }
                for f in &fs.properties {
                    if !bs.properties.contains(f) {
                        stale.push(format!("schema {name} 新增属性: {f}"));
                    }
                }
            }
        }
    }
    for name in face.schemas.keys() {
        if !baseline.schemas.contains_key(name) {
            stale.push(format!("新增 schema: {name}"));
        }
    }

    if !breaking.is_empty() || !stale.is_empty() {
        panic!(
            "OpenAPI 契约快照不匹配。\n破坏性变更（对外承诺断裂，禁止直接合并）:\n{}\n快照过期（向后兼容新增，确认后更新基线）:\n{}\n—— 有意变更请运行 UPDATE_OPENAPI_SNAPSHOT=1 cargo test -p plm-server --test e2e openapi_snapshot 刷新基线并 git diff 审阅",
            breaking.join("\n"),
            stale.join("\n")
        );
    }

    ctx.cleanup().await;
}

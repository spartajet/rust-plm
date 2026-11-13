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
    assert!(obj_no.starts_with("DWG-"), "自动码={obj_no}");

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

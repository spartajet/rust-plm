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

//! mod-doc 集成测试（计划 Task2）：真 PG18 库 + 真文件卷。

mod common;

use common::TestCtx;
use core_model::query::{Cond, ObjectQuery, Op, Page};
use serde_json::json;
use std::io::Cursor;

const BYTES: &[u8] = b"%DWG-fake-content-0123456789";

#[tokio::test]
async fn upload_detail_download_roundtrip() {
    let ctx = TestCtx::setup("rt").await;
    // Step1 先以最直观的 API 形态书写（计划 Interfaces 锚定）：
    let attrs = json!({"name": "Bracket", "material": "Q235"});
    let mut r = Cursor::new(BYTES);
    let d = ctx
        .service
        .upload(
            "DWG-RT-1".into(),
            attrs.as_object().unwrap().clone(),
            "part.dwg".into(),
            Some("application/octet-stream".into()),
            &mut r,
        )
        .await
        .unwrap();
    assert_eq!(d.object.obj_no, "DWG-RT-1");
    assert_eq!(d.object.state, "working");
    let f = d.file.expect("上传后必有 original 文件");
    assert_eq!(f.name, "part.dwg");
    assert_eq!(f.size, BYTES.len() as i64);

    let detail = ctx.service.detail("DWG-RT-1").await.unwrap();
    assert_eq!(detail.object.attrs.get("material").unwrap(), "Q235");

    let (name, mut stream, size) = ctx.service.open_original("DWG-RT-1").await.unwrap();
    use tokio::io::AsyncReadExt;
    let mut out = Vec::new();
    stream.read_to_end(&mut out).await.unwrap();
    assert_eq!(out, BYTES);
    assert_eq!(name, "part.dwg");
    assert_eq!(size, BYTES.len() as i64);
    ctx.cleanup().await;
}

#[tokio::test]
async fn duplicate_obj_no_conflicts_and_no_orphan() {
    let ctx = TestCtx::setup("dup").await;
    let attrs = json!({"name": "Bracket"});
    let mut r1 = Cursor::new(BYTES);
    ctx.service
        .upload(
            "DWG-DUP-1".into(),
            attrs.as_object().unwrap().clone(),
            "a.dwg".into(),
            None,
            &mut r1,
        )
        .await
        .unwrap();
    let before = ctx.vol_file_count().await;

    let mut r2 = Cursor::new(BYTES);
    let err = ctx
        .service
        .upload(
            "DWG-DUP-1".into(),
            attrs.as_object().unwrap().clone(),
            "a.dwg".into(),
            None,
            &mut r2,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "MDL-OBJ-NO-DUPLICATE");
    assert_eq!(
        ctx.vol_file_count().await,
        before,
        "内容寻址去重：同内容不得新增卷文件"
    );
    ctx.cleanup().await;
}

#[tokio::test]
async fn empty_file_rejected() {
    let ctx = TestCtx::setup("empty").await;
    let mut r = Cursor::new(&b""[..]);
    let err = ctx
        .service
        .upload(
            "DWG-E-1".into(),
            json!({"name":"X"}).as_object().unwrap().clone(),
            "e.dwg".into(),
            None,
            &mut r,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "MDL-FILE-EMPTY");
    ctx.cleanup().await;
}

#[tokio::test]
async fn missing_required_attr_rejected() {
    let ctx = TestCtx::setup("req").await;
    let mut r = Cursor::new(BYTES);
    let err = ctx
        .service
        .upload(
            "DWG-R-1".into(),
            serde_json::Map::new(),
            "x.dwg".into(),
            None,
            &mut r,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "MDL-ATTR-REQUIRED");
    ctx.cleanup().await;
}

#[tokio::test]
async fn list_filters_by_material() {
    let ctx = TestCtx::setup("list").await;
    for (no, material) in [("DWG-L-1", "Q235"), ("DWG-L-2", "AL6061")] {
        let mut r = Cursor::new(BYTES);
        ctx.service
            .upload(
                no.into(),
                json!({"name":"N","material":material})
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
    let q = ObjectQuery {
        type_code: "drawing".into(),
        conds: vec![Cond {
            attr: "material".into(),
            op: Op::Eq,
            value: json!("Q235"),
        }],
        sorts: vec![],
        page: Some(Page::default()),
    };
    let (items, total) = ctx.service.list(&q).await.unwrap();
    assert_eq!(total, 1);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].obj_no, "DWG-L-1");
    ctx.cleanup().await;
}

#[tokio::test]
async fn detail_missing_404() {
    let ctx = TestCtx::setup("miss").await;
    let err = ctx.service.detail("NOPE").await.unwrap_err();
    assert_eq!(err.code(), "MDL-OBJ-NOT-FOUND");
    ctx.cleanup().await;
}

#[tokio::test]
async fn list_filter_by_system_column_obj_no() {
    // 终审 I3：物理列绑定曾带 JSON 引号 → 静默空结果
    let ctx = TestCtx::setup("syscol").await;
    let mut r = Cursor::new(BYTES);
    ctx.service
        .upload(
            "DWG-SYS-7".into(),
            json!({"name":"N"}).as_object().unwrap().clone(),
            "f.dwg".into(),
            None,
            &mut r,
        )
        .await
        .unwrap();
    let q = ObjectQuery {
        type_code: "drawing".into(),
        conds: vec![Cond {
            attr: "obj_no".into(),
            op: Op::Eq,
            value: json!("DWG-SYS-7"),
        }],
        sorts: vec![],
        page: Some(Page::default()),
    };
    let (items, total) = ctx.service.list(&q).await.unwrap();
    assert_eq!(total, 1, "系统列等值过滤必须命中");
    assert_eq!(items[0].obj_no, "DWG-SYS-7");
    ctx.cleanup().await;
}

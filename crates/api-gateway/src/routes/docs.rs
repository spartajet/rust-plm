//! /api/v1/docs 路由：multipart 上传 / 列表 / 详情 / 下载（骨架期无鉴权，plan 已记录债务）。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use axum::Json;
use core_model::query::{Cond, ObjectQuery, Op, Page, Sort};
use core_model::types::AttrMap;
use futures_util::StreamExt;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io;
use tokio_util::io::{ReaderStream, StreamReader};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct FileInfoDto {
    pub name: String,
    pub hash: String,
    pub size: i64,
    pub mime: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct DocResponse {
    /// BizObject 序列化（系统列+attrs）
    pub object: Value,
    pub file: Option<FileInfoDto>,
}

#[derive(Serialize, ToSchema)]
pub struct ListResponse {
    pub items: Vec<Value>,
    pub total: u64,
    pub page: u32,
    pub size: u32,
}

/// POST /api/v1/docs — multipart: obj_no + attrs(JSON) + file(单文件)
#[utoipa::path(
    post, path = "/api/v1/docs",
    responses((status = 201, body = DocResponse), (status = 400, description = "字段/校验错误"), (status = 409))
)]
pub async fn upload_doc(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    mut mp: Multipart,
) -> Result<(StatusCode, Json<DocResponse>), ApiError> {
    let mut obj_no: Option<String> = None;
    let mut attrs: Option<AttrMap> = None;
    // staged: (name, mime, hash, size) —— file 字段在循环内即时流式落盘（Field 不能跨迭代持有）
    let mut staged: Option<(Option<String>, Option<String>, String, i64)> = None;
    let map_mp_err =
        |e: axum::extract::multipart::MultipartError| core_model::DomainError::File(e.to_string());

    while let Some(field) = mp
        .next_field()
        .await
        .map_err(|e| ApiError::new(map_mp_err(e), &tid.0))?
    {
        match field.name().unwrap_or("") {
            "obj_no" => {
                obj_no = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::new(map_mp_err(e), &tid.0))?,
                );
            }
            "attrs" => {
                let s = field
                    .text()
                    .await
                    .map_err(|e| ApiError::new(map_mp_err(e), &tid.0))?;
                if !s.trim().is_empty() {
                    let v: Value = serde_json::from_str(&s).map_err(|e| {
                        ApiError::new(
                            core_model::DomainError::AttrTypeMismatch(
                                "attrs".into(),
                                format!("json-object: {e}"),
                            ),
                            &tid.0,
                        )
                    })?;
                    let m = v.as_object().cloned().ok_or_else(|| {
                        ApiError::new(
                            core_model::DomainError::AttrTypeMismatch(
                                "attrs".into(),
                                "json-object".into(),
                            ),
                            &tid.0,
                        )
                    })?;
                    attrs = Some(m);
                }
            }
            "file" => {
                if staged.is_some() {
                    return Err(ApiError::new(core_model::DomainError::FileMulti, &tid.0));
                }
                let name = field.file_name().map(str::to_owned);
                let mime = field.content_type().map(str::to_owned);
                let stream = field.map(|r| r.map_err(|e| io::Error::other(e.to_string())));
                let mut reader = Box::pin(StreamReader::new(stream));
                let (hash, size) = st
                    .service
                    .stage_file(&mut reader)
                    .await
                    .map_err(|e| ApiError::new(e, &tid.0))?;
                staged = Some((name, mime, hash, size));
            }
            _ => { /* 未知字段忽略 */ }
        }
    }

    let obj_no = obj_no.filter(|s| !s.trim().is_empty()).ok_or_else(|| {
        ApiError::new(
            core_model::DomainError::FieldRequired("obj_no".into()),
            &tid.0,
        )
    })?;
    let (file_name, mime, hash, size) = staged.ok_or_else(|| {
        ApiError::new(
            core_model::DomainError::FieldRequired("file".into()),
            &tid.0,
        )
    })?;

    let detail = st
        .service
        .upload_staged(
            obj_no,
            attrs.unwrap_or_default(),
            file_name.unwrap_or_default(),
            mime,
            hash,
            size,
        )
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;

    Ok((StatusCode::CREATED, Json(to_response(detail))))
}

#[derive(Deserialize)]
pub struct ListQ {
    #[serde(flatten)]
    raw: HashMap<String, String>,
    pub page: Option<u32>,
    pub size: Option<u32>,
    pub sort: Option<String>,
}

/// GET /api/v1/docs?filter[attr]=v&sort=-obj_no&page=1&size=20
#[utoipa::path(
    get, path = "/api/v1/docs",
    params(("filter[attr]" = String, Query, description = "属性等值过滤（走 GIN/提升列）"), ("sort" = String, Query, description = "-attr 表示降序"), ("page" = u32, Query), ("size" = u32, Query)),
    responses((status = 200, body = ListResponse))
)]
pub async fn list_docs(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Query(q): Query<ListQ>,
) -> Result<Json<ListResponse>, ApiError> {
    let mut conds = Vec::new();
    for (k, v) in &q.raw {
        if let Some(attr) = k.strip_prefix("filter[").and_then(|s| s.strip_suffix(']')) {
            conds.push(Cond {
                attr: attr.to_string(),
                op: Op::Eq,
                value: json!(v),
            });
        }
    }
    let sorts = match &q.sort {
        Some(s) if !s.is_empty() => {
            let (attr, desc) = match s.strip_prefix('-') {
                Some(a) => (a.to_string(), true),
                None => (s.clone(), false),
            };
            vec![Sort { attr, desc }]
        }
        _ => vec![],
    };
    let oq = ObjectQuery {
        type_code: mod_doc::DocService::TYPE.into(),
        conds,
        sorts,
        page: Some(Page {
            page: q.page.unwrap_or(1),
            size: q.size.unwrap_or(20),
        }),
    };
    let (items, total) = st
        .service
        .list(&oq)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    let page = oq.page.unwrap_or_default();
    let out = ListResponse {
        items: items.iter().map(|o| json!(o)).collect(),
        total,
        page: page.page,
        size: page.size,
    };
    Ok(Json(out))
}

/// GET /api/v1/docs/{obj_no}
#[utoipa::path(
    get, path = "/api/v1/docs/{obj_no}",
    params(("obj_no" = String, Path)),
    responses((status = 200, body = DocResponse), (status = 404))
)]
pub async fn get_doc(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Path(obj_no): Path<String>,
) -> Result<Json<DocResponse>, ApiError> {
    let d = st
        .service
        .detail(&obj_no)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(Json(to_response(d)))
}

/// GET /api/v1/docs/{obj_no}/file — 流式下载（Content-Disposition 原名，UTF-8 百分号编码）
#[utoipa::path(
    get, path = "/api/v1/docs/{obj_no}/file",
    params(("obj_no" = String, Path)),
    responses((status = 200, description = "文件流"), (status = 404))
)]
pub async fn download_doc(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Path(obj_no): Path<String>,
) -> Result<Response, ApiError> {
    let (name, stream, size) = st
        .service
        .open_original(&obj_no)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!(
            "attachment; filename=\"{}\"; filename*=UTF-8''{}",
            ascii_fallback(&name),
            pct_encode(&name)
        )
        .parse()
        .unwrap_or(HeaderValue::from_static("attachment")),
    );
    if let Ok(v) = HeaderValue::from_str(&size.to_string()) {
        headers.insert(header::CONTENT_LENGTH, v);
    }

    let body = axum::body::Body::from_stream(ReaderStream::new(stream));
    Ok((StatusCode::OK, headers, body).into_response())
}

fn to_response(d: mod_doc::DocDetail) -> DocResponse {
    DocResponse {
        object: json!(d.object),
        file: d.file.map(|f| FileInfoDto {
            name: f.name,
            hash: f.hash,
            size: f.size,
            mime: f.mime,
        }),
    }
}

/// ASCII 兜底文件名（header 保留字符安全）
fn ascii_fallback(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii() && !c.is_control() && *c != '"' && *c != '\\')
        .collect();
    if cleaned.is_empty() {
        "download".into()
    } else {
        cleaned
    }
}

/// UTF-8 百分号编码（RFC 5987 filename*）
fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

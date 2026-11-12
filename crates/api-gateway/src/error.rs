//! 统一错误体 {code, message, trace_id}（ADR-0015）+ trace 中间件。

use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use core_model::DomainError;
use serde_json::json;
use uuid::Uuid;

/// 请求追踪 ID（每请求生成；Extension + X-Trace-Id 响应头）
#[derive(Clone)]
pub struct TraceId(pub String);

pub async fn trace_mw(mut req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let tid = TraceId(Uuid::new_v4().to_string());
    let mut res = {
        req.extensions_mut().insert(tid.clone());
        next.run(req).await
    };
    if let Ok(v) = HeaderValue::from_str(&tid.0) {
        res.headers_mut().insert("x-trace-id", v);
    }
    // PLM 数据不得被浏览器/中间层缓存（审计与一致性；终审生命周期 e2e 实证 GET 启发式缓存竞态）
    res.headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    res
}

/// DomainError → HTTP 错误体（code/status 来自 core-model 错误码表）
pub struct ApiError {
    err: DomainError,
    trace_id: String,
    message_override: Option<String>,
}

impl ApiError {
    pub fn new(err: DomainError, trace_id: &str) -> Self {
        Self {
            err,
            trace_id: trace_id.to_string(),
            message_override: None,
        }
    }

    /// 覆盖错误体 message（语义细化时使用；code/status 不变）
    pub fn with_message(mut self, msg: &str) -> Self {
        self.message_override = Some(msg.to_string());
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.err.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let message = self
            .message_override
            .unwrap_or_else(|| self.err.to_string());
        let body = json!({
            "code": self.err.code(),
            "message": message,
            "trace_id": self.trace_id,
        });
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ApiError 错误码/状态码映射表（稳定错误码合同；body 渲染由 e2e 钉）
    #[test]
    fn duplicate_maps_to_409_and_stable_code() {
        let e = DomainError::ObjNoDuplicate("drawing:DWG-1".into());
        let api = ApiError::new(e, "t-123");
        assert_eq!(api.err.code(), "MDL-OBJ-NO-DUPLICATE");
        assert_eq!(api.err.http_status(), 409);
        assert_eq!(api.trace_id, "t-123");
    }

    #[test]
    fn file_multi_maps_to_400() {
        let e = DomainError::FileMulti;
        assert_eq!(e.code(), "MDL-FILE-MULTI");
        assert_eq!(e.http_status(), 400);
    }

    #[test]
    fn field_required_maps_to_400() {
        let e = DomainError::FieldRequired("obj_no".into());
        assert_eq!(e.code(), "MDL-FIELD-REQUIRED");
        assert_eq!(e.http_status(), 400);
    }

    #[test]
    fn file_too_large_maps_to_413() {
        let e = DomainError::FileTooLarge(1024);
        assert_eq!(e.code(), "MDL-FILE-TOO-LARGE");
        assert_eq!(e.http_status(), 413);
    }

    #[test]
    fn state_forbidden_maps_to_423() {
        let e = DomainError::ObjStateForbidden("x".into());
        assert_eq!(e.http_status(), 423);
    }

    #[test]
    fn trace_id_header_value_roundtrip() {
        let tid = TraceId(Uuid::new_v4().to_string());
        assert!(HeaderValue::from_str(&tid.0).is_ok());
        let _ = axum::http::header::HeaderName::from_static("x-trace-id");
    }
}

//! /api/v1/auth：登录与 refresh 轮换（豁免鉴权）。

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::Extension;
use axum::Json;
use core_sec::TokenPair;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct RefreshReq {
    pub refresh_token: String,
}

#[derive(Serialize, ToSchema)]
pub struct TokenPairDto {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

fn dto(p: TokenPair) -> TokenPairDto {
    TokenPairDto {
        access_token: p.access_token,
        refresh_token: p.refresh_token,
        expires_in: p.expires_in,
    }
}

/// POST /api/v1/auth/login
#[utoipa::path(
    post, path = "/api/v1/auth/login",
    request_body = LoginReq,
    responses((status = 200, body = TokenPairDto), (status = 401, description = "凭证错误"))
)]
pub async fn login(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Json(req): Json<LoginReq>,
) -> Result<Json<TokenPairDto>, ApiError> {
    let p = st
        .sec
        .login(&req.username, &req.password)
        .await
        .map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(Json(dto(p)))
}

/// POST /api/v1/auth/refresh
#[utoipa::path(
    post, path = "/api/v1/auth/refresh",
    request_body = RefreshReq,
    responses((status = 200, body = TokenPairDto), (status = 401))
)]
pub async fn refresh(
    State(st): State<AppState>,
    Extension(tid): Extension<crate::error::TraceId>,
    Json(req): Json<RefreshReq>,
) -> Result<Json<TokenPairDto>, ApiError> {
    let p = st.sec.refresh(&req.refresh_token).await.map_err(|e| ApiError::new(e, &tid.0))?;
    Ok(Json(dto(p)))
}

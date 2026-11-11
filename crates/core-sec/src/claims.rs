//! JWT claims 与令牌时效常量。

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const ACCESS_TTL_SECS: i64 = 15 * 60;
pub const REFRESH_TTL_SECS: i64 = 7 * 24 * 3600;

/// typ: "access" | "refresh"——refresh 冒充 access 在 parse_access 拒绝（Review Focus #1）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub username: String,
    pub typ: String,
    pub exp: i64,
}

impl Claims {
    pub fn access(sub: Uuid, username: &str, now: i64) -> Self {
        Self {
            sub,
            username: username.into(),
            typ: "access".into(),
            exp: now + ACCESS_TTL_SECS,
        }
    }

    pub fn refresh(sub: Uuid, username: &str, now: i64) -> Self {
        Self {
            sub,
            username: username.into(),
            typ: "refresh".into(),
            exp: now + REFRESH_TTL_SECS,
        }
    }
}

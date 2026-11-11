//! SecService：登录/轮换/校验 + admin 引导。密钥策略（plan 全局约束）：
//! `PLM_JWT_SECRET` 优先 → `<data_dir>/jwt.key` 读/建 → 兜底随机（WARN，重启失效）。

use crate::claims::{Claims, ACCESS_TTL_SECS, REFRESH_TTL_SECS};
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use core_model::{DomainError, Result};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminBootInfo {
    pub created: bool,
    /// 仅首建且密码随机生成时返回（调用方打印一次到日志）
    pub printed_password: Option<String>,
}



/// 已认证用户（中间件注入 Extension）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: uuid::Uuid,
    pub username: String,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// 密钥解析（纯 IO 可单测）：env 优先 → 文件复用 → 随机生成落盘
pub fn resolve_secret(env_secret: Option<&str>, key_dir: &Path) -> std::io::Result<String> {
    if let Some(s) = env_secret.filter(|s| !s.trim().is_empty()) {
        return Ok(s.to_string());
    }
    let path: PathBuf = key_dir.join("jwt.key");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let t = existing.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    let secret = hex::encode(rand_bytes());
    std::fs::create_dir_all(key_dir)?;
    std::fs::write(&path, &secret)?;
    tracing::warn!(path = %path.display(), "PLM_JWT_SECRET 未设置：已生成密钥文件（跨重启有效）；多实例部署请显式设置 PLM_JWT_SECRET");
    Ok(secret)
}

fn rand_bytes() -> [u8; 32] {
    // OsRng 走 OS 熵源（password-hash 同源），避开 rand 0.9 API 分歧
    use argon2::password_hash::rand_core::RngCore;
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    b
}

fn random_password() -> String {
    // 24 位无歧义字符集
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789";
    let bytes = rand_bytes();
    bytes.iter().map(|b| CHARS[(*b as usize) % CHARS.len()] as char).collect()
}

pub struct SecService {
    pool: PgPool,
    enc: EncodingKey,
    dec: DecodingKey,
}

impl SecService {
    /// 生产入口：env/data_dir 密钥策略
    pub fn new(pool: PgPool, jwt_secret: String) -> Self {
        Self::build(pool, jwt_secret)
    }

    /// 密钥策略单测入口：显式 key_dir
    pub fn new_with_key_dir(env_secret: Option<&str>, key_dir: &Path) -> Self {
        let secret = resolve_secret(env_secret, key_dir).expect("jwt.key 读写失败");
        Self::build(PgPool::connect_lazy("postgres://invalid").expect("lazy pool"), secret)
    }

    fn build(pool: PgPool, secret: String) -> Self {
        Self {
            pool,
            enc: EncodingKey::from_secret(secret.as_bytes()),
            dec: DecodingKey::from_secret(secret.as_bytes()),
        }
    }

    /// 签发任意 claims（内部 + 测试 + 后续密码重置令牌）
    pub fn sign_claims(&self, c: &Claims) -> Result<String> {
        Ok(jsonwebtoken::encode(&Header::default(), c, &self.enc)
            .map_err(|e| DomainError::Internal(format!("jwt encode: {e}")))?)
    }

    fn decode(&self, token: &str) -> Result<Claims> {
        jsonwebtoken::decode::<Claims>(
            token,
            &self.dec,
            &Validation::new(jsonwebtoken::Algorithm::HS256),
        )
        .map(|d| d.claims)
        .map_err(|_| DomainError::SecUnauthorized)
    }

    /// 校验 access（typ 必须 = access，exp 自动校验；refresh 冒充 → 401）
    pub fn parse_access(&self, token: &str) -> Result<Claims> {
        let c = self.decode(token)?;
        if c.typ != "access" {
            return Err(DomainError::SecUnauthorized);
        }
        Ok(c)
    }

    fn parse_refresh(&self, token: &str) -> Result<Claims> {
        let c = self.decode(token)?;
        if c.typ != "refresh" {
            return Err(DomainError::SecUnauthorized);
        }
        Ok(c)
    }

    /// 首启引导：仅当无 admin 时创建；已存在则不改密码（Review Focus #4）
    pub async fn ensure_admin(&self, password: Option<String>) -> Result<AdminBootInfo> {
        let exists: Option<uuid::Uuid> = sqlx::query_scalar(
            "SELECT id FROM sec_user WHERE lower(username) = 'admin' LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        if exists.is_some() {
            return Ok(AdminBootInfo { created: false, printed_password: None });
        }
        let (plain, printed) = match password.filter(|p| !p.trim().is_empty()) {
            Some(p) => (p, None),
            None => {
                let p = random_password();
                (p.clone(), Some(p))
            }
        };
        let hash = hash_password(&plain)?;
        sqlx::query("INSERT INTO sec_user(username, password_hash, display_name) VALUES ('admin', $1, '管理员')")
            .bind(hash)
            .execute(&self.pool)
            .await?;
        Ok(AdminBootInfo { created: true, printed_password: printed })
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<TokenPair> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: uuid::Uuid,
            username: String,
            password_hash: String,
            enabled: bool,
        }
        let row: Option<Row> = sqlx::query_as(
            "SELECT id, username, password_hash, enabled FROM sec_user WHERE lower(username) = lower($1)",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;
        // 统一 401 语义（不泄露"用户存在"）；argon2 错误一律折叠为凭证错误
        let bad = || DomainError::SecBadCredentials;
        let r = row.ok_or_else(bad)?;
        if !r.enabled {
            return Err(bad());
        }
        let parsed = PasswordHash::new(&r.password_hash).map_err(|_| bad())?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|_| bad())?;

        let pair = self.issue(&r.id, &r.username);
        let _ = sqlx::query("UPDATE sec_user SET last_login_at = now() WHERE id = $1")
            .bind(r.id)
            .execute(&self.pool)
            .await;
        Ok(pair)
    }

    /// refresh 轮换：校验 refresh claim + 用户仍启用；骨架期不吊销旧 refresh（文档化）
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenPair> {
        let c = self.parse_refresh(refresh_token)?;
        let enabled: Option<bool> = sqlx::query_scalar(
            "SELECT enabled FROM sec_user WHERE id = $1 AND lower(username) = lower($2)",
        )
        .bind(c.sub)
        .bind(&c.username)
        .fetch_optional(&self.pool)
        .await?;
        match enabled {
            Some(true) => Ok(self.issue(&c.sub, &c.username)),
            _ => Err(DomainError::SecUnauthorized),
        }
    }

    fn issue(&self, sub: &uuid::Uuid, username: &str) -> TokenPair {
        let t = now();
        TokenPair {
            access_token: self
                .sign_claims(&Claims::access(*sub, username, t))
                .expect("access 签发不应失败"),
            refresh_token: self
                .sign_claims(&Claims::refresh(*sub, username, t))
                .expect("refresh 签发不应失败"),
            expires_in: ACCESS_TTL_SECS,
        }
    }
}

fn hash_password(plain: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| DomainError::Internal(format!("argon2 hash: {e}")))?
        .to_string())
}

/// 中间件用：Bearer 前缀剥离（畸形 → 401）
pub fn strip_bearer(header_value: &str) -> Result<&str> {
    header_value
        .strip_prefix("Bearer ")
        .filter(|t| !t.trim().is_empty())
        .map(|t| t.trim())
        .ok_or(DomainError::SecUnauthorized)
}

/// 暴露 REFRESH 时效给调用方常量使用（文档/测试）
pub fn refresh_ttl() -> i64 {
    REFRESH_TTL_SECS
}

//! SecService：登录/轮换/校验 + admin 引导。密钥策略（plan 全局约束）：
//! `PLM_JWT_SECRET` 优先 → `<data_dir>/jwt.key` 读/建 → 兜底随机（WARN，重启失效）。

use crate::claims::{Claims, ACCESS_TTL_SECS, REFRESH_TTL_SECS};
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use core_model::{DomainError, Result};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

/// 已认证用户（中间件注入 Extension；max_secret_level 由授权段回填）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: uuid::Uuid,
    pub username: String,
    pub max_secret_level: i16,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// 密钥解析（纯 IO 可单测）：env 优先 → 文件复用 → 随机生成落盘；
/// 落盘不可用时**降级**为内存随机密钥并 WARN（服务照常启动，重启失效）——终审 I2。
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
    let persisted = write_secret_file(key_dir, &path, &secret);
    if let Err(e) = persisted {
        tracing::warn!(
            error = %e,
            "jwt.key 落盘失败：使用内存随机密钥（重启后全部令牌失效）。多实例部署请显式设置 PLM_JWT_SECRET"
        );
    } else {
        tracing::warn!(
            path = %path.display(),
            "PLM_JWT_SECRET 未设置：已生成密钥文件（跨重启有效）；多实例部署请显式设置 PLM_JWT_SECRET"
        );
    }
    Ok(secret)
}

/// jwt.key 落盘：Unix 收紧 0600（终审 I1）；失败由调用方降级
fn write_secret_file(key_dir: &Path, path: &Path, secret: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(key_dir)?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
            .write_all(secret.as_bytes())?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = key_dir;
        std::fs::write(path, secret)
    }
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
    bytes
        .iter()
        .map(|b| CHARS[(*b as usize) % CHARS.len()] as char)
        .collect()
}

pub struct SecService {
    pool: PgPool,
    enc: EncodingKey,
    dec: DecodingKey,
    /// casbin 求值器（策略源=PG 表；grant/revoke/建号后整体重建，arc-swap 即时生效）。
    /// None=尚未装载（fail-closed：一切 enforce → false）
    enf: arc_swap::ArcSwap<Option<casbin::Enforcer>>,
}

impl SecService {
    /// 生产入口：env/data_dir 密钥策略
    pub fn new(pool: PgPool, jwt_secret: String) -> Self {
        Self::build(pool, jwt_secret)
    }

    /// 密钥策略单测入口：显式 key_dir
    pub fn new_with_key_dir(env_secret: Option<&str>, key_dir: &Path) -> Self {
        let secret = resolve_secret(env_secret, key_dir).expect("jwt.key 读写失败");
        Self::build(
            PgPool::connect_lazy("postgres://invalid").expect("lazy pool"),
            secret,
        )
    }

    fn build(pool: PgPool, secret: String) -> Self {
        Self {
            pool,
            enc: EncodingKey::from_secret(secret.as_bytes()),
            dec: DecodingKey::from_secret(secret.as_bytes()),
            enf: arc_swap::ArcSwap::from_pointee(None),
        }
    }

    /// 从 PG 策略表重建求值器（启动/grant/revoke/建号后调用；进程内即时生效）
    pub async fn reload_policies(&self) -> Result<()> {
        let e = crate::rbac::build_enforcer(&self.pool).await?;
        self.enf.store(Arc::new(Some(e)));
        Ok(())
    }

    /// 授权判定（用户名为主体；未装载/未知用户 → false，fail-closed）
    pub async fn enforce(&self, username: &str, obj: &str, act: &str) -> Result<bool> {
        use casbin::CoreApi;
        match self.enf.load().as_ref() {
            Some(e) => e
                .enforce((username, obj, act))
                .map_err(|err| DomainError::Internal(format!("casbin enforce: {err}"))),
            None => Ok(false),
        }
    }

    /// 建用户（admin 专用路径经网关权限层把关）：重复(大小写归一) → USER-EXISTS；角色必须存在
    pub async fn create_user(
        &self,
        username: &str,
        password: &str,
        display_name: Option<&str>,
        roles: &[&str],
        max_secret_level: i16,
    ) -> Result<uuid::Uuid> {
        for r in roles {
            let ok: Option<i32> = sqlx::query_scalar("SELECT 1 FROM sec_role WHERE code = $1")
                .bind(r)
                .fetch_optional(&self.pool)
                .await?;
            if ok.is_none() {
                return Err(DomainError::SecRoleNotFound(r.to_string()));
            }
        }
        let hash = hash_password(password)?;
        let id: Option<uuid::Uuid> = sqlx::query_scalar(
            "INSERT INTO sec_user(username, password_hash, display_name, max_secret_level)              VALUES ($1, $2, $3, $4) ON CONFLICT (lower(username)) DO NOTHING RETURNING id",
        )
        .bind(username)
        .bind(hash)
        .bind(display_name)
        .bind(max_secret_level)
        .fetch_optional(&self.pool)
        .await?;
        let id = id.ok_or(DomainError::SecUserExists)?;
        for r in roles {
            sqlx::query("INSERT INTO sec_user_role(username, role_code) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                .bind(username)
                .bind(r)
                .execute(&self.pool)
                .await?;
        }
        self.reload_policies().await?;
        Ok(id)
    }

    pub async fn grant(&self, username: &str, role: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO sec_user_role(username, role_code) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(username)
        .bind(role)
        .execute(&self.pool)
        .await?;
        self.reload_policies().await
    }

    pub async fn revoke(&self, username: &str, role: &str) -> Result<()> {
        sqlx::query("DELETE FROM sec_user_role WHERE username = $1 AND role_code = $2")
            .bind(username)
            .bind(role)
            .execute(&self.pool)
            .await?;
        self.reload_policies().await
    }

    /// 密级上限（ABAC 第一维；每请求一次索引查询，位图优化见 PRD 规模化目标）
    pub async fn max_secret_level_of(&self, user: uuid::Uuid) -> Result<i16> {
        sqlx::query_scalar("SELECT max_secret_level FROM sec_user WHERE id = $1")
            .bind(user)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(DomainError::SecUnauthorized)
    }

    pub async fn user_id(&self, username: &str) -> Result<uuid::Uuid> {
        sqlx::query_scalar("SELECT id FROM sec_user WHERE lower(username) = lower($1)")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(DomainError::SecUnauthorized)
    }

    /// 签发任意 claims（内部 + 测试 + 后续密码重置令牌）
    pub fn sign_claims(&self, c: &Claims) -> Result<String> {
        jsonwebtoken::encode(&Header::default(), c, &self.enc)
            .map_err(|e| DomainError::Internal(format!("jwt encode: {e}")))
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
        let exists: Option<uuid::Uuid> =
            sqlx::query_scalar("SELECT id FROM sec_user WHERE lower(username) = 'admin' LIMIT 1")
                .fetch_optional(&self.pool)
                .await?;
        if exists.is_some() {
            return Ok(AdminBootInfo {
                created: false,
                printed_password: None,
            });
        }
        let (plain, printed) = match password.filter(|p| !p.trim().is_empty()) {
            Some(p) => (p, None),
            None => {
                let p = random_password();
                (p.clone(), Some(p))
            }
        };
        let hash = hash_password(&plain)?;
        // M5：ON CONFLICT 幂等（双实例首启败者不再崩溃）
        let created = sqlx::query(
            "INSERT INTO sec_user(username, password_hash, display_name) \
             VALUES ('admin', $1, '管理员') ON CONFLICT (lower(username)) DO NOTHING",
        )
        .bind(hash)
        .execute(&self.pool)
        .await?
        .rows_affected()
            > 0;
        // admin 角色绑定幂等补齐（迁移/历史库也可能缺）；随后重建求值器
        sqlx::query(
            "INSERT INTO sec_user_role(username, role_code) VALUES ('admin', 'admin') \
             ON CONFLICT DO NOTHING",
        )
        .execute(&self.pool)
        .await?;
        if created {
            self.reload_policies().await?;
        }
        Ok(AdminBootInfo {
            created,
            printed_password: printed.filter(|_| created),
        })
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
        // 统一 401 语义：响应体与**耗时**均不区分"用户不存在/禁用/密码错"（终审 I3）——
        // 未知/禁用路径对 DUMMY_HASH 跑一次等价 argon2 验证
        let bad = || DomainError::SecBadCredentials;
        let r = match row {
            Some(r) if r.enabled => r,
            _ => {
                fold_unknown_user(password);
                return Err(bad());
            }
        };
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

/// 未知/禁用用户的等价验证目标（终审 I3）：对同一密码短语 argon2 一次，抹平时序差。
/// PHC 串进程内懒生成（OnceLock）——自愈、无魔法常量；生成失败则跳过（最坏退回时序差，不影响正确性）。
fn fold_unknown_user(password: &str) {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let hash = DUMMY.get_or_init(|| {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(b"dummy", &salt)
            .map(|h| h.to_string())
            .unwrap_or_default()
    });
    if let Ok(parsed) = PasswordHash::new(hash) {
        let _ = Argon2::default().verify_password(password.as_bytes(), &parsed);
    }
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

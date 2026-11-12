//! RustPLM 服务端装配（概设 §2.1 R-D4：bin 不持 handler，只组装）。
//! 序：tracing → Config(figment) → pool → 双 MIGRATOR → TypeRegistry(+watcher) →
//!     HookRegistry/PgObjectStore/LocalFileStore/DocService → router → serve(优雅停机)。

use anyhow::{bail, Context, Result};
use figment::{
    providers::{Env, Serialized},
    Figment,
};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Deserialize, serde::Serialize)]
struct Config {
    database_url: String,
    data_dir: PathBuf,
    http_port: u16,
    max_upload: u64,
    /// 缺省：随机生成并打印一次
    admin_password: Option<String>,
    /// 缺省：data_dir/jwt.key 文件复用
    jwt_secret: Option<String>,
    /// IM 集成（增量10）：none|dingtalk|wecom|feishu
    #[serde(default)]
    im_provider: String,
    #[serde(default)]
    im_webhook: String,
    #[serde(default)]
    im_secret: String,
    /// 详情页链接模板（{obj_no} 占位）
    #[serde(default)]
    im_doc_url_template: String,
    /// IM id→PLM username 映射 JSON（如 {"im-ding-1":"zhangsan"}）
    #[serde(default)]
    im_user_map: String,
}

impl Config {
    #[allow(clippy::field_reassign_with_default)]
    fn load() -> Result<Self> {
        let mut defaults = Self {
            database_url: String::new(),
            data_dir: PathBuf::from("data"),
            http_port: 8080,
            max_upload: 200 * 1024 * 1024,
            admin_password: None,
            jwt_secret: None,
            im_provider: String::new(),
            im_webhook: String::new(),
            im_secret: String::new(),
            im_doc_url_template: String::new(),
            im_user_map: String::new(),
        };
        let _ = &mut defaults;
        let cfg: Config = Figment::from(Serialized::defaults(defaults))
            .merge(Env::raw().map(|k| k.as_str().to_lowercase().into()))
            .merge(Env::prefixed("PLM_"))
            .extract()?;
        if cfg.database_url.is_empty() {
            bail!("DATABASE_URL 未设置（示例：postgres://plm:plmdev@localhost:5432/plm）");
        }
        Ok(cfg)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::load().context("读取配置失败")?;
    tracing::info!(port = cfg.http_port, data_dir = %cfg.data_dir.display(), "RustPLM 启动中");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(&cfg.database_url)
        .await
        .context("连接 PostgreSQL 失败")?;

    // 迁移链：core-model(0001) → mod-doc(0002/0003)（共享 _sqlx_migrations，各 Migrator 忽略他者版本）
    core_model::migrator()
        .run(&pool)
        .await
        .context("core-model 迁移失败")?;
    mod_doc::migrator()
        .run(&pool)
        .await
        .context("mod-doc 迁移失败")?;
    core_sec::migrator()
        .run(&pool)
        .await
        .context("core-sec 迁移失败")?;
    core_version::migrator()
        .run(&pool)
        .await
        .context("core-version 迁移失败")?;
    core_bom::migrator()
        .run(&pool)
        .await
        .context("core-bom 迁移失败")?;

    // JWT 密钥策略（env → data_dir/jwt.key → 随机）+ admin 引导（仅缺省时创建）
    let jwt_secret = core_sec::resolve_secret(cfg.jwt_secret.as_deref(), &cfg.data_dir)
        .context("解析 JWT 密钥失败")?;
    let sec = Arc::new(core_sec::SecService::new(pool.clone(), jwt_secret));
    let boot = sec
        .ensure_admin(cfg.admin_password.clone())
        .await
        .context("admin 引导失败")?;
    if let Some(pw) = &boot.printed_password {
        tracing::warn!("已创建内置 admin 账号，初始密码（仅此一次打印，请立即修改）：{pw}");
    }

    let registry = core_model::TypeRegistry::load(pool.clone()).await?;
    let watcher = registry.spawn_watcher(5).await;

    let hooks = Arc::new(core_model::HookRegistry::new());
    let objects = Arc::new(core_model::PgObjectStore::new(
        pool.clone(),
        registry.clone(),
        hooks,
    ));
    let files = Arc::new(core_files::LocalFileStore::new(cfg.data_dir.clone()));
    let service = Arc::new(mod_doc::DocService::new(
        pool.clone(),
        registry.clone(),
        objects,
        files,
    ));

    let bom = Arc::new(core_bom::BomStore::new(pool.clone()));
    // 发布冻结注入（增量9 BOM 版本链）：released 同事务快照 BOM 子树（装配共用实现见 api-gateway::bom_release_hook）
    let lifecycle = Arc::new(core_version::LifecycleStore::new_with_hook(
        pool.clone(),
        Some(api_gateway::bom_release_hook(bom.clone())),
    ));

    // IM 集成（增量10）：迁移 + 卡片推送轮询 + 回调上下文
    core_im::migrator()
        .run(&pool)
        .await
        .context("core-im 迁移失败")?;
    let sender = core_im::sender_for(
        &cfg.im_provider,
        &cfg.im_webhook,
        &cfg.im_secret,
        reqwest::Client::new(),
    );
    let pusher = Arc::new(core_im::CardPusher::new(
        pool.clone(),
        sender,
        cfg.im_doc_url_template.clone(),
    ));
    pusher.spawn(1000);
    let im_user_map: std::collections::HashMap<String, String> =
        if cfg.im_user_map.trim().is_empty() {
            Default::default()
        } else {
            serde_json::from_str(&cfg.im_user_map).context("PLM_IM_USER_MAP 须为 JSON 对象")?
        };
    // 事件外投循环（ADR-0007；1s 轮询 + 退避/死信）
    core_events::migrator()
        .run(&pool)
        .await
        .context("core-events 迁移失败")?;
    let dispatcher = Arc::new(core_events::EventDispatcher::new(pool.clone()));
    let dispatcher_handle = dispatcher.spawn(1000);
    let state = api_gateway::AppState {
        registry: registry.clone(),
        service,
        pool,
        max_upload: cfg.max_upload,
        sec,
        lifecycle,
        bom,
        im_secret: cfg.im_secret.clone(),
        im_user_map,
    };
    let app = api_gateway::router(state);

    let addr = format!("0.0.0.0:{}", cfg.http_port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("绑定 {addr} 失败"))?;
    tracing::info!(%addr, "RustPLM 就绪");

    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    watcher.abort();
    dispatcher_handle.abort();
    result.context("HTTP 服务异常退出")?;
    tracing::info!("RustPLM 已优雅停机");
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("收到停机信号，排空在途请求…");
}

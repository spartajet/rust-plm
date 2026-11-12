//! 三档流程集成测试（增量5 / SMB-2）：minimal / standard / strict + 档位热切换 + 会签。

use core_model::{HookRegistry, PgObjectStore, TypeRegistry};
use core_version::{LifecycleEvent, LifecycleStore};
use mod_doc::DocService;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn unique_db(hint: &str) -> String {
    let hint: String = hint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(
        "tier_{}_{}",
        hint,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

struct Ctx {
    svc: Arc<DocService>,
    store: LifecycleStore,
    registry: Arc<TypeRegistry>,
    pool: PgPool,
    db: String,
    admin_url: String,
    files_root: PathBuf,
}

async fn setup(hint: &str) -> Ctx {
    let admin_url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL");
    let db = unique_db(hint);
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE \"{db}\""))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let prefix = admin_url
        .rsplit_once('/')
        .map(|(p, _)| p)
        .unwrap()
        .to_string();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&format!("{prefix}/{db}"))
        .await
        .unwrap();
    core_model::migrator().run(&pool).await.unwrap();
    mod_doc::migrator().run(&pool).await.unwrap();
    core_version::migrator().run(&pool).await.unwrap();
    let registry = TypeRegistry::load(pool.clone()).await.unwrap();
    let files_root = std::env::temp_dir().join(format!("tier-{db}"));
    let files = Arc::new(core_files::LocalFileStore::new(&files_root));
    let hooks = Arc::new(HookRegistry::new());
    let objects = Arc::new(PgObjectStore::new(pool.clone(), registry.clone(), hooks));
    let svc = Arc::new(DocService::new(
        pool.clone(),
        registry.clone(),
        objects,
        files,
    ));
    let store = LifecycleStore::new(pool.clone());
    Ctx {
        svc,
        store,
        registry,
        pool,
        db,
        admin_url,
        files_root,
    }
}

impl Ctx {
    async fn drop_it(self) {
        self.pool.close().await;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.admin_url)
            .await
            .unwrap();
        sqlx::query(&format!(
            "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
            self.db
        ))
        .execute(&admin)
        .await
        .unwrap();
        admin.close().await;
        let _ = std::fs::remove_dir_all(&self.files_root);
    }

    async fn new_doc(&self, obj_no: &str) {
        let mut r = Cursor::new(b"TIER".to_vec());
        self.svc
            .upload(
                obj_no.into(),
                serde_json::json!({"name": "N"})
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

    /// 改档位（模拟网关 PUT：UPDATE config + registry 热刷新）
    async fn set_tier(&self, tier: &str) {
        sqlx::query(
            "UPDATE cm_type SET config = config || jsonb_build_object('flow_tier', $1) \
             WHERE code = 'drawing'",
        )
        .bind(tier)
        .execute(&self.pool)
        .await
        .unwrap();
        self.registry.reload_if_changed().await.unwrap();
    }

    async fn outbox_count(&self, obj_no: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM event_outbox WHERE payload->>'obj_no' = $1")
            .bind(obj_no)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn snapshot_count(&self, obj_no: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM cm_obj_snapshot WHERE obj_no = $1")
            .bind(obj_no)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

const A: Option<Uuid> = Some(uuid::Uuid::from_u128(0xAA00));
const B: Option<Uuid> = Some(uuid::Uuid::from_u128(0xBB00));

/// Review Focus #1：minimal submit 直达 released 且快照/事件照发
#[tokio::test]
async fn minimal_submit_goes_straight_to_released_with_snapshot_and_event() {
    let c = setup("min").await;
    c.set_tier("minimal").await;
    c.new_doc("DWG-MIN-1").await;

    let o = c
        .store
        .transition("drawing", "DWG-MIN-1", LifecycleEvent::Submit, A)
        .await
        .unwrap();
    assert_eq!(o.state, "released", "极简档提交即发布");

    // 发布语义跨档一致（Review Focus #1：快照/事件照发）
    assert_eq!(c.snapshot_count("DWG-MIN-1").await, 1);
    assert_eq!(c.outbox_count("DWG-MIN-1").await, 1);

    // 发布不可变：released update → 423（Review Focus #4 同源）
    let o2 = c.svc.detail("DWG-MIN-1").await.unwrap();
    let err = c
        .svc
        .update_object(
            "DWG-MIN-1",
            &serde_json::json!({"name": "X"})
                .as_object()
                .unwrap()
                .clone(),
            o2.object.attrs_rev,
            Uuid::nil(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.http_status(), 423);
    c.drop_it().await;
}

/// standard 档行为与既有链路完全一致（回归保护）
#[tokio::test]
async fn standard_chain_unchanged() {
    let c = setup("std").await;
    c.set_tier("standard").await;
    c.new_doc("DWG-STD-1").await;
    let o = c
        .store
        .transition("drawing", "DWG-STD-1", LifecycleEvent::Submit, A)
        .await
        .unwrap();
    assert_eq!(o.state, "review");
    let o = c
        .store
        .transition("drawing", "DWG-STD-1", LifecycleEvent::Approve, B)
        .await
        .unwrap();
    assert_eq!(o.state, "released");
    assert_eq!(c.snapshot_count("DWG-STD-1").await, 1);
    c.drop_it().await;
}

/// Review Focus #2：strict 两级审批 + 同人第二级 → 409 maker-checker
#[tokio::test]
async fn strict_two_level_and_maker_checker() {
    let c = setup("strict").await;
    c.set_tier("strict").await;
    c.new_doc("DWG-ST-1").await;

    let o = c
        .store
        .transition("drawing", "DWG-ST-1", LifecycleEvent::Submit, A)
        .await
        .unwrap();
    assert_eq!(o.state, "review");

    // 第一级 approve（A）→ checked
    let o = c
        .store
        .transition("drawing", "DWG-ST-1", LifecycleEvent::Approve, A)
        .await
        .unwrap();
    assert_eq!(o.state, "checked");

    // 同人（A）第二级 → 409 maker-checker
    let e = c
        .store
        .transition("drawing", "DWG-ST-1", LifecycleEvent::Approve, A)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-LCS-MAKER-CHECKER");

    // 异人（B）第二级 → released
    let o = c
        .store
        .transition("drawing", "DWG-ST-1", LifecycleEvent::Approve, B)
        .await
        .unwrap();
    assert_eq!(o.state, "released");
    assert_eq!(c.snapshot_count("DWG-ST-1").await, 1);
    c.drop_it().await;
}

/// strict checked 态 reject 回 working（两级任一层可驳）
#[tokio::test]
async fn strict_checked_reject_back_to_working() {
    let c = setup("strej").await;
    c.set_tier("strict").await;
    c.new_doc("DWG-ST-2").await;
    c.store
        .transition("drawing", "DWG-ST-2", LifecycleEvent::Submit, A)
        .await
        .unwrap();
    c.store
        .transition("drawing", "DWG-ST-2", LifecycleEvent::Approve, A)
        .await
        .unwrap();
    let o = c
        .store
        .transition("drawing", "DWG-ST-2", LifecycleEvent::Reject, B)
        .await
        .unwrap();
    assert_eq!(o.state, "working", "checked 驳回回 working 可重新修改");
    c.drop_it().await;
}

/// Review Focus #3：档位切换即时生效（standard→minimal 后新对象 submit 直达）
#[tokio::test]
async fn tier_switch_takes_effect_immediately() {
    let c = setup("switch").await;
    c.set_tier("standard").await;
    c.new_doc("DWG-SW-1").await;
    let o = c
        .store
        .transition("drawing", "DWG-SW-1", LifecycleEvent::Submit, A)
        .await
        .unwrap();
    assert_eq!(o.state, "review", "标准档先到 review");

    // 切极简档（热生效）
    c.set_tier("minimal").await;

    // 该 review 态对象：approve 兜底直达 released（防卡死语义）
    let o = c
        .store
        .transition("drawing", "DWG-SW-1", LifecycleEvent::Approve, B)
        .await
        .unwrap();
    assert_eq!(o.state, "released");

    // 新对象：submit 直达 released
    c.new_doc("DWG-SW-2").await;
    let o = c
        .store
        .transition("drawing", "DWG-SW-2", LifecycleEvent::Submit, A)
        .await
        .unwrap();
    assert_eq!(o.state, "released", "切档后新对象 submit 直达 released");
    c.drop_it().await;
}

/// Review Focus #5 同源：档位修改网关门控（docs:approve）在 e2e 钉；此处钉 Tier parse
#[tokio::test]
async fn flow_tier_parse_and_default() {
    assert!(matches!(
        core_version::FlowTier::parse("minimal"),
        Some(core_version::FlowTier::Minimal)
    ));
    assert!(core_version::FlowTier::parse("ninja").is_none());
    // 缺省 standard（store 层验证：未设置 flow_tier 的类型走标准链）
    let c = setup("deftier").await;
    c.new_doc("DWG-DF-1").await;
    let o = c
        .store
        .transition("drawing", "DWG-DF-1", LifecycleEvent::Submit, A)
        .await
        .unwrap();
    assert_eq!(o.state, "review", "未配置档位缺省 standard");
    c.drop_it().await;
}

// ==================== 终审修复钉子（C1/I1） ====================

#[tokio::test]
async fn c1_maker_checker_includes_submitter() {
    // 终审 C1：第二级审批须异于提交人 **且** 复核人——提交人不得终审自己单据
    let c = setup("c1mc").await;
    c.set_tier("strict").await;
    c.new_doc("DWG-C1-1").await; // 提交人 = upload 内部 actor=None → created_by NULL？

    // 显式以 A 身份建/提交：upload 的 created_by 目前为 None（骨架 create 未带 actor），
    // 改用 SQL 注入提交人痕迹模拟"提交人=A"
    sqlx::query("UPDATE cm_object SET created_by = $1 WHERE obj_no = 'DWG-C1-1'")
        .bind(A)
        .execute(&c.pool)
        .await
        .unwrap();

    // A 提交（updated_by=A）→ review
    let o = c
        .store
        .transition("drawing", "DWG-C1-1", LifecycleEvent::Submit, A)
        .await
        .unwrap();
    assert_eq!(o.state, "review");
    // B 第一级 approve（updated_by=B）→ checked
    let o = c
        .store
        .transition("drawing", "DWG-C1-1", LifecycleEvent::Approve, B)
        .await
        .unwrap();
    assert_eq!(o.state, "checked");
    // A（提交人，异于复核人 B）第二级 → 409 maker-checker（C1 钉）
    let e = c
        .store
        .transition("drawing", "DWG-C1-1", LifecycleEvent::Approve, A)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-LCS-MAKER-CHECKER");
    // C（第三人）→ released
    let cc = Some(uuid::Uuid::from_u128(0xCC00));
    let o = c
        .store
        .transition("drawing", "DWG-C1-1", LifecycleEvent::Approve, cc)
        .await
        .unwrap();
    assert_eq!(o.state, "released");
    c.drop_it().await;
}

#[tokio::test]
async fn c1_actor_none_fail_closed_on_checked_approve() {
    // 终审 M1 同批：checked+Approve 且 actor=None → fail-closed
    let c = setup("nonact").await;
    c.set_tier("strict").await;
    c.new_doc("DWG-NONE-1").await;
    c.store
        .transition("drawing", "DWG-NONE-1", LifecycleEvent::Submit, A)
        .await
        .unwrap();
    c.store
        .transition("drawing", "DWG-NONE-1", LifecycleEvent::Approve, B)
        .await
        .unwrap();
    let e = c
        .store
        .transition("drawing", "DWG-NONE-1", LifecycleEvent::Approve, None)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-LCS-MAKER-CHECKER");
    c.drop_it().await;
}

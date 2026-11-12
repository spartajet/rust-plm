//! core-bom 集成测试（计划 Task1）：真 PG18 库。覆盖 Review Focus #1–#3。
//! 前置：core-model 迁移（cm_object/cm_type）+ core-bom 迁移（bom_edge/bom_path）。

use core_bom::{BomStore, ExpandMode, LineInput};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn unique_db(hint: &str) -> String {
    let hint: String = hint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(
        "bom_{}_{}",
        hint,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

struct Ctx {
    store: BomStore,
    pool: PgPool,
    /// 种子节点 id（node_ids[0] 为 root）
    node_ids: Vec<Uuid>,
    db: String,
    admin_url: String,
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
    core_bom::migrator().run(&pool).await.unwrap();

    // 种子：一个 bom 类型 + 40 个对象节点
    let type_id: Uuid = sqlx::query_scalar(
        "INSERT INTO cm_type(code, name, base_type) VALUES ('bomtest','BOM测试','bom') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut node_ids = Vec::new();
    for i in 0..40 {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO cm_object(type_id, obj_no, attrs) VALUES ($1, $2, '{}') RETURNING id",
        )
        .bind(type_id)
        .bind(format!("BN-{i}"))
        .fetch_one(&pool)
        .await
        .unwrap();
        node_ids.push(id);
    }
    let store = BomStore::new(pool.clone());
    Ctx {
        store,
        pool,
        node_ids,
        db,
        admin_url,
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
    }

    /// 递归 CTE 全量展开（对拍基准，含数量聚合；不含 root 自身）
    async fn cte_expand(&self, root: Uuid) -> HashMap<Uuid, i64> {
        let rows: Vec<(Uuid, i64)> = sqlx::query_as(
            "WITH RECURSIVE t(child, qty) AS ( \
               SELECT child, qty FROM bom_edge WHERE parent = $1 \
               UNION ALL \
               SELECT e.child, t.qty * e.qty FROM bom_edge e JOIN t ON e.parent = t.child \
             ) SELECT child, SUM(qty)::bigint FROM t GROUP BY child",
        )
        .bind(root)
        .fetch_all(&self.pool)
        .await
        .unwrap()
        .into_iter()
        .collect();
        rows.into_iter().collect()
    }
}

const ROOT: usize = 0;

#[tokio::test]
async fn build_expand_matches_cte_three_modes() {
    // Review Focus #4：三模式展开行数/内容（与 CTE 对拍为真值来源）
    let c = setup("expand").await;
    let ids = &c.node_ids;
    // 结构：root → [1..9]（数量 2..4），每个中层 → 2 个叶（10..39 交错）
    let lines: Vec<LineInput> = ids
        .iter()
        .take(9)
        .skip(1)
        .enumerate()
        .map(|(i, child)| LineInput {
            line_no: (i + 1) as i32,
            child: *child,
            qty: 2,
        })
        .collect();
    c.store.set_lines(ids[ROOT], lines).await.unwrap();

    let mid_lines = [
        LineInput {
            line_no: 1,
            child: ids[10],
            qty: 1,
        },
        LineInput {
            line_no: 2,
            child: ids[11],
            qty: 2,
        },
    ];
    for mid in ids.iter().take(9).skip(1) {
        c.store.set_lines(*mid, mid_lines.to_vec()).await.unwrap();
    }

    // Full：闭包展开 == CTE 对拍（Review Focus #2 静态部分）
    let full = c.store.expand(ids[ROOT], ExpandMode::Full).await.unwrap();
    let cte = c.cte_expand(ids[ROOT]).await;
    assert_eq!(full.len(), cte.len(), "全量展开节点数与 CTE 一致");
    for row in &full {
        assert_eq!(
            row.qty_total, cte[&row.child],
            "节点 {} 数量与 CTE 一致",
            row.child
        );
    }

    // LeafOnly：只有叶子（无子边）
    let leaf = c
        .store
        .expand(ids[ROOT], ExpandMode::LeafOnly)
        .await
        .unwrap();
    assert!(
        leaf.iter()
            .all(|r| r.child == ids[10] || r.child == ids[11]),
        "尾级只含叶子节点"
    );

    // OneLevel：恰好 8 行（root 的直接子）
    let one = c
        .store
        .expand(ids[ROOT], ExpandMode::OneLevel)
        .await
        .unwrap();
    assert_eq!(one.len(), 8);
    c.drop_it().await;
}

#[tokio::test]
async fn cycle_self_and_chain_rejected() {
    // Review Focus #1：自环与长链成环均拒绝
    let c = setup("cycle").await;
    let ids = &c.node_ids;
    // 链：root→1→2→3
    c.store
        .set_lines(
            ids[ROOT],
            vec![LineInput {
                line_no: 1,
                child: ids[1],
                qty: 1,
            }],
        )
        .await
        .unwrap();
    c.store
        .set_lines(
            ids[1],
            vec![LineInput {
                line_no: 1,
                child: ids[2],
                qty: 1,
            }],
        )
        .await
        .unwrap();
    c.store
        .set_lines(
            ids[2],
            vec![LineInput {
                line_no: 1,
                child: ids[3],
                qty: 1,
            }],
        )
        .await
        .unwrap();

    // 长链成环：3→root（root 已可达 3）
    let e = c
        .store
        .set_lines(
            ids[3],
            vec![LineInput {
                line_no: 1,
                child: ids[ROOT],
                qty: 1,
            }],
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-BOM-CYCLE");

    // 自环：1→1（1 已可达自身，深度 0 行存在）
    let e = c
        .store
        .set_lines(
            ids[1],
            vec![LineInput {
                line_no: 2,
                child: ids[1],
                qty: 1,
            }],
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-BOM-CYCLE");
    c.drop_it().await;
}

#[tokio::test]
async fn delta_maintain_matches_full_rebuild() {
    // Review Focus #2：改一行后增量闭包 == 全量重建对拍
    let c = setup("delta").await;
    let ids = &c.node_ids;
    c.store
        .set_lines(
            ids[ROOT],
            vec![
                LineInput {
                    line_no: 1,
                    child: ids[1],
                    qty: 2,
                },
                LineInput {
                    line_no: 2,
                    child: ids[2],
                    qty: 3,
                },
            ],
        )
        .await
        .unwrap();
    c.store
        .set_lines(
            ids[1],
            vec![LineInput {
                line_no: 1,
                child: ids[10],
                qty: 4,
            }],
        )
        .await
        .unwrap();

    // 改 root 的 line 1：qty 2→5（子树整体乘数变化）
    c.store
        .set_lines(
            ids[ROOT],
            vec![
                LineInput {
                    line_no: 1,
                    child: ids[1],
                    qty: 5,
                },
                LineInput {
                    line_no: 2,
                    child: ids[2],
                    qty: 3,
                },
            ],
        )
        .await
        .unwrap();

    let closure = c.store.expand(ids[ROOT], ExpandMode::Full).await.unwrap();
    let cte = c.cte_expand(ids[ROOT]).await;
    assert_eq!(closure.len(), cte.len());
    for row in &closure {
        assert_eq!(row.qty_total, cte[&row.child], "增量后与全量对拍一致");
    }
    // 特值：ids[10] 经 1 号路径 = 5*4=20
    let ten = closure.iter().find(|r| r.child == ids[10]).unwrap();
    assert_eq!(ten.qty_total, 20);
    c.drop_it().await;
}

#[tokio::test]
async fn dag_shared_child_quantity_summed() {
    // Review Focus #3：借用共享（多父）数量求和
    let c = setup("dag").await;
    let ids = &c.node_ids;
    // 共享子 10：root→1(qty2)→10(qty3) 与 root→2(qty1)→10(qty5)
    c.store
        .set_lines(
            ids[ROOT],
            vec![
                LineInput {
                    line_no: 1,
                    child: ids[1],
                    qty: 2,
                },
                LineInput {
                    line_no: 2,
                    child: ids[2],
                    qty: 1,
                },
            ],
        )
        .await
        .unwrap();
    c.store
        .set_lines(
            ids[1],
            vec![LineInput {
                line_no: 1,
                child: ids[10],
                qty: 3,
            }],
        )
        .await
        .unwrap();
    c.store
        .set_lines(
            ids[2],
            vec![LineInput {
                line_no: 1,
                child: ids[10],
                qty: 5,
            }],
        )
        .await
        .unwrap();

    let full = c.store.expand(ids[ROOT], ExpandMode::Full).await.unwrap();
    let ten = full.iter().find(|r| r.child == ids[10]).unwrap();
    assert_eq!(ten.qty_total, 11, "共享子数量必须求和（6+5=11）");
    c.drop_it().await;
}

#[tokio::test]
async fn where_used_reports_all_ancestors() {
    let c = setup("wu").await;
    let ids = &c.node_ids;
    c.store
        .set_lines(
            ids[ROOT],
            vec![LineInput {
                line_no: 1,
                child: ids[1],
                qty: 1,
            }],
        )
        .await
        .unwrap();
    c.store
        .set_lines(
            ids[1],
            vec![LineInput {
                line_no: 1,
                child: ids[2],
                qty: 2,
            }],
        )
        .await
        .unwrap();

    let used = c.store.where_used(ids[2]).await.unwrap();
    assert!(
        used.iter().any(|(a, q)| *a == ids[ROOT] && *q == 2),
        "root 反查命中"
    );
    assert!(used.iter().any(|(a, _)| *a == ids[1]), "直接父反查命中");
    c.drop_it().await;
}

// ==================== 终审修复钉子（C1/I1 收敛子树 + M10） ====================

#[tokio::test]
async fn converging_subtrees_delta_matches_full() {
    // 终审 C1/I1：DAG 收敛（同 parent 同 child 不同行号不同 qty；两条路径同深度收敛 D）
    // 全量替换前（旧结构）：P→C(qty2)、P→C2(qty3)；C→D(qty1)、C2→D(qty1)
    // → D 经两条路径同深度 depth=2 收敛于 P，qty = 2+3 = 5
    // 全量替换 P→[C(qty1)] 后：旧 D 贡献须整扣（残留即 C1 腐蚀）
    let c = setup("conv").await;
    let ids = &c.node_ids;
    let p = ids[0];
    let cc = ids[1];
    let c2 = ids[2];
    let d = ids[3];

    c.store
        .set_lines(
            p,
            vec![
                LineInput {
                    line_no: 1,
                    child: cc,
                    qty: 2,
                },
                LineInput {
                    line_no: 2,
                    child: c2,
                    qty: 3,
                },
            ],
        )
        .await
        .unwrap();
    c.store
        .set_lines(
            cc,
            vec![LineInput {
                line_no: 1,
                child: d,
                qty: 1,
            }],
        )
        .await
        .unwrap();
    c.store
        .set_lines(
            c2,
            vec![LineInput {
                line_no: 1,
                child: d,
                qty: 1,
            }],
        )
        .await
        .unwrap();

    let full = c.store.expand(p, ExpandMode::Full).await.unwrap();
    let d_row = full.iter().find(|r| r.child == d).unwrap();
    assert_eq!(d_row.qty_total, 5, "收敛前 D = 2*1 + 3*1 = 5");

    // 全量替换：只留 C(qty1)——D 应变为 1（旧 C2/C 双路径扣净）
    c.store
        .set_lines(
            p,
            vec![LineInput {
                line_no: 1,
                child: cc,
                qty: 1,
            }],
        )
        .await
        .unwrap();

    let full = c.store.expand(p, ExpandMode::Full).await.unwrap();
    let cte = c.cte_expand(p).await;
    assert_eq!(full.len(), cte.len(), "替换后节点集一致");
    for row in &full {
        assert_eq!(
            row.qty_total, cte[&row.child],
            "替换后数量与 CTE 一致（终审 C1 钉）"
        );
    }
    let d_row = full.iter().find(|r| r.child == d).unwrap();
    assert_eq!(d_row.qty_total, 1);
    assert!(
        full.iter().find(|r| r.child == c2).is_none(),
        "被移除子件不得残留"
    );
    c.drop_it().await;
}

#[tokio::test]
async fn same_child_multi_lines_same_parent_delta() {
    // 终审 C1 最简复现：同 parent 两行同 child 不同 qty，随后全量替换为不含该 child
    let c = setup("samechild").await;
    let ids = &c.node_ids;
    let p = ids[0];
    let leaf = ids[10];
    c.store
        .set_lines(
            p,
            vec![
                LineInput {
                    line_no: 1,
                    child: leaf,
                    qty: 2,
                },
                LineInput {
                    line_no: 2,
                    child: leaf,
                    qty: 3,
                },
            ],
        )
        .await
        .unwrap();
    let full = c.store.expand(p, ExpandMode::Full).await.unwrap();
    let l = full.iter().find(|r| r.child == leaf).unwrap();
    assert_eq!(l.qty_total, 5, "多行同 child 先求和");

    // 替换为不含 leaf 的行
    c.store
        .set_lines(
            p,
            vec![LineInput {
                line_no: 1,
                child: ids[20],
                qty: 1,
            }],
        )
        .await
        .unwrap();
    let full = c.store.expand(p, ExpandMode::Full).await.unwrap();
    assert!(
        full.iter().find(|r| r.child == leaf).is_none(),
        "旧 child 必须被扣净（终审 C1）"
    );
    c.drop_it().await;
}

#[tokio::test]
async fn line_no_must_be_positive() {
    // 终审 M10
    let c = setup("lineno").await;
    let e = c
        .store
        .set_lines(
            c.node_ids[0],
            vec![LineInput {
                line_no: 0,
                child: c.node_ids[1],
                qty: 1,
            }],
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-BAD-REQUEST");
    c.drop_it().await;
}

#[tokio::test]
async fn set_lines_rejected_when_parent_not_working() {
    // 增量8 Task3：released/review parent 的 BOM 不可静默污染（423 ObjStateForbidden）
    let c = setup("sguard").await;
    let parent = c.node_ids[ROOT];
    let lines = || {
        vec![LineInput {
            line_no: 1,
            child: c.node_ids[1],
            qty: 2,
        }]
    };

    for state in ["released", "review", "obsolete"] {
        sqlx::query("UPDATE cm_object SET state=$2 WHERE id=$1")
            .bind(parent)
            .bind(state)
            .execute(&c.pool)
            .await
            .unwrap();
        let e = c.store.set_lines(parent, lines()).await.unwrap_err();
        assert_eq!(e.code(), "MDL-OBJ-STATE-FORBIDDEN", "state={state}");
        assert_eq!(e.http_status(), 423, "state={state}");
    }

    // working 正常（回归钉）
    sqlx::query("UPDATE cm_object SET state='working' WHERE id=$1")
        .bind(parent)
        .execute(&c.pool)
        .await
        .unwrap();
    c.store
        .set_lines(parent, lines())
        .await
        .expect("working parent 必须可编辑");
    c.drop_it().await;
}

// ==================== BOM 版本链（增量9 Task1：快照冻结 + as-of 展开） ====================

use std::collections::HashMap as _HashMap;

/// 建 live 边：root->a(2), a->b(3), root->b(1) —— b 经两条路径 qty=7
async fn seed_deep(c: &Ctx) -> (Uuid, Uuid, Uuid) {
    let (root, a, b) = (c.node_ids[0], c.node_ids[1], c.node_ids[2]);
    c.store
        .set_lines(
            root,
            vec![
                LineInput { line_no: 1, child: a, qty: 2 },
                LineInput { line_no: 2, child: b, qty: 1 },
            ],
        )
        .await
        .unwrap();
    c.store
        .set_lines(a, vec![LineInput { line_no: 1, child: b, qty: 3 }])
        .await
        .unwrap();
    (root, a, b)
}

#[tokio::test]
async fn snapshot_freezes_idempotent_and_asof_matches_live() {
    // Review Focus #2：同边集下快照 CTE 与 live expand 三模式对拍
    let c = setup("vsnap").await;
    let (root, a, b) = seed_deep(&c).await;

    let mut tx = c.pool.begin().await.unwrap();
    let n = c.store.snapshot_live_edges_tx(&mut *tx, root, 1).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(n, 3, "快照须落全部 3 条边");

    // 幂等：同 major 重快照 → 0 新增
    let mut tx = c.pool.begin().await.unwrap();
    let n2 = c.store.snapshot_live_edges_tx(&mut *tx, root, 1).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(n2, 0, "ON CONFLICT 幂等");

    // bom_release 行存在（无 BOM 空集也须有——asof 守卫依据）
    let rel: i64 = sqlx::query_scalar("SELECT count(*) FROM bom_release WHERE parent=$1 AND major=1")
        .bind(root)
        .fetch_one(&c.pool)
        .await
        .unwrap();
    assert_eq!(rel, 1);

    // 三模式对拍：asof(major=1) == live
    for mode in [ExpandMode::Full, ExpandMode::LeafOnly, ExpandMode::OneLevel] {
        let live: HashMap<Uuid, i64> = c
            .store
            .expand(root, mode)
            .await
            .unwrap()
            .into_iter()
            .map(|r| (r.child, r.qty_total))
            .collect();
        let asof: HashMap<Uuid, i64> = c
            .store
            .expand_asof(root, 1, mode)
            .await
            .unwrap()
            .into_iter()
            .map(|r| (r.child, r.qty_total))
            .collect();
        assert_eq!(live, asof, "mode={mode:?} 对拍失配");
    }
    let full = c.store.expand_asof(root, 1, ExpandMode::Full).await.unwrap();
    let b = full.iter().find(|r| r.child == b).unwrap();
    assert_eq!(b.qty_total, 7, "双路径聚合 2*3+1");
    let leaf = c.store.expand_asof(root, 1, ExpandMode::LeafOnly).await.unwrap();
    assert_eq!(leaf.len(), 1, "仅 b 是叶子");
    let one = c.store.expand_asof(root, 1, ExpandMode::OneLevel).await.unwrap();
    assert_eq!(one.len(), 2, "一层= a+b");
    let _ = _HashMap::<Uuid, i64>::new();
    c.drop_it().await;
}

#[tokio::test]
async fn asof_isolated_from_live_edits() {
    // Review Focus #2 隔离钉：改 live 不影响冻结快照
    let c = setup("viso").await;
    let (root, a, _b) = seed_deep(&c).await;
    let mut tx = c.pool.begin().await.unwrap();
    c.store.snapshot_live_edges_tx(&mut *tx, root, 1).await.unwrap();
    tx.commit().await.unwrap();

    // v2 工作区：live 替换为 a(qty9)
    c.store
        .set_lines(root, vec![LineInput { line_no: 1, child: a, qty: 9 }])
        .await
        .unwrap();
    let live = c.store.expand(root, ExpandMode::OneLevel).await.unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].qty_total, 9);

    let asof = c.store.expand_asof(root, 1, ExpandMode::OneLevel).await.unwrap();
    assert_eq!(asof.len(), 2, "冻结快照不得被 live 编辑污染");
    let sum: i64 = asof.iter().map(|r| r.qty_total).sum();
    assert_eq!(sum, 3, "原 2+1");
    c.drop_it().await;
}

#[tokio::test]
async fn expand_asof_unreleased_major_is_400() {
    // Review Focus #3：未发布 major → 400 MDL-BAD-REQUEST
    let c = setup("v400").await;
    let (root, a, _b) = seed_deep(&c).await;
    let e = c
        .store
        .expand_asof(root, 9, ExpandMode::Full)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "MDL-BAD-REQUEST");
    let _ = a;
    c.drop_it().await;
}

#[tokio::test]
async fn empty_bom_release_still_asof_expands_empty() {
    // 无 BOM 边的对象发布：bom_release 落行、asof 返回空集（非 400）
    let c = setup("vempty").await;
    let root = c.node_ids[5];
    let mut tx = c.pool.begin().await.unwrap();
    let n = c.store.snapshot_live_edges_tx(&mut *tx, root, 1).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(n, 0);
    let rows = c.store.expand_asof(root, 1, ExpandMode::Full).await.unwrap();
    assert!(rows.is_empty());
    c.drop_it().await;
}

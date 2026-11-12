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

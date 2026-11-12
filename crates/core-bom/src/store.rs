//! BomStore：结构写（单事务：循环校验→物化扣减→替换边→闭包扣减/累加）与读（闭包表三模式/反查）。
//! 算法移植自 spikes/bom-spike（V-2：增量维护与全量对拍 100% 一致，143ms/2000 边）。

use core_model::{DomainError, Result};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

/// BOM 行输入（set_lines 全量替换 parent 的直接子集合）
#[derive(Debug, Clone, serde::Deserialize)]
pub struct LineInput {
    pub line_no: i32,
    pub child: Uuid,
    pub qty: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ExpandRow {
    pub child: Uuid,
    pub qty_total: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExpandMode {
    /// 全部后代（与递归 CTE 对拍一致）
    Full,
    /// 仅叶子（无子边）
    LeafOnly,
    /// 直接一层
    OneLevel,
}

pub struct BomStore {
    pool: PgPool,
}

impl BomStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 全量替换 parent 的直接子集合，并增量维护闭包（单事务）。
    /// 循环拒绝：任一新边 child 已可达 parent（含自环）→ MDL-BOM-CYCLE。
    pub async fn set_lines(&self, parent: Uuid, lines: Vec<LineInput>) -> Result<()> {
        // 行号唯一 / 数量正 / 自环即时拒
        let mut seen = std::collections::HashSet::new();
        for l in &lines {
            if !seen.insert(l.line_no) {
                return Err(DomainError::BadRequest(format!(
                    "line_no 重复: {}",
                    l.line_no
                )));
            }
            if l.qty <= 0 {
                return Err(DomainError::BadRequest("qty 必须为正数".into()));
            }
            if l.line_no < 1 {
                return Err(DomainError::BadRequest("line_no 须 ≥ 1".into())); // 终审 M10
            }
            if l.child == parent {
                return Err(DomainError::BomCycle);
            }
        }

        let mut tx = self.pool.begin().await?;

        // 终审 I2：并发造环防护——锁 parent + 全部 child（按 id 排序防死锁），
        // 锁定窗口内做 child⇝parent 校验（READ COMMITTED 下判定落在锁定集上）
        let mut node_ids: Vec<Uuid> = std::iter::once(parent)
            .chain(lines.iter().map(|l| l.child))
            .collect();
        node_ids.sort();
        node_ids.dedup();
        sqlx::query("SELECT id FROM cm_object WHERE id = ANY($1) ORDER BY id FOR UPDATE")
            .bind(&node_ids)
            .execute(&mut *tx)
            .await?;
        // 增量8 Task3：BOM 编辑仅限 working parent——锁后查态（锁定窗口内无 revise 竞态漏网）
        let pstate: String = sqlx::query_scalar("SELECT state FROM cm_object WHERE id = $1")
            .bind(parent)
            .fetch_one(&mut *tx)
            .await?;
        if pstate != "working" {
            return Err(DomainError::ObjStateForbidden(format!(
                "bom parent state={pstate}"
            )));
        }
        for l in &lines {
            let reachable: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM bom_path WHERE ancestor = $1 AND descendant = $2",
            )
            .bind(l.child)
            .bind(parent)
            .fetch_one(&mut *tx)
            .await?;
            if reachable > 0 {
                return Err(DomainError::BomCycle);
            }
        }

        // 1) 物化扣减集：旧边贡献按 (a,d,dep) **先求和再扣**（终审 C1：UPDATE..FROM 未聚合静默腐蚀）
        //    I1 同根因：累加侧同键多行也会触发 PG 21000，聚合一并消解
        sqlx::query(
            "UPDATE bom_path p \
             SET qty_path = p.qty_path - dd.q \
             FROM ( \
               SELECT src.a, src.d, src.dep, SUM(src.q) AS q \
               FROM ( \
                 SELECT pa.ancestor AS a, po.descendant AS d, pa.depth + 1 + po.depth AS dep, \
                        pa.qty_path * e.qty * po.qty_path AS q \
                 FROM bom_path pa \
                 JOIN bom_edge e ON e.parent = $1 \
                 JOIN bom_path po ON po.ancestor = e.child \
                 WHERE pa.descendant = $1 \
               ) src \
               GROUP BY src.a, src.d, src.dep \
             ) dd \
             WHERE p.ancestor = dd.a AND p.descendant = dd.d AND p.depth = dd.dep",
        )
        .bind(parent)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM bom_path WHERE qty_path <= 0")
            .execute(&mut *tx)
            .await?;

        // 2) 替换边
        sqlx::query("DELETE FROM bom_edge WHERE parent = $1")
            .bind(parent)
            .execute(&mut *tx)
            .await?;
        // 终审 M1：child 存在性预校验（FK 23503 → 语义化 404）
        for l in &lines {
            let exists: Option<i32> =
                sqlx::query_scalar("SELECT 1 FROM cm_object WHERE id = $1 AND deleted_at IS NULL")
                    .bind(l.child)
                    .fetch_optional(&mut *tx)
                    .await?;
            if exists.is_none() {
                return Err(DomainError::ObjNotFound(format!("child {}", l.child)));
            }
            sqlx::query("INSERT INTO bom_edge(parent, line_no, child, qty) VALUES ($1,$2,$3,$4)")
                .bind(parent)
                .bind(l.line_no)
                .bind(l.child)
                .bind(l.qty)
                .execute(&mut *tx)
                .await?;
        }

        // 3) 确保节点自身行（(X,X,0,1)——CROSS JOIN 的起点；V-2 冷启动修复）
        for n in std::iter::once(parent).chain(lines.iter().map(|l| l.child)) {
            sqlx::query(
                "INSERT INTO bom_path(ancestor, descendant, depth, qty_path)                  VALUES ($1, $1, 0, 1)                  ON CONFLICT (ancestor, descendant, depth) DO NOTHING",
            )
            .bind(n)
            .execute(&mut *tx)
            .await?;
        }

        // 4) 累加新边贡献：语句内同键多行先 SUM（终审 I1：PG 21000 硬错消解）
        for l in &lines {
            sqlx::query(
                "INSERT INTO bom_path(ancestor, descendant, depth, qty_path) \
                 SELECT agg.a, agg.d, agg.dep, SUM(agg.q) \
                 FROM ( \
                   SELECT pa.ancestor AS a, pn.descendant AS d, pa.depth + 1 + pn.depth AS dep, \
                          pa.qty_path * $3 * pn.qty_path AS q \
                   FROM bom_path pa \
                   CROSS JOIN bom_path pn \
                   WHERE pa.descendant = $1 AND pn.ancestor = $2 \
                 ) agg \
                 GROUP BY agg.a, agg.d, agg.dep \
                 ON CONFLICT (ancestor, descendant, depth) \
                 DO UPDATE SET qty_path = bom_path.qty_path + EXCLUDED.qty_path",
            )
            .bind(parent)
            .bind(l.child)
            .bind(l.qty)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// 展开（读闭包表；Full 排除 root 自身行 depth=0）
    pub async fn expand(&self, parent: Uuid, mode: ExpandMode) -> Result<Vec<ExpandRow>> {
        match mode {
            ExpandMode::Full | ExpandMode::LeafOnly => {
                let leaf_filter = if mode == ExpandMode::LeafOnly {
                    "AND NOT EXISTS (SELECT 1 FROM bom_edge e WHERE e.parent = p.descendant)"
                } else {
                    ""
                };
                let rows: Vec<ExpandRow> = sqlx::query_as(&format!(
                    "SELECT p.descendant AS child, SUM(p.qty_path)::bigint AS qty_total \
                         FROM bom_path p \
                         WHERE p.ancestor = $1 AND p.descendant <> p.ancestor {leaf_filter} \
                         GROUP BY p.descendant \
                         ORDER BY child"
                ))
                .bind(parent)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows)
            }
            ExpandMode::OneLevel => {
                let rows: Vec<ExpandRow> = sqlx::query_as(
                    "SELECT child, SUM(qty)::bigint AS qty_total \
                     FROM bom_edge WHERE parent = $1 GROUP BY child ORDER BY child",
                )
                .bind(parent)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows)
            }
        }
    }

    /// Where-used：(祖先, 经所有路径数量和)
    pub async fn where_used(&self, child: Uuid) -> Result<Vec<(Uuid, i64)>> {
        let rows: Vec<(Uuid, i64)> = sqlx::query_as(
            "SELECT ancestor, SUM(qty_path)::bigint FROM bom_path \
             WHERE descendant = $1 AND ancestor <> $1 \
             GROUP BY ancestor",
        )
        .bind(child)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

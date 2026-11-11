//! V-2 BOM 闭包表最小原型（概设 §9 / ADR-0003 验证）
//!
//! 验收标准：
//!  ① 随机 10 万行 BOM 构建 + 闭包生成 < 60s
//!  ② 全量展开 < 1s（P95，含行属性聚合）
//!  ③ 子树替换的闭包增量维护 < 2s
//!  ④ 展开结果与递归 CTE 对拍 100% 一致
//!  ⑤ 循环引用正确拒绝
//!
//! 运行：DATABASE_URL=postgres://user:pass@localhost/rust_plm_spike cargo run -r -p bom-spike

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rand::prelude::IndexedRandom;
use sqlx::postgres::{PgPoolCopyExt, PgPoolOptions};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::Instant;

const TARGET_EDGES: usize = 100_000;
const MAX_DEPTH: usize = 10;
const SAMPLES: usize = 20;

fn p95(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((v.len() as f64 - 1.0) * 0.95).round() as usize;
    v[idx.min(v.len() - 1)]
}

type Paths = BTreeMap<i64, Vec<(i64 /*desc*/, i32 /*depth*/, i64 /*qty_path*/)>>;

/// 图数据（Rust 侧）
struct Graph {
    children: HashMap<i64, Vec<(i64, i64)>>, // parent -> [(child, qty)]
    edges: Vec<(i64, i32, i64, i64)>,        // (parent, line_no, child, qty)
}

fn gen_graph(rng: &mut StdRng) -> (Graph, HashMap<i64, usize>, i64) {
    let mut children: HashMap<i64, Vec<(i64, i64)>> = HashMap::new();
    let mut edges = Vec::with_capacity(TARGET_EDGES + 16);
    let mut depth_of: HashMap<i64, usize> = HashMap::new();
    depth_of.insert(1, 0);
    let mut queue: VecDeque<(i64, usize)> = VecDeque::new();
    queue.push_back((1, 0));
    let mut next_id: i64 = 2;
    // 已完成节点（子已全部生成），按深度分桶，供共享引用（只允许指向更深层 → 分层图无环）
    let mut done_by_depth: Vec<Vec<i64>> = vec![Vec::new(); MAX_DEPTH + 1];
    let mut left = TARGET_EDGES;
    while let Some((node, depth)) = queue.pop_front() {
        if left == 0 || depth >= MAX_DEPTH {
            done_by_depth[depth.min(MAX_DEPTH)].push(node);
            continue;
        }
        let max_c = left.min(8);
        let c = rng.random_range(2..=max_c.max(2));
        for line in 1..=c as i32 {
            left -= 1;
            // 5% 概率复用"更深层已完成节点"（借用件 → DAG）
            let mut child = None;
            if rng.random_bool(0.05) {
                let lo = depth + 1;
                let cands: Vec<&Vec<i64>> = done_by_depth[lo..=MAX_DEPTH].iter()
                    .filter(|v| !v.is_empty()).collect();
                if let Some(bucket) = cands.choose(&mut *rng) {
                    child = Some(bucket[rng.random_range(0..bucket.len())]);
                }
            }
            let child = match child {
                Some(c) => c,
                None => {
                    let id = next_id;
                    next_id += 1;
                    depth_of.insert(id, depth + 1);
                    queue.push_back((id, depth + 1));
                    id
                }
            };
            let qty = rng.random_range(1..=3);
            children.entry(node).or_default().push((child, qty));
            edges.push((node, line, child, qty));
        }
        done_by_depth[depth.min(MAX_DEPTH)].push(node);
    }
    (Graph { children, edges }, depth_of, next_id)
}

/// 后序递归 + memo：node 的全部路径闭包（含自身 depth=0 qty=1），(desc,depth) 相同则数量求和
fn build_paths(node: i64, g: &Graph, memo: &mut Paths) -> Vec<(i64, i32, i64)> {
    if let Some(v) = memo.get(&node) {
        return v.clone();
    }
    let mut m: BTreeMap<(i64, i32), i64> = BTreeMap::new();
    m.insert((node, 0), 1);
    if let Some(kids) = g.children.get(&node) {
        for &(child, qty) in kids {
            let sub = build_paths(child, g, memo);
            for (d, dep, qp) in sub {
                *m.entry((d, dep + 1)).or_insert(0) += qty * qp;
            }
        }
    }
    let v: Vec<_> = m.into_iter().map(|((d, dep), q)| (d, dep, q)).collect();
    memo.insert(node, v.clone());
    v
}

/// 子树边数（DAG，visited 防重复）
fn subtree_edges(root: i64, g: &Graph) -> usize {
    let mut seen = HashSet::new();
    let mut stack = vec![root];
    let mut n = 0;
    while let Some(x) = stack.pop() {
        if !seen.insert(x) { continue; }
        if let Some(kids) = g.children.get(&x) {
            for &(c, _) in kids { n += 1; stack.push(c); }
        }
    }
    n
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL").expect("env DATABASE_URL");
    let pool = PgPoolOptions::new().max_connections(8).connect(&url).await?;
    println!("== V-2 BOM 闭包表最小原型 ==");

    // ---------- [1/6] 建表 ----------
    sqlx::raw_sql("DROP TABLE IF EXISTS bom_path; DROP TABLE IF EXISTS bom_edge;").execute(&pool).await?;
    sqlx::raw_sql(
        r#"
        CREATE TABLE bom_edge(
          parent bigint NOT NULL, line_no int NOT NULL,
          child bigint NOT NULL, qty bigint NOT NULL,
          PRIMARY KEY(parent, line_no)
        );
        CREATE INDEX idx_edge_child ON bom_edge(child);
        CREATE TABLE bom_path(
          ancestor bigint NOT NULL, descendant bigint NOT NULL, depth int NOT NULL,
          qty_path bigint NOT NULL,          -- 该(ancestor,descendant,depth)全部路径数量乘积之和
          PRIMARY KEY(ancestor, descendant, depth)
        );
        CREATE INDEX idx_path_desc ON bom_path(descendant);
        "#,
    ).execute(&pool).await?;
    println!("[1/6] 建表（bom_edge / bom_path 闭包表）............ OK");

    // ---------- [2/6] 生成 10 万边 BOM + 闭包计算 + COPY（验收①） ----------
    let t0 = Instant::now();
    let mut rng = StdRng::seed_from_u64(2026);
    let (g, _depth_of, mut next_id) = gen_graph(&mut rng);
    let mut memo = Paths::new();
    build_paths(1, &g, &mut memo);
    let path_rows: usize = memo.values().map(|v| v.len()).sum();
    let nodes = memo.len();
    println!("      生成图：{} 边 / {} 节点（深度≤{MAX_DEPTH}，含借用共享）；闭包 {} 行",
        g.edges.len(), nodes, path_rows);
    assert!(g.edges.len() >= TARGET_EDGES, "边数应达到目标");

    // COPY edges
    {
        let mut c = pool.copy_in_raw(
            "COPY bom_edge(parent, line_no, child, qty) FROM STDIN WITH (FORMAT csv)").await?;
        let mut buf = Vec::with_capacity(g.edges.len() * 40);
        for (p, l, ch, q) in &g.edges {
            buf.extend_from_slice(format!("{p},{l},{ch},{q}\n").as_bytes());
        }
        c.send(buf).await?;
        let n = c.finish().await?;
        assert_eq!(n as usize, g.edges.len());
    }
    // COPY paths
    {
        let mut c = pool.copy_in_raw(
            "COPY bom_path(ancestor, descendant, depth, qty_path) FROM STDIN WITH (FORMAT csv)").await?;
        let mut buf = Vec::with_capacity(path_rows * 44);
        for (anc, rows) in &memo {
            for (d, dep, q) in rows {
                buf.extend_from_slice(format!("{anc},{d},{dep},{q}\n").as_bytes());
            }
        }
        c.send(buf).await?;
        let n = c.finish().await?;
        assert_eq!(n as usize, path_rows);
    }
    sqlx::raw_sql("ANALYZE bom_edge; ANALYZE bom_path;").execute(&pool).await?;
    let build_secs = t0.elapsed().as_secs_f64();
    println!("[2/6] 构建总耗时（生成+闭包计算+COPY+ANALYZE）：{build_secs:.1}s（验收 <60s）");

    // ---------- [3/6] 展开基准（验收②） ----------
    // Full 展开
    let sql_full = "SELECT p.descendant, COUNT(*)::bigint, SUM(p.qty_path)::bigint \
                    FROM bom_path p WHERE p.ancestor = 1 AND p.descendant <> p.ancestor \
                    GROUP BY p.descendant";
    let plan: String = sqlx::query_scalar::<_, String>(&format!("EXPLAIN (FORMAT TEXT) {sql_full}"))
        .fetch_all(&pool).await?.join(" | ");
    let mut xs = Vec::new();
    let mut full_rows = 0usize;
    for _ in 0..SAMPLES {
        let t = Instant::now();
        let rows: Vec<(i64, i64, i64)> = sqlx::query_as(sql_full).fetch_all(&pool).await?;
        full_rows = rows.len();
        xs.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let full_p95 = p95(xs);
    println!("[3/6] 全量展开：{full_rows} 个后代节点，P95 = {full_p95:.1} ms（验收 <1000ms）");
    let short: String = plan.chars().take(110).collect();
    println!("      计划：{short}…");

    // LeafOnly
    sqlx::raw_sql("DROP TABLE IF EXISTS t_leaf; \
                 CREATE TABLE t_leaf AS \
                 SELECT DISTINCT p.descendant FROM bom_path p WHERE p.ancestor = 1 \
                 AND NOT EXISTS (SELECT 1 FROM bom_edge e WHERE e.parent = p.descendant)")
        .execute(&pool).await?;
    let sql_leaf = "SELECT p.descendant, SUM(p.qty_path)::bigint FROM bom_path p JOIN t_leaf USING(descendant) \
                    WHERE p.ancestor = 1 GROUP BY p.descendant";
    let mut xs = Vec::new(); let mut leaf_rows = 0;
    for _ in 0..SAMPLES {
        let t = Instant::now();
        let rows: Vec<(i64, i64)> = sqlx::query_as(sql_leaf).fetch_all(&pool).await?;
        leaf_rows = rows.len();
        xs.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let leaf_p95 = p95(xs);
    println!("      尾级展开（LeafOnly）：{leaf_rows} 叶件，P95 = {leaf_p95:.1} ms");

    // Where-used：选一个中间件（有父有子）
    let mid: i64 = sqlx::query_scalar(
        "SELECT p.descendant FROM bom_path p WHERE p.ancestor = 1 AND p.depth = 5 LIMIT 1")
        .fetch_one(&pool).await?;
    let sql_wu = "SELECT ancestor, COUNT(*)::bigint, SUM(qty_path)::bigint FROM bom_path \
                  WHERE descendant = $1 GROUP BY ancestor";
    let mut xs = Vec::new(); let mut wu_rows = 0;
    for _ in 0..SAMPLES {
        let t = Instant::now();
        let rows: Vec<(i64, i64, i64)> = sqlx::query_as(sql_wu).bind(mid).fetch_all(&pool).await?;
        wu_rows = rows.len();
        xs.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!("      反查 Where-used（节点 {mid}）：{wu_rows} 个使用处，P95 = {:.1} ms", p95(xs));

    // ---------- [4/6] 递归 CTE 对拍（验收④） ----------
    const SQL_CMP: &str = "SELECT descendant, SUM(qty_path)::bigint FROM bom_path \
                           WHERE ancestor=1 AND descendant<>1 GROUP BY descendant";
    let t = Instant::now();
    let cte: Vec<(i64, i64)> = sqlx::query_as(
        "WITH RECURSIVE t(child, qty) AS ( \
           SELECT child, qty FROM bom_edge WHERE parent = 1 \
           UNION ALL \
           SELECT e.child, t.qty * e.qty FROM bom_edge e JOIN t ON e.parent = t.child \
         ) SELECT child, SUM(qty)::bigint FROM t GROUP BY child")
        .fetch_all(&pool).await?;
    let cte_secs = t.elapsed().as_secs_f64();
    let closure_m: HashMap<i64, i64> = sqlx::query_as::<_, (i64, i64)>(SQL_CMP)
        .fetch_all(&pool).await?.into_iter().collect();
    let cte_m: HashMap<i64, i64> = cte.into_iter().collect();
    let same = closure_m.len() == cte_m.len()
        && closure_m.iter().all(|(k, v)| cte_m.get(k) == Some(v));
    println!("[4/6] 对拍：闭包 {} 行 vs 递归 CTE {} 行 → {}（CTE 耗时 {cte_secs:.2}s vs 闭包 {full_p95:.0}ms，约 {:.0}x）",
        closure_m.len(), cte_m.len(), if same { "100% 一致 ✔" } else { "不一致 ✘" },
        if full_p95 > 0.0 { cte_secs * 1000.0 / full_p95 } else { 0.0 });
    assert!(same, "闭包展开必须与递归 CTE 一致");

    // ---------- [5/6] 增量维护：替换子树（验收③） ----------
    // 选 P（深度 2、非叶）使其第一个孩子 old 的子树边数适中且 old 仅被 P 引用（独占）
    let cands: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT p.descendant FROM bom_path p WHERE p.ancestor=1 AND p.depth=2")
        .fetch_all(&pool).await?;
    let mut chosen: Option<(i64, i32, i64)> = None; // (P, line_no, old)
    for p in cands {
        let kids: Vec<(i32, i64)> = sqlx::query_as(
            "SELECT line_no, child FROM bom_edge WHERE parent=$1 ORDER BY line_no LIMIT 1")
            .bind(p).fetch_all(&pool).await?;
        let Some((line, old)) = kids.first().copied() else { continue; };
        let indeg: i64 = sqlx::query_scalar("SELECT count(DISTINCT parent) FROM bom_edge WHERE child=$1")
            .bind(old).fetch_one(&pool).await?;
        if indeg != 1 { continue; }
        let sz = subtree_edges(old, &g);
        if (500..=5000).contains(&sz) {
            chosen = Some((p, line, old));
            println!("      选中：P={p} 第 {line} 行 old={old}（子树 {sz} 边，独占引用）");
            break;
        }
    }
    let Some((p, line_no, old)) = chosen else {
        panic!("未找到合适的替换目标（P/old）");
    };

    // 预生成替换子树 new_root（2000 边），Rust 侧闭包
    let new_root = next_id; next_id += 1;
    let new_g = gen_small_tree(&mut rng, new_root, 2000, &mut next_id);
    let mut new_memo = Paths::new();
    build_paths(new_root, &new_g, &mut new_memo);
    let new_path_rows = new_memo.get(&new_root).map(|v| v.len()).unwrap_or(0);
    let new_edges: usize = new_g.edges.len();

    let mut tx = pool.begin().await?;
    let t = Instant::now();
    {
        // 1) 新子树边与自身闭包入库
        let mut c = tx.copy_in_raw(
            "COPY bom_edge(parent, line_no, child, qty) FROM STDIN WITH (FORMAT csv)").await?;
        let mut buf = Vec::new();
        for (pp, l, ch, q) in &new_g.edges {
            buf.extend_from_slice(format!("{pp},{l},{ch},{q}\n").as_bytes());
        }
        c.send(buf).await?; c.finish().await?;
        let mut c = tx.copy_in_raw(
            "COPY bom_path(ancestor, descendant, depth, qty_path) FROM STDIN WITH (FORMAT csv)").await?;
        let mut buf = Vec::new();
        for (anc, rows) in &new_memo {
            for (d, dep, q) in rows { buf.extend_from_slice(format!("{anc},{d},{dep},{q}\n").as_bytes()); }
        }
        c.send(buf).await?; c.finish().await?;
        // 2) 改边
        sqlx::query("UPDATE bom_edge SET child=$1 WHERE parent=$2 AND line_no=$3")
            .bind(new_root).bind(p).bind(line_no).execute(&mut *tx).await?;
        // 3) 扣除 old 子树经 P 边的全部贡献（集合化 SQL）
        sqlx::query(
            "CREATE TEMP TABLE dd ON COMMIT DROP AS \
             SELECT pa.ancestor AS a, po.descendant AS d, pa.depth + 1 + po.depth AS dep, \
                    pa.qty_path * e.qty * po.qty_path AS q \
             FROM bom_path pa JOIN bom_edge e ON e.parent = $1 AND e.line_no = $2 \
                  JOIN bom_path po ON po.ancestor = $3 \
             WHERE pa.descendant = $1")
            .bind(p).bind(line_no).bind(old).execute(&mut *tx).await?;
        sqlx::query("UPDATE bom_path p SET qty_path = p.qty_path - dd.q FROM dd \
                     WHERE p.ancestor=dd.a AND p.descendant=dd.d AND p.depth=dd.dep")
            .execute(&mut *tx).await?;
        sqlx::query("DELETE FROM bom_path WHERE qty_path <= 0").execute(&mut *tx).await?;
        // 4) 加入 new 子树经 P 边的贡献
        sqlx::query(
            "CREATE TEMP TABLE di ON COMMIT DROP AS \
             SELECT pa.ancestor AS a, pn.descendant AS d, pa.depth + 1 + pn.depth AS dep, \
                    pa.qty_path * e.qty * pn.qty_path AS q \
             FROM bom_path pa JOIN bom_edge e ON e.parent = $1 AND e.line_no = $2 \
                  CROSS JOIN bom_path pn \
             WHERE pa.descendant = $1 AND pn.ancestor = $3")
            .bind(p).bind(line_no).bind(new_root).execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO bom_path(ancestor,descendant,depth,qty_path) SELECT a,d,dep,q FROM di \
             ON CONFLICT (ancestor,descendant,depth) DO UPDATE SET qty_path = bom_path.qty_path + EXCLUDED.qty_path")
            .execute(&mut *tx).await?;
        // 5) old 子树自身闭包成为孤儿（old 独占引用）→ 清理
        sqlx::query("DELETE FROM bom_path WHERE ancestor = $1 AND $1 NOT IN (SELECT child FROM bom_edge)")
            .bind(old).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    let inc_ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("[5/6] 增量维护（替换 {new_edges} 边子树 → {new_path_rows} 闭包行）：{inc_ms:.0} ms（验收 <2000ms）");

    // 增量正确性：与递归 CTE 再对拍
    let cte2: Vec<(i64, i64)> = sqlx::query_as(
        "WITH RECURSIVE t(child, qty) AS ( \
           SELECT child, qty FROM bom_edge WHERE parent = 1 \
           UNION ALL \
           SELECT e.child, t.qty * e.qty FROM bom_edge e JOIN t ON e.parent = t.child \
         ) SELECT child, SUM(qty)::bigint FROM t GROUP BY child")
        .fetch_all(&pool).await?;
    let closure2: HashMap<i64, i64> = sqlx::query_as::<_, (i64, i64)>(SQL_CMP)
        .fetch_all(&pool).await?.into_iter().collect();
    let cte2_m: HashMap<i64, i64> = cte2.into_iter().collect();
    let same2 = closure2.len() == cte2_m.len()
        && closure2.iter().all(|(k, v)| cte2_m.get(k) == Some(v));
    println!("      增量后再对拍：{}（闭包 {} 行 vs CTE {} 行）",
        if same2 { "100% 一致 ✔" } else { "不一致 ✘" }, closure2.len(), cte2_m.len());
    assert!(same2, "增量维护后闭包必须与递归 CTE 一致");

    // 展开性能复核（维护后）
    let mut xs = Vec::new();
    for _ in 0..SAMPLES {
        let t = Instant::now();
        let _rows: Vec<(i64, i64, i64)> = sqlx::query_as(sql_full).fetch_all(&pool).await?;
        xs.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!("      维护后全量展开 P95 = {:.1} ms", p95(xs));

    // ---------- [6/6] 循环引用拒绝（验收⑤） ----------
    let leaf: i64 = sqlx::query_scalar(
        "SELECT p.descendant FROM bom_path p WHERE p.ancestor=1 AND p.depth=(SELECT max(depth) FROM bom_path WHERE ancestor=1) LIMIT 1")
        .fetch_one(&pool).await?;
    // 加边 X→Y 成环 ⟺ 存在 bom_path(ancestor=Y, descendant=X)（Y 已可达 X）
    let would_cycle: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM bom_path WHERE ancestor=$1 AND descendant=$2")
        .bind(1i64).bind(leaf).fetch_one(&pool).await?;
    println!("[6/6] 循环检测：加边 {leaf}→1(root)：{}",
        if would_cycle > 0 { "命中祖先链，拒绝 ✔" } else { "未检测到（异常）✘" });
    assert!(would_cycle > 0);
    // 反例：指向无关节点（新未用 id）应放行
    let unused = next_id;
    let ok: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM bom_path WHERE ancestor=$1 AND descendant=$2")
        .bind(unused).bind(leaf).fetch_one(&pool).await?;
    assert_eq!(ok, 0, "无关节点应放行");
    println!("      反例：{leaf}→{unused}（无关节点）检测通过，放行 ✔");

    println!("\n== V-2 汇总：① {build_secs:.1}s(<60s) ② {full_p95:.1}ms(<1000ms) ③ {inc_ms:.0}ms(<2000ms) ④ 一致 ⑤ 拒绝 ✔ ==");
    Ok(())
}

/// 生成一棵独立小树（用于替换子树）
fn gen_small_tree(rng: &mut StdRng, root: i64, target: usize, next_id: &mut i64) -> Graph {
    let mut children: HashMap<i64, Vec<(i64, i64)>> = HashMap::new();
    let mut edges = Vec::new();
    let mut queue: VecDeque<(i64, usize)> = VecDeque::new();
    queue.push_back((root, 0));
    let mut left = target;
    while let Some((node, depth)) = queue.pop_front() {
        if left == 0 || depth >= 8 { continue; }
        let max_c = left.min(6);
        let c = rng.random_range(2..=max_c.max(2));
        for line in 1..=c as i32 {
            left -= 1;
            let id = *next_id; *next_id += 1;
            queue.push_back((id, depth + 1));
            let qty = rng.random_range(1..=3);
            children.entry(node).or_default().push((id, qty));
            edges.push((node, line, id, qty));
            if left == 0 { break; }
        }
    }
    Graph { children, edges }
}

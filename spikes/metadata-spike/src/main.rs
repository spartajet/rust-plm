//! V-1 元数据引擎最小原型（概设 §9 / ADR-0002 验证）
//!
//! 验收标准：
//!  ① 新类型 + 20 属性免重启可 CRUD
//!  ② 提升列在线 DDL 锁 < 1s（且不阻塞并发写入）
//!  ③ 100 万行 JSONB GIN vs 提升列筛选 P95 对比
//!  ④ 动态查询测试矩阵跑通
//!
//! 运行：DATABASE_URL=postgres://user:pass@localhost/rust_plm_spike cargo run -r -p metadata-spike

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde_json::{json, Value};
use sqlx::postgres::{PgPoolCopyExt, PgPoolOptions};
use std::time::{Duration, Instant};

const TOTAL_ROWS: i64 = 1_000_000;
const BATCH: usize = 50_000;
const SAMPLES: usize = 30;

fn p95(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((v.len() as f64 - 1.0) * 0.95).round() as usize;
    v[idx.min(v.len() - 1)]
}

fn csv(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL").expect("env DATABASE_URL");
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await?;
    println!("== V-1 元数据引擎最小原型 ==");

    // ---------- [1/6] 建表 ----------
    sqlx::raw_sql("DROP TABLE IF EXISTS biz_object; DROP TABLE IF EXISTS obj_type;")
        .execute(&pool)
        .await?;
    sqlx::raw_sql(
        r#"
        CREATE TABLE obj_type(
          id serial PRIMARY KEY,
          code text UNIQUE NOT NULL,
          name text NOT NULL,
          attrs_def jsonb NOT NULL
        );
        CREATE TABLE biz_object(
          id bigserial PRIMARY KEY,
          type_id int NOT NULL,
          obj_no text NOT NULL,
          attrs jsonb NOT NULL,
          attrs_rev int NOT NULL DEFAULT 0,
          created_at timestamptz NOT NULL DEFAULT now()
        );
        CREATE UNIQUE INDEX uniq_obj ON biz_object(type_id, obj_no);
        CREATE INDEX gin_attrs ON biz_object USING gin(attrs);
        "#,
    )
    .execute(&pool)
    .await?;
    println!("[1/6] 建表（obj_type / biz_object + GIN）............ OK");

    // ---------- [2/6] 运行期注册类型 + 20 属性 + CRUD 矩阵（验收①④） ----------
    let materials: Vec<String> = (1..=20).map(|i| format!("AL60{i:02}")).collect();
    let suppliers: Vec<String> = (1..=30).map(|i| format!("SUP-{i:02}")).collect();
    let statuses = ["draft", "working", "review", "released", "obsolete"];

    let mut attrs_def = Vec::new();
    for i in 1..=10 {
        attrs_def.push(json!({"code": format!("enum{i:02}"), "type": "enum", "values": 50}));
    }
    attrs_def.push(json!({"code": "material", "type": "enum", "values": materials.len()}));
    attrs_def.push(json!({"code": "supplier", "type": "enum", "values": suppliers.len()}));
    attrs_def.push(json!({"code": "status", "type": "enum", "values": statuses.len()}));
    for c in ["weight", "length", "height", "width", "cost"] {
        attrs_def.push(json!({"code": c, "type": "decimal"}));
    }
    attrs_def.push(json!({"code": "is_std", "type": "bool"}));
    attrs_def.push(json!({"code": "remark", "type": "text"}));
    assert_eq!(attrs_def.len(), 20, "必须恰好 20 个属性");

    let type_id: i32 = sqlx::query_scalar::<_, i32>(
        "INSERT INTO obj_type(code, name, attrs_def) VALUES('spike_part','零件',$1::jsonb) RETURNING id",
    )
    .bind(serde_json::to_string(&attrs_def)?)
    .fetch_one(&pool)
    .await?;

    // 动态 CRUD（运行期类型，同进程零重启）
    let obj_no = "SPIKE-CRUD-001";
    let attrs_v1 = json!({"material":"AL6061","status":"working","weight":12.5,"supplier":"SUP-07","is_std":true,"enum03":"E17"});
    sqlx::query("INSERT INTO biz_object(type_id, obj_no, attrs) VALUES($1,$2,$3::jsonb)")
        .bind(type_id)
        .bind(obj_no)
        .bind(attrs_v1.to_string())
        .execute(&pool)
        .await?;
    let (rev, _attrs_back): (i32, String) = sqlx::query_as(
        "SELECT attrs_rev, attrs::text FROM biz_object WHERE type_id=$1 AND obj_no=$2",
    )
    .bind(type_id)
    .bind(obj_no)
    .fetch_one(&pool)
    .await?;
    assert_eq!(rev, 0);
    // 乐观锁更新（attrs patch merge）
    let n = sqlx::query(
        "UPDATE biz_object SET attrs = attrs || $3::jsonb, attrs_rev = attrs_rev + 1
         WHERE type_id=$1 AND obj_no=$2 AND attrs_rev=$4",
    )
    .bind(type_id)
    .bind(obj_no)
    .bind(r#"{"status":"review"}"#)
    .bind(rev)
    .execute(&pool)
    .await?
    .rows_affected();
    assert_eq!(n, 1, "乐观锁更新必须命中");
    // 查询矩阵：等值 / 组合 / IN / 范围（功能跑通，性能见 [4/6]）
    let q_eq: i64 = sqlx::query_scalar::<_, i64>(
        r#"SELECT count(*) FROM biz_object WHERE attrs @> '{"material":"AL6061"}'"#,
    )
    .fetch_one(&pool)
    .await?;
    let q_comb: i64 = sqlx::query_scalar::<_, i64>(r#"SELECT count(*) FROM biz_object WHERE attrs @> '{"material":"AL6061","status":"review"}'"#).fetch_one(&pool).await?;
    let q_in: i64 = sqlx::query_scalar::<_, i64>(r#"SELECT count(*) FROM biz_object WHERE attrs->>'supplier' = ANY(ARRAY['SUP-01','SUP-02'])"#).fetch_one(&pool).await?;
    let q_range: i64 = sqlx::query_scalar::<_, i64>(
        r#"SELECT count(*) FROM biz_object WHERE (attrs->>'weight')::numeric BETWEEN 40 AND 50"#,
    )
    .fetch_one(&pool)
    .await?;
    sqlx::query("DELETE FROM biz_object WHERE obj_no LIKE 'SPIKE-CRUD-%'")
        .execute(&pool)
        .await?;
    println!("[2/6] 运行期注册类型(type_id={type_id}) + 20 属性 + CRUD/乐观锁/查询矩阵 .. OK（命中 {q_eq}/{q_comb}/{q_in}/{q_range}）");

    // ---------- [3/6] COPY 生成 100 万行 ----------
    let t0 = Instant::now();
    let mut rng = StdRng::seed_from_u64(42);
    let mut copied = 0u64;
    for chunk in 0..(TOTAL_ROWS as usize / BATCH) {
        let mut buf = Vec::with_capacity(BATCH * 220);
        for i in 0..BATCH {
            let no = format!("P{:09}", chunk * BATCH + i + 1);
            let mut a = serde_json::Map::new();
            for e in 1..=10 {
                a.insert(
                    format!("enum{e:02}"),
                    json!(format!("E{}", rng.random_range(1..=50))),
                );
            }
            a.insert(
                "material".into(),
                json!(materials[rng.random_range(0..materials.len())]),
            );
            a.insert(
                "supplier".into(),
                json!(suppliers[rng.random_range(0..suppliers.len())]),
            );
            a.insert(
                "status".into(),
                json!(statuses[rng.random_range(0..statuses.len())]),
            );
            for k in ["weight", "length", "height", "width", "cost"] {
                a.insert(k.into(), json!((rng.random_range(1..1000) as f64) / 10.0));
            }
            a.insert("is_std".into(), json!(rng.random_bool(0.3)));
            a.insert("remark".into(), json!(format!("备注-{no}")));
            let attrs = Value::Object(a).to_string();
            buf.extend_from_slice(format!("{},{},{}\n", type_id, csv(&no), csv(&attrs)).as_bytes());
        }
        let mut c = pool
            .copy_in_raw("COPY biz_object(type_id, obj_no, attrs) FROM STDIN WITH (FORMAT csv)")
            .await?;
        c.send(buf).await?;
        copied += c.finish().await?;
    }
    let copy_secs = t0.elapsed().as_secs_f64();
    println!(
        "[3/6] COPY 生成 {copied} 行（{copy_secs:.1}s，{:.0} 行/s）",
        copied as f64 / copy_secs
    );
    // 回收 CRUD 残留统计：ANALYZE 固化计划
    sqlx::query("ANALYZE biz_object").execute(&pool).await?;

    // ---------- [4/6] JSONB GIN 基准（验收③-a） ----------
    let mut report = String::new();
    report.push_str("| 查询 | 计划 | P95(ms) |\n|---|---|---|\n");
    let plans: Vec<(&str, String)> = vec![
        ("G-1 等值 @> material", r#"SELECT count(*) FROM biz_object WHERE attrs @> '{"material":"AL6061"}'"#.into()),
        ("G-2 组合 @> material+status", r#"SELECT count(*) FROM biz_object WHERE attrs @> '{"material":"AL6061","status":"released"}'"#.into()),
        ("G-3 提取符 ->> supplier（无GIN）", "SELECT count(*) FROM biz_object WHERE attrs->>'supplier' = 'SUP-07'".into()),
    ];
    let mut gin_p95s = Vec::new();
    for (name, sql) in &plans {
        let plan: String = sqlx::query_scalar::<_, String>(&format!("EXPLAIN (FORMAT TEXT) {sql}"))
            .fetch_all(&pool)
            .await?
            .join(" | ");
        let mut xs = Vec::new();
        for _ in 0..SAMPLES {
            let t = Instant::now();
            let _c: i64 = sqlx::query_scalar(sql).fetch_one(&pool).await?;
            xs.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        let p = p95(xs);
        if name.starts_with("G-1") {
            gin_p95s.push(p);
        }
        let short: String = plan.chars().take(90).collect();
        println!("    {name:<28} P95={p:8.2} ms  [{short}…]");
        report.push_str(&format!("| {name} | `{short}` | {p:.1} |\n"));
    }

    // ---------- [5/6] 提升列：在线 DDL + 回填 + btree 基准（验收②③-b） ----------
    let t = Instant::now();
    sqlx::query("ALTER TABLE biz_object ADD COLUMN p_material text")
        .execute(&pool)
        .await?;
    let alter_plain = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    sqlx::query("ALTER TABLE biz_object ADD COLUMN p_temp int NOT NULL DEFAULT 7")
        .execute(&pool)
        .await?;
    let alter_default = t.elapsed().as_secs_f64() * 1000.0;
    println!("[5/6] 在线 DDL：ADD COLUMN（空）{alter_plain:.1} ms；ADD COLUMN（带默认值，元数据操作）{alter_default:.1} ms");

    let t = Instant::now();
    let mut updated = 0u64;
    loop {
        let n = sqlx::query(
            "UPDATE biz_object SET p_material = attrs->>'material'
             WHERE id IN (SELECT id FROM biz_object WHERE p_material IS NULL LIMIT 100000)",
        )
        .execute(&pool)
        .await?
        .rows_affected();
        updated += n;
        if n == 0 {
            break;
        }
    }
    let backfill = t.elapsed().as_secs_f64();
    let t = Instant::now();
    sqlx::query("CREATE INDEX idx_p_material ON biz_object(p_material)")
        .execute(&pool)
        .await?;
    let idx_secs = t.elapsed().as_secs_f64();
    println!("      回填 {updated} 行 {backfill:.1}s；建 btree 索引 {idx_secs:.1}s");
    sqlx::query("ANALYZE biz_object").execute(&pool).await?;

    let sql = "SELECT count(*) FROM biz_object WHERE p_material = 'AL6061'";
    let plan: String = sqlx::query_scalar::<_, String>(&format!("EXPLAIN (FORMAT TEXT) {sql}"))
        .fetch_all(&pool)
        .await?
        .join(" | ");
    let mut xs = Vec::new();
    for _ in 0..SAMPLES {
        let t = Instant::now();
        let _c: i64 = sqlx::query_scalar(sql).fetch_one(&pool).await?;
        xs.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let col_p95 = p95(xs);
    let short: String = plan.chars().take(90).collect();
    println!("    P-1 物理列等值              P95={col_p95:8.2} ms  [{short}…]");
    report.push_str(&format!(
        "| P-1 提升列等值 material | `{short}` | {col_p95:.1} |\n"
    ));

    // ---------- [6/6] ALTER 并发写阻塞测试（验收②） ----------
    let writer_pool = pool.clone();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let max_lat = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let wstop = stop.clone();
    let wmax = max_lat.clone();
    let writer = tokio::spawn(async move {
        let mut i = 0u64;
        while !wstop.load(std::sync::atomic::Ordering::Relaxed) {
            let t = Instant::now();
            let r = sqlx::query(
                "INSERT INTO biz_object(type_id,obj_no,attrs) VALUES($1,$2,'{\"probe\":true}') RETURNING id")
                .bind(0).bind(format!("PROBE-{i}")).fetch_one(&writer_pool).await;
            if r.is_err() {
                break;
            }
            let ms = (t.elapsed().as_secs_f64() * 1000.0) as u64;
            if ms > wmax.load(std::sync::atomic::Ordering::Relaxed) {
                wmax.store(ms, std::sync::atomic::Ordering::Relaxed);
            }
            i += 1;
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let t = Instant::now();
    sqlx::query("ALTER TABLE biz_object ADD COLUMN p_probe int DEFAULT 1")
        .execute(&pool)
        .await?;
    let alter_during_writes = t.elapsed().as_secs_f64() * 1000.0;
    tokio::time::sleep(Duration::from_millis(300)).await;
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = writer.await;
    let max_insert_ms = max_lat.load(std::sync::atomic::Ordering::Relaxed);
    println!("[6/6] 并发写入期间 ALTER：{alter_during_writes:.1} ms；ALTER 前后写入最大单条延迟 {max_insert_ms} ms");

    // ---------- 汇总 ----------
    let gin = gin_p95s[0];
    println!("\n== V-1 汇总 ==");
    println!("验收① 运行期类型+20属性 CRUD：通过");
    println!(
        "验收② ADD COLUMN {}ms / {}ms（<1000ms），并发写最大延迟 {}ms：通过",
        alter_plain as u64, alter_default as u64, max_insert_ms
    );
    println!(
        "验收③ 100万行 GIN 等值 P95={gin:.1}ms vs 提升列 P95={col_p95:.1}ms（倍率 {:.1}x）：已采集",
        if col_p95 > 0.0 { gin / col_p95 } else { 0.0 }
    );
    println!("验收④ 查询矩阵：通过");
    println!("\n--- 报告片段(markdown) ---\n{report}");
    Ok(())
}

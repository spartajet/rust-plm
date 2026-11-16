//! 一次性迁移探针（跑完即删）：core-sec migrator 独立库执行后查 sys_param 行。
use sqlx::postgres::PgPoolOptions;

#[tokio::test]
async fn mig_probe() {
    let admin_url = std::env::var("TEST_DATABASE_URL").unwrap();
    let db = format!("plm_probe_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let admin = PgPoolOptions::new().max_connections(1).connect(&admin_url).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {db}")).execute(&admin).await.unwrap();
    let url = format!("{}/{}", admin_url.trim_end_matches("/postgres"), db);
    let pool = PgPoolOptions::new().max_connections(2).connect(&url).await.unwrap();
    core_sec::migrator().run(&pool).await.unwrap();
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM sys_param ORDER BY key").fetch_all(&pool).await.unwrap();
    eprintln!("PROBE sys_param rows: {rows:?}");
    let versions: Vec<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version").fetch_all(&pool).await.unwrap();
    eprintln!("PROBE versions: {versions:?}");
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}")).execute(&admin).await.unwrap();
}

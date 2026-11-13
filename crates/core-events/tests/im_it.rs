#[tokio::test]
async fn outbox_topic_partial_index_exists() {
    // 增量13 M2：0013 部分索引存在性钉（IM pusher 反连接/dispatcher topic 扫描的索引依赖）
    let admin_url = std::env::var("TEST_DATABASE_URL").unwrap();
    let db = format!(
        "imx_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let admin = sqlx::postgres::PgPoolOptions::new()
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
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&format!("{prefix}/{db}"))
        .await
        .unwrap();
    core_model::migrator().run(&pool).await.unwrap();
    core_version::migrator().run(&pool).await.unwrap();
    core_events::migrator().run(&pool).await.unwrap();

    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_indexes \
         WHERE tablename='event_outbox' AND indexdef LIKE '%doc.submitted%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        n >= 1,
        "0013 部分索引必须存在（partial WHERE doc.submitted）"
    );

    pool.close().await;
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await
        .unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

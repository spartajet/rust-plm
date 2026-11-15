//! core-search —— 全文检索（增量20 RPT-01/03，ADR-0005 内嵌 tantivy）。
//! tokenizer：jieba 分词 + CJK 双字兜底（子串召回）；service：对账扫描近实时 + ABAC 密级过滤。

pub mod service;
pub mod tokenizer;

pub use service::{GroupResult, SearchHit, SearchService, SEARCH_TYPES};
pub use tokenizer::JiebaTokenizer;

/// 启动后台扫描器：先全量重建（幂等），随后按 interval 增量扫描推进水位。
/// 失败仅 WARN 不中断（下一轮自愈）；interval 由 PLM_SEARCH_SCAN_SECS 配置（缺省 2s，测试可注入更短）。
pub fn spawn_scanner(
    service: std::sync::Arc<SearchService>,
    pool: sqlx::PgPool,
    interval: std::time::Duration,
) {
    tokio::spawn(async move {
        let mut watermark = chrono::DateTime::<chrono::Utc>::UNIX_EPOCH;
        if service.degraded() {
            tracing::warn!("搜索 degraded：跳过扫描器写入循环");
            return;
        }
        match service.rebuild_full(&pool).await {
            Ok((n, max_at)) => {
                watermark = max_at;
                tracing::info!(indexed = n, "搜索索引全量重建完成");
            }
            Err(e) => tracing::warn!(error = %e, "搜索索引全量重建失败（下轮扫描自愈）"),
        }
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut scans: u32 = 0;
        loop {
            tick.tick().await;
            scans += 1;
            // 周期性全量重建：兜底迟提交窗口（事务开始早/提交晚于水位扫描的行）
            if scans.is_multiple_of(300) {
                match service.rebuild_full(&pool).await {
                    Ok((_, max_at)) => watermark = max_at,
                    Err(e) => tracing::warn!(error = %e, "搜索索引周期重建失败（下轮自愈）"),
                }
                continue;
            }
            match service.scan_once(&pool, &mut watermark).await {
                Ok(n) if n > 0 => tracing::debug!(replayed = n, "搜索索引增量回放"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "搜索索引扫描失败（下轮自愈）"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::tokenizer::TextAnalyzer;

    #[test]
    fn tokenizer_registered_name_is_stable() {
        // schema 引用常量名——重构改名会静默破坏既有索引，钉住
        assert_eq!(JiebaTokenizer::NAME, "jieba_bigram");
    }

    #[test]
    fn analyzer_smoke() {
        let mut a = TextAnalyzer::builder(JiebaTokenizer::default()).build();
        let mut ts = a.token_stream("轴承座 DWG");
        let mut n = 0;
        while ts.advance() {
            assert!(!ts.token().text.is_empty());
            n += 1;
        }
        assert!(n >= 3, "jieba+bigram 至少产出 3 token，got {n}");
    }
}

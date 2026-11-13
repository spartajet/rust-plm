//! COD 编码系统（增量16，PRD §6.2 COD-01/02/03）：规则解析/渲染 + 计数器取号。
//! 规则存 cm_type.config.cod_rule（JSONB——ADR-0002 配置优先于代码，registry 热生效）；
//! 计数器 cod_counter 原子 upsert（并发无重号，key=日期段按日流水）。

use crate::{DomainError, Result};
use chrono::Utc;
use sqlx::PgPool;

/// 编码规则（cm_type.config.cod_rule；缺省/字段缺失=类型不参与自动赋码）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodRule {
    pub prefix: String,
    /// "yyyyMMdd" 或 "none"
    pub date_fmt: String,
    pub seq_digits: i32,
}

impl CodRule {
    pub fn from_config(v: &serde_json::Value) -> Option<Self> {
        let obj = v.get("cod_rule")?;
        Some(Self {
            prefix: obj.get("prefix")?.as_str()?.to_string(),
            date_fmt: obj.get("date_fmt")?.as_str()?.to_string(),
            seq_digits: obj.get("seq_digits")?.as_i64()? as i32,
        })
    }

    /// 渲染 `{prefix}{date?}{seq:0>digits}`（seq 溢出位数自然增长）
    pub fn render(&self, date_key: &str, seq: i64) -> String {
        let seq_str = format!("{:0>width$}", seq, width = self.seq_digits as usize);
        if self.date_fmt == "none" {
            format!("{}{}", self.prefix, seq_str)
        } else {
            format!("{}{}-{}", self.prefix, date_key, seq_str)
        }
    }

    /// 当前日期段 key（PRD 用 yyyyMMdd 面向配置的写法 → chrono 格式映射）
    pub fn date_key_now(&self) -> String {
        if self.date_fmt == "none" {
            "none".into()
        } else {
            let fmt = match self.date_fmt.as_str() {
                "yyyyMMdd" => "%Y%m%d",
                "yyyyMM" => "%Y%m",
                other => other, // 已是 chrono 格式则透传
            };
            Utc::now().format(fmt).to_string()
        }
    }
}

/// 原子取号：INSERT ON CONFLICT DO UPDATE —— 并发无重号（COD-02）
async fn next_seq(pool: &PgPool, type_code: &str, key: &str) -> Result<i64> {
    let v: i64 = sqlx::query_scalar(
        "INSERT INTO cod_counter(type_code, key, value) VALUES ($1, $2, 1) \
         ON CONFLICT (type_code, key) DO UPDATE SET value = cod_counter.value + 1 \
         RETURNING value",
    )
    .bind(type_code)
    .bind(key)
    .fetch_one(pool)
    .await
    .map_err(|e| DomainError::Db(e.to_string()))?;
    Ok(v)
}

/// 按规则生成下一个码（事务外取号——create 同事务插入由调用方编排，烧号窗口记账）
pub async fn next_code(
    pool: &PgPool,
    type_code: &str,
    rule: &CodRule,
    date_key: &str,
) -> Result<String> {
    let seq = next_seq(pool, type_code, date_key).await?;
    Ok(rule.render(date_key, seq))
}

/// 预览（不烧号）：未来 n 个码（SELECT value 为只读快照——并发下可能少 1，预览语义可接受）
pub async fn preview(
    pool: &PgPool,
    type_code: &str,
    rule: &CodRule,
    n: usize,
) -> Result<Vec<String>> {
    let key = rule.date_key_now();
    let cur: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT value FROM cod_counter WHERE type_code=$1 AND key=$2), 0)",
    )
    .bind(type_code)
    .bind(&key)
    .fetch_one(pool)
    .await
    .map_err(|e| DomainError::Db(e.to_string()))?;
    Ok((1..=n as i64).map(|i| rule.render(&key, cur + i)).collect())
}

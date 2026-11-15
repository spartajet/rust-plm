//! SearchService（增量20 RPT-01/03，ADR-0005）：内嵌 tantivy 索引 + 对账扫描近实时。
//!
//! 更新管道=MVP 形态的对账扫描器：启动全量重建（幂等 delete+add），随后按扫描周期
//! 拉 `updated_at > 水位-1s` 的最新活版本按 obj_no 幂等 upsert——<5s 近实时，零侵入写入链路。
//! 权限=ABAC 密级查询侧过滤（term 枚举 0..=max）+命中后 stored 复核双保险。

use crate::tokenizer::JiebaTokenizer;
use core_model::DomainError;
use sqlx::PgPool;
use std::path::Path;
use tantivy::collector::{Count, TopDocs};
use tantivy::query::{BooleanQuery, TermQuery};
use tantivy::query_grammar::Occur;
use tantivy::schema::{Field, Schema, Value, STORED, STRING};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

/// 参与全局检索的对象类型（RPT-01 MVP：图纸+物料；新类型在此登记，台账记）
pub const SEARCH_TYPES: [&str; 2] = ["drawing", "part"];

/// 检索命中行
#[derive(Debug, serde::Serialize)]
pub struct SearchHit {
    pub type_code: String,
    pub obj_no: String,
    pub name: Option<String>,
    pub state: String,
    pub major_ver: i32,
    pub minor_ver: i32,
    pub secret_level: i16,
}

/// 类型聚类结果（RPT-01：一处搜一切、按类型聚类）
#[derive(Debug, serde::Serialize)]
pub struct GroupResult {
    pub type_code: String,
    pub total: u64,
    pub items: Vec<SearchHit>,
}

/// schema 字段句柄（build_schema 产出，open 持有）
struct Fields {
    f_type: Field,
    f_obj_no: Field,
    f_text: Field,
    f_name: Field,
    f_state: Field,
    f_major: Field,
    f_minor: Field,
    f_secret: Field,
}

fn is_cjk(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c)
}

fn build_schema() -> (Schema, Fields) {
    let mut b = Schema::builder();
    let f_type = b.add_text_field("type_code", STRING | STORED);
    let f_obj_no = b.add_text_field("obj_no", STRING | STORED);
    let f_text = b.add_text_field(
        "text",
        tantivy::schema::TextOptions::default()
            .set_stored()
            .set_indexing_options(
                tantivy::schema::TextFieldIndexing::default()
                    .set_tokenizer(JiebaTokenizer::NAME)
                    // QueryParser BM25 评分需位置索引（缺省 Basic 无 positions——报错钉）
                    .set_index_option(tantivy::schema::IndexRecordOption::WithFreqsAndPositions),
            ),
    );
    let f_name = b.add_text_field("name", STORED);
    let f_state = b.add_text_field("state", STORED);
    let f_major = b.add_i64_field("major_ver", STORED);
    let f_minor = b.add_i64_field("minor_ver", STORED);
    let f_secret = b.add_u64_field(
        "secret_level",
        tantivy::schema::NumericOptions::default()
            .set_stored()
            .set_indexed(),
    );
    (
        b.build(),
        Fields {
            f_type,
            f_obj_no,
            f_text,
            f_name,
            f_state,
            f_major,
            f_minor,
            f_secret,
        },
    )
}

/// 扫描行（cm_object 最新活版本 + 类型码 + 检索文本属性）
#[derive(Debug, sqlx::FromRow)]
struct ScanRow {
    obj_no: String,
    type_code: String,
    name: Option<String>,
    material: Option<String>,
    state: String,
    major_ver: i32,
    minor_ver: i32,
    secret_level: i16,
    updated_at: chrono::DateTime<chrono::Utc>,
}

const SCAN_HEAD: &str = "WITH latest AS ( \
     SELECT DISTINCT ON (o.obj_no) o.obj_no, t.code AS type_code, o.attrs, o.state, \
            o.major_ver, o.minor_ver, o.secret_level, o.updated_at \
     FROM cm_object o JOIN cm_type t ON t.id = o.type_id \
     WHERE o.deleted_at IS NULL";

const SCAN_TAIL: &str =
    " ORDER BY o.obj_no, o.major_ver DESC, o.minor_ver DESC, o.iteration DESC) \
     SELECT obj_no, type_code, attrs->>'name' AS name, attrs->>'material' AS material, \
            state, major_ver, minor_ver, secret_level, updated_at FROM latest";

pub struct SearchService {
    reader: IndexReader,
    /// 查询侧分析器（与索引侧同 tokenizer；text_terms_query 逐 term 析取——避开 QueryParser
    /// 把相邻 token 组 PhraseQuery 的语义，jieba+bigram 重叠 token 会破坏邻接）
    analyzer: tantivy::tokenizer::TextAnalyzer,
    /// tokio Mutex：0.27 commit 为 &mut self；commit 退避重试需跨 await 持锁（std Guard 非 Send）
    writer: Option<tokio::sync::Mutex<IndexWriter>>,
    f: Fields,
}

impl SearchService {
    /// 打开（或创建）索引目录。目录锁冲突（多实例共享卷）→ writer=None 进入 degraded：
    /// 查询仍可用（reader），写入 503 fail-closed（不崩服务）。
    pub fn open(dir: &Path) -> Result<Self, DomainError> {
        std::fs::create_dir_all(dir).map_err(|e| DomainError::Internal(e.to_string()))?;
        let (schema, f) = build_schema();
        // 以 meta.json 判存在（Index::exists 需 Directory 实参，0.27）；损坏 meta 走 open 报错——fail-closed
        let index = if dir.join("meta.json").exists() {
            Index::open_in_dir(dir).map_err(|e| DomainError::Internal(e.to_string()))?
        } else {
            Index::create_in_dir(dir, schema).map_err(|e| DomainError::Internal(e.to_string()))?
        };
        let analyzer = tantivy::tokenizer::TextAnalyzer::builder(JiebaTokenizer::default()).build();
        index
            .tokenizers()
            .register(JiebaTokenizer::NAME, analyzer.clone());
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(|e| DomainError::Internal(e.to_string()))?;
        let writer = match index.writer(50_000_000) {
            Ok(w) => Some(tokio::sync::Mutex::new(w)),
            Err(e) => {
                tracing::warn!(error = %e, "tantivy 写锁被占（多实例共享索引卷？）——搜索进入 degraded 只读");
                None
            }
        };
        Ok(Self {
            reader,
            writer,
            analyzer,
            f,
        })
    }

    /// degraded（写锁被占）：索引写入不可用，查询仍可
    pub fn degraded(&self) -> bool {
        self.writer.is_none()
    }

    /// 单行 → tantivy 文档
    fn doc_of(&self, r: &ScanRow) -> TantivyDocument {
        let mut d = TantivyDocument::new();
        d.add_text(self.f.f_type, &r.type_code);
        d.add_text(self.f.f_obj_no, &r.obj_no);
        let mut text = String::new();
        if let Some(n) = &r.name {
            text.push_str(n);
            text.push(' ');
        }
        if let Some(m) = &r.material {
            text.push_str(m);
        }
        d.add_text(self.f.f_text, &text);
        if let Some(n) = &r.name {
            d.add_text(self.f.f_name, n);
        }
        d.add_text(self.f.f_state, &r.state);
        d.add_i64(self.f.f_major, r.major_ver as i64);
        d.add_i64(self.f.f_minor, r.minor_ver as i64);
        d.add_u64(self.f.f_secret, r.secret_level.max(0) as u64);
        d
    }

    /// 幂等 upsert 一批行（按 obj_no 先删后加）
    async fn upsert_rows(&self, rows: &[ScanRow]) -> Result<(), DomainError> {
        let Some(writer) = &self.writer else {
            return Err(DomainError::ServiceUnavailable(
                "搜索索引写入不可用（degraded：多实例共享索引卷）".into(),
            ));
        };
        let mut w = writer.lock().await;
        for r in rows {
            w.delete_term(Term::from_field_text(self.f.f_obj_no, &r.obj_no));
            // add_document 仅入内存队列（commit 原子生效；单条失败在 commit 处统一暴露）
            w.add_document(self.doc_of(r))
                .map_err(|e| DomainError::Internal(e.to_string()))?;
        }
        // commit 重试：Windows 安全软件对新写索引文件（.del/.term）有秒级扫描锁，
        // 瞬时 PermissionDenied 退避重试可自愈（本机环境实证；ADR-0007 退避思想同源）
        let mut last_err = None;
        for wait in [0u64, 300, 900] {
            if wait > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
            }
            match w.commit() {
                Ok(_) => {
                    last_err = None;
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(e) = last_err {
            return Err(DomainError::Internal(e.to_string()));
        }
        self.reader
            .reload()
            .map_err(|e| DomainError::Internal(e.to_string()))?;
        Ok(())
    }

    /// 启动全量重建（幂等；返回行数与新水位）
    pub async fn rebuild_full(
        &self,
        pool: &PgPool,
    ) -> Result<(usize, chrono::DateTime<chrono::Utc>), DomainError> {
        let sql = format!("{SCAN_HEAD}{SCAN_TAIL}");
        let rows: Vec<ScanRow> = sqlx::query_as(&sql)
            .fetch_all(pool)
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;
        let watermark = rows
            .iter()
            .map(|r| r.updated_at)
            .max()
            .unwrap_or_else(chrono::Utc::now);
        self.upsert_rows(&rows).await?;
        Ok((rows.len(), watermark))
    }

    /// 增量扫描：严格 `updated_at > 水位`（稳态零回放）。
    /// 迟提交窗口（事务开始早于快照、提交晚于本查询的行——updated_at ≤ 水位）由调用方
    /// 周期性 rebuild_full 兜底（spawn_scanner 每 300 轮 ≈10min；MVP 规模全量重建亚秒级）。
    pub async fn scan_once(
        &self,
        pool: &PgPool,
        watermark: &mut chrono::DateTime<chrono::Utc>,
    ) -> Result<usize, DomainError> {
        let since = *watermark;
        let sql = format!("{SCAN_HEAD} AND o.updated_at > $1{SCAN_TAIL}");
        let rows: Vec<ScanRow> = sqlx::query_as(&sql)
            .bind(since)
            .fetch_all(pool)
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;
        if rows.is_empty() {
            return Ok(0);
        }
        let n = rows.len();
        if let Some(max_at) = rows.iter().map(|r| r.updated_at).max() {
            *watermark = max_at;
        }
        self.upsert_rows(&rows).await?;
        Ok(n)
    }

    /// 内容查询：obj_no 精确 ∪ text 分词 terms 析取（OR）。
    /// 弃 QueryParser：其把分析出的相邻 token 组成 PhraseQuery，jieba 词与 CJK bigram
    /// 重叠 token 会让单词条查询也带邻接约束而全空（诊断实证）。
    /// 内容查询：obj_no 精确 ∪ text 分词 terms 析取（OR，仅 CJK 查询）。
    /// 弃 QueryParser：其把分析出的相邻 token 组成 PhraseQuery，jieba 词与 CJK bigram
    /// 重叠 token 会让单词条查询也带邻接约束而全空（诊断实证）。
    /// 纯 ASCII（编号式输入）只走 obj_no 精确——分词碎片（dwg/fe/…）在 OR 下会泛匹配
    /// 一切名称含该碎片的对象（Playwright 并行钉诊断实证）。
    fn content_query(&self, q: &str) -> BooleanQuery {
        let mut clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> = Vec::new();
        clauses.push((
            Occur::Should,
            Box::new(TermQuery::new(
                Term::from_field_text(self.f.f_obj_no, q),
                tantivy::schema::IndexRecordOption::Basic,
            )),
        ));
        if q.chars().any(is_cjk) {
            let mut an = self.analyzer.clone();
            let mut ts = an.token_stream(q);
            let mut terms = std::collections::BTreeSet::new();
            while ts.advance() {
                terms.insert(ts.token().text.clone());
            }
            for t in &terms {
                clauses.push((
                    Occur::Should,
                    Box::new(TermQuery::new(
                        Term::from_field_text(self.f.f_text, t),
                        tantivy::schema::IndexRecordOption::WithFreqsAndPositions,
                    )),
                ));
            }
        }
        BooleanQuery::new(clauses)
    }

    /// ABAC：密级 0..=max 枚举 term（查询侧过滤；命中后 stored 复核 + DB 权威复核三保险）。
    /// max<0 → 恒假 term（空 BooleanQuery 在 tantivy 0.27 是 match-all——fail-open 陷阱，终审 M1-1）
    fn secret_filter(&self, max_secret: i16) -> BooleanQuery {
        if max_secret < 0 {
            return BooleanQuery::new(vec![(
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_u64(self.f.f_secret, u64::MAX),
                    tantivy::schema::IndexRecordOption::Basic,
                )) as Box<dyn tantivy::query::Query>,
            )]);
        }
        let clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> = (0..=i64::from(max_secret))
            .map(|lvl| {
                let tq: Box<dyn tantivy::query::Query> = Box::new(TermQuery::new(
                    Term::from_field_u64(self.f.f_secret, lvl as u64),
                    tantivy::schema::IndexRecordOption::Basic,
                ));
                (Occur::Should, tq)
            })
            .collect();
        BooleanQuery::new(clauses)
    }

    fn type_filter(&self, type_code: &str) -> BooleanQuery {
        BooleanQuery::new(vec![(
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(self.f.f_type, type_code),
                tantivy::schema::IndexRecordOption::Basic,
            )) as Box<dyn tantivy::query::Query>,
        )])
    }

    fn combined(&self, q: &str, type_code: &str, max_secret: i16) -> BooleanQuery {
        BooleanQuery::new(vec![
            (
                Occur::Must,
                Box::new(self.content_query(q)) as Box<dyn tantivy::query::Query>,
            ),
            (
                Occur::Must,
                Box::new(self.type_filter(type_code)) as Box<dyn tantivy::query::Query>,
            ),
            (
                Occur::Must,
                Box::new(self.secret_filter(max_secret)) as Box<dyn tantivy::query::Query>,
            ),
        ])
    }

    fn hit_of(&self, doc: &TantivyDocument, max_secret: i16) -> Option<SearchHit> {
        let secret = doc
            .get_first(self.f.f_secret)
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as i16;
        // stored 复核（索引内序反防护；对索引陈旧——提密后未扫描——无防护，权威复核在
        // enrich_with_db_secret 的 DB 回查，终审 I1-1 硬化）
        if secret > max_secret {
            return None;
        }
        Some(SearchHit {
            type_code: doc
                .get_first(self.f.f_type)
                .and_then(|v| v.as_str())?
                .to_string(),
            obj_no: doc
                .get_first(self.f.f_obj_no)
                .and_then(|v| v.as_str())?
                .to_string(),
            name: doc
                .get_first(self.f.f_name)
                .and_then(|v| v.as_str())
                .map(String::from),
            state: doc
                .get_first(self.f.f_state)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            major_ver: doc
                .get_first(self.f.f_major)
                .and_then(|v| v.as_i64())
                .unwrap_or(1) as i32,
            minor_ver: doc
                .get_first(self.f.f_minor)
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32,
            secret_level: secret,
        })
    }

    /// 诊断：text 字段裸 term 命中数（doc hidden——运维对账/测试隔离 QueryParser 与索引侧）
    #[doc(hidden)]
    pub fn raw_text_term_hits(&self, term: &str) -> Result<u64, DomainError> {
        let searcher = self.reader.searcher();
        let q = TermQuery::new(
            Term::from_field_text(self.f.f_text, term),
            tantivy::schema::IndexRecordOption::Basic,
        );
        Ok(searcher
            .search(&q, &Count)
            .map_err(|e| DomainError::Internal(e.to_string()))? as u64)
    }

    /// DB 密级权威复核（终审 I1-1 硬化）：命中行回查最新活版本 secret_level——
    /// 索引陈旧（带外提密后未及扫描）时 fail-closed 剔除；对象已删同样剔除。
    /// total 仍为索引侧计数（偏高方向安全——多计不泄露内容，页面 items 精确）。
    async fn recheck_db_secret(
        &self,
        pool: &PgPool,
        hits: Vec<SearchHit>,
        max_secret: i16,
    ) -> Result<Vec<SearchHit>, DomainError> {
        if hits.is_empty() {
            return Ok(hits);
        }
        let nos: Vec<String> = hits.iter().map(|h| h.obj_no.clone()).collect();
        let rows: Vec<(String, i16)> = sqlx::query_as(
            "SELECT DISTINCT ON (obj_no) obj_no, secret_level FROM cm_object \
             WHERE obj_no = ANY($1) AND deleted_at IS NULL \
             ORDER BY obj_no, major_ver DESC, minor_ver DESC, iteration DESC",
        )
        .bind(&nos)
        .fetch_all(pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;
        let latest: std::collections::HashMap<String, i16> = rows.into_iter().collect();
        Ok(hits
            .into_iter()
            .filter(|h| {
                latest
                    .get(&h.obj_no)
                    .map(|s| *s <= max_secret)
                    .unwrap_or(false)
            })
            .collect())
    }

    /// 聚类检索（RPT-01 主路径）：每类型独立计数+取前 per_group 条；DB 密级权威复核
    pub async fn search_groups(
        &self,
        pool: &PgPool,
        q: &str,
        max_secret: i16,
        per_group: usize,
    ) -> Result<Vec<GroupResult>, DomainError> {
        let searcher = self.reader.searcher();
        let mut groups = Vec::new();
        for t in SEARCH_TYPES {
            let query = self.combined(q, t, max_secret);
            let total: u64 = searcher
                .search(&query, &Count)
                .map_err(|e| DomainError::Internal(e.to_string()))?
                as u64;
            let items = if total == 0 {
                Vec::new()
            } else {
                let top = TopDocs::with_limit(per_group).order_by_score();
                let raw: Vec<SearchHit> = searcher
                    .search(&query, &top)
                    .map_err(|e| DomainError::Internal(e.to_string()))?
                    .into_iter()
                    .filter_map(|(_, addr)| {
                        let doc: TantivyDocument = searcher.doc(addr).ok()?;
                        self.hit_of(&doc, max_secret)
                    })
                    .collect();
                self.recheck_db_secret(pool, raw, max_secret).await?
            };
            groups.push(GroupResult {
                type_code: t.to_string(),
                total,
                items,
            });
        }
        Ok(groups)
    }

    /// 单类型钻取分页（q 命中该类型的平铺列表）；DB 密级权威复核
    pub async fn search_page(
        &self,
        pool: &PgPool,
        q: &str,
        type_code: &str,
        max_secret: i16,
        page: u32,
        size: u32,
    ) -> Result<(Vec<SearchHit>, u64), DomainError> {
        let searcher = self.reader.searcher();
        let query = self.combined(q, type_code, max_secret);
        let total: u64 = searcher
            .search(&query, &Count)
            .map_err(|e| DomainError::Internal(e.to_string()))? as u64;
        let off = i64::from(page.saturating_sub(1)) * i64::from(size);
        let items = if total == 0 {
            Vec::new()
        } else {
            let top = TopDocs::with_limit(size as usize)
                .and_offset(off as usize)
                .order_by_score();
            let raw: Vec<SearchHit> = searcher
                .search(&query, &top)
                .map_err(|e| DomainError::Internal(e.to_string()))?
                .into_iter()
                .filter_map(|(_, addr)| {
                    let doc: TantivyDocument = searcher.doc(addr).ok()?;
                    self.hit_of(&doc, max_secret)
                })
                .collect();
            self.recheck_db_secret(pool, raw, max_secret).await?
        };
        Ok((items, total))
    }
}

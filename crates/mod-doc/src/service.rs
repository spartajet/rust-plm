//! DocService —— 图文档业务编排（概设 §4 mod-doc）。
//!
//! upload 序（plan Task2 锚定）：FileStore.put（流式+哈希）→ 空文件拒绝 →
//! ObjectStore.create（唯一预检/乐观锁/钩子在 core-model）→ file_object ON CONFLICT DO NOTHING
//! → file_ref INSERT。已知窗口（Review Focus #1）：create 失败留孤儿文件——内容寻址去重使其无害，
//! GC 属 core-files 完整版。

use crate::dto::{DocDetail, FileInfo};
use core_files::FileStore;
use core_model::query::ObjectQuery;
use core_model::types::{AttrMap, BizObject};
use core_model::{DomainError, ObjectStore, PgObjectStore, Result, TypeRegistry};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::io::AsyncRead;

pub struct DocService {
    pool: PgPool,
    /// 预留：钩子/查重门禁与提升列决策需要类型元数据（core-files 完整版接入）
    #[allow(dead_code)]
    registry: Arc<TypeRegistry>,
    objects: Arc<PgObjectStore>,
    files: Arc<dyn FileStore>,
}

/// 已落盘的暂存上传件（clippy 超参收敛：upload_staged 结构体参数，增量19）
pub struct StagedUpload {
    pub obj_no: String,
    pub attrs: AttrMap,
    pub file_name: String,
    pub mime: Option<String>,
    pub hash: String,
    pub size: i64,
}

impl DocService {
    pub const TYPE: &str = "drawing";

    pub fn new(
        pool: PgPool,
        registry: Arc<TypeRegistry>,
        objects: Arc<PgObjectStore>,
        files: Arc<dyn FileStore>,
    ) -> Self {
        Self {
            pool,
            registry,
            objects,
            files,
        }
    }

    async fn put_stream(
        &self,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<(String, u64)> {
        let m = self
            .files
            .put(reader)
            .await
            .map_err(|e| DomainError::File(e.to_string()))?;
        if m.size == 0 {
            return Err(DomainError::FileEmpty);
        }
        Ok((m.hash, m.size))
    }

    /// 单体路径：staged 文件 + 建对象两步合一（测试/CLI 便捷入口）
    pub async fn upload(
        &self,
        obj_no: String,
        attrs: AttrMap,
        file_name: String,
        mime: Option<String>,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<DocDetail> {
        let (hash, size) = self.stage_file(reader).await?;
        self.upload_staged(
            StagedUpload {
                obj_no,
                attrs,
                file_name,
                mime,
                hash,
                size,
            },
            None,
        )
        .await
    }

    /// 第一步：流式落盘（内容寻址）。网关在 multipart 循环内消费 file 字段时调用，
    /// 避免 Field 借用跨 next_field 迭代（E0499）。
    pub async fn stage_file(
        &self,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<(String, i64)> {
        let (hash, size) = self.put_stream(reader).await?;
        Ok((hash, size as i64))
    }

    /// 第二步：建对象 + 文件两行落库。调用前提：stage_file 已完成（或已有 hash）。
    pub async fn upload_staged(
        &self,
        staged: StagedUpload,
        actor: Option<uuid::Uuid>,
    ) -> Result<DocDetail> {
        let StagedUpload {
            obj_no,
            attrs,
            file_name,
            mime,
            hash,
            size,
        } = staged;
        if file_name.trim().is_empty() {
            return Err(DomainError::FieldRequired("file.filename".into()));
        }
        // owner/created_by/updated_by 同源（INSERT $5 复用）——传 actor 使"我发起"盒与
        // 审计创建人成立（增量19 WF-03）；owner 语义收窄为独立字段留待后续（台账记）
        let cmd = core_model::CreateCmd {
            type_code: Self::TYPE.into(),
            obj_no,
            attrs,
            owner: actor,
            secret_level: 0,
        };
        let obj = self.objects.create(cmd).await?;

        sqlx::query("INSERT INTO file_object(hash, size, mime) VALUES ($1, $2, $3) ON CONFLICT (hash) DO NOTHING")
            .bind(&hash)
            .bind(size)
            .bind(&mime)
            .execute(&self.pool)
            .await?;
        sqlx::query(
            "INSERT INTO file_ref(object_id, role, hash, name) VALUES ($1, 'original', $2, $3)",
        )
        .bind(obj.id)
        .bind(&hash)
        .bind(&file_name)
        .execute(&self.pool)
        .await?;

        Ok(DocDetail {
            object: obj,
            file: Some(FileInfo {
                name: file_name,
                hash,
                size,
                mime,
            }),
        })
    }

    pub async fn list(&self, q: &ObjectQuery) -> Result<(Vec<BizObject>, u64)> {
        self.objects.query(q).await
    }

    /// 修改 working 态对象属性（乐观锁；发布态由 ObjectStore 423 拒绝）。
    /// 增量8 M3'：取最新版定位——升版后 (1,0) 硬编码会导致 404。
    pub async fn update_object(
        &self,
        obj_no: &str,
        patch: &AttrMap,
        expect_rev: i32,
        actor: uuid::Uuid,
    ) -> Result<BizObject> {
        let cur = self.objects.get_latest(Self::TYPE, obj_no).await?;
        self.objects
            .update(
                Self::TYPE,
                obj_no,
                cur.major,
                cur.minor,
                patch,
                expect_rev,
                actor,
            )
            .await
    }

    /// 生命周期转发（网关 6 端点消费；权限在网关把关）。
    /// 增量8：approve 落快照时携带发布件哈希（快照 file_hash 数据面）。
    pub async fn transition(
        &self,
        store: &core_version::LifecycleStore,
        ev: core_version::LifecycleEvent,
        obj_no: &str,
        actor: uuid::Uuid,
    ) -> Result<BizObject> {
        let cur = self.objects.get_latest(Self::TYPE, obj_no).await?;
        let file_hash = self.original_file(cur.id).await?.map(|f| f.hash);
        store
            .transition_with_file(Self::TYPE, obj_no, ev, Some(actor), file_hash.as_deref())
            .await
    }

    pub async fn detail(&self, obj_no: &str) -> Result<DocDetail> {
        let object = self.objects.get_latest(Self::TYPE, obj_no).await?;
        let file = self.original_file(object.id).await?;
        Ok(DocDetail { object, file })
    }

    /// 版本历史（增量8）：当前最新版 + released 快照链（升版出口的数据面）
    pub async fn versions(
        &self,
        obj_no: &str,
    ) -> Result<(BizObject, Vec<crate::dto::VersionSnapshot>)> {
        #[derive(sqlx::FromRow)]
        struct SnapRow {
            major_ver: i32,
            minor_ver: i32,
            state: String,
            file_hash: Option<String>,
            created_at: Option<chrono::DateTime<chrono::Utc>>,
        }
        let object = self.objects.get_latest(Self::TYPE, obj_no).await?;
        let rows: Vec<SnapRow> = sqlx::query_as(
            "SELECT major_ver, minor_ver, state, file_hash, created_at FROM cm_obj_snapshot \
                 WHERE type_code = $1 AND obj_no = $2 \
                 ORDER BY major_ver DESC, minor_ver DESC",
        )
        .bind(Self::TYPE)
        .bind(obj_no)
        .fetch_all(&self.pool)
        .await?;
        let snaps = rows
            .into_iter()
            .map(|r| crate::dto::VersionSnapshot {
                major: r.major_ver,
                minor: r.minor_ver,
                state: r.state,
                file_hash: r.file_hash,
                created_at: r.created_at,
            })
            .collect();
        Ok((object, snaps))
    }

    /// (原始文件名, 只读流, 字节大小)
    pub async fn open_original(
        &self,
        obj_no: &str,
    ) -> Result<(String, Box<dyn tokio::io::AsyncRead + Send + Unpin>, i64)> {
        let object = self.objects.get_latest(Self::TYPE, obj_no).await?;
        let file = self
            .original_file(object.id)
            .await?
            .ok_or_else(|| DomainError::File(format!("对象无附件: {obj_no}")))?;
        let (stream, size) = self
            .files
            .open(&file.hash)
            .await
            .map_err(|e| DomainError::File(e.to_string()))?;
        Ok((file.name, stream, size as i64))
    }

    async fn original_file(&self, object_id: uuid::Uuid) -> Result<Option<FileInfo>> {
        let row: Option<(String, String, i64, Option<String>)> = sqlx::query_as(
            "SELECT fr.name, fr.hash, fo.size, fo.mime \
             FROM file_ref fr JOIN file_object fo ON fo.hash = fr.hash \
             WHERE fr.object_id = $1 AND fr.role = 'original' LIMIT 1",
        )
        .bind(object_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(name, hash, size, mime)| FileInfo {
            name,
            hash,
            size,
            mime,
        }))
    }
}

// ==================== WF-03 待办中心（增量19，方案 A 零新表） ====================

/// 待办盒类型：todo=待我审批（review/checked 且最后操作人非我）；done=我已办理（audit_log）；
/// mine=我发起（created_by=me）。均取每 obj_no 最新活版本。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoBox {
    Todo,
    Done,
    Mine,
}

impl TodoBox {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "todo" => Some(Self::Todo),
            "done" => Some(Self::Done),
            "mine" => Some(Self::Mine),
            _ => None,
        }
    }
}

/// 生命周期六动作（已办盒数据源过滤；与 LifecycleEvent 事件名一致）
const LC_ACTIONS: &str = "('submit','approve','reject','release','obsolete','revise')";

/// 统一行（todo/mine 与 done 盒共用；未启用字段为 NULL）
#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct TodoItem {
    pub obj_no: String,
    pub name: Option<String>,
    /// 对象当前状态（todo/mine=最新活版本态；done=联查最新态，对象已删为 NULL）
    pub state: Option<String>,
    pub major_ver: Option<i32>,
    pub minor_ver: Option<i32>,
    pub secret_level: Option<i16>,
    /// 最后操作人用户名（todo/mine 盒；即待办语境下的"提交人"）
    pub submitter: Option<String>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    /// done 盒：动作与明细（from/to/comment）
    pub action: Option<String>,
    pub at: Option<chrono::DateTime<chrono::Utc>>,
    pub detail: Option<serde_json::Value>,
}

/// 最新活版本 CTE：每 obj_no 取 major/minor/iteration 最大行（软删排除；仅 drawing 域）
const LATEST_CTE: &str = "latest AS ( \
     SELECT DISTINCT ON (obj_no) obj_no, attrs, state, major_ver, minor_ver, secret_level, \
            created_by, updated_by, updated_at \
     FROM cm_object \
     WHERE type_id = (SELECT id FROM cm_type WHERE code = 'drawing') AND deleted_at IS NULL \
     ORDER BY obj_no, major_ver DESC, minor_ver DESC, iteration DESC)";

impl DocService {
    /// WF-03 待办中心三盒查询（ABAC 密级过滤；分页）。
    /// - Todo：state IN (review,checked) 且 secret≤上限 且 updated_by IS DISTINCT FROM me
    ///   （与 strict maker-checker 呼应——自己的提交不出现在自己的待办）。
    /// - Done：audit_log 中 username=me 的 lifecycle 六动作行（依赖 do_transition/im 回调显式埋点）。
    /// - Mine：created_by=me。
    pub async fn todos(
        &self,
        b: TodoBox,
        me: uuid::Uuid,
        username: &str,
        max_secret_level: i16,
        page: u32,
        size: u32,
    ) -> Result<(Vec<TodoItem>, i64)> {
        let off = i64::from(page.saturating_sub(1)) * i64::from(size);
        match b {
            TodoBox::Todo | TodoBox::Mine => {
                let cond = if b == TodoBox::Todo {
                    "l.state IN ('review','checked') AND l.secret_level <= $1 \
                     AND (l.updated_by IS DISTINCT FROM $2)"
                } else {
                    "l.created_by = $2 AND l.secret_level <= $1"
                };
                let list_sql = format!(
                    "WITH {LATEST_CTE} \
                     SELECT l.obj_no, l.attrs->>'name' AS name, l.state, \
                            l.major_ver, l.minor_ver, l.secret_level, \
                            u.username AS submitter, l.updated_at, \
                            NULL::text AS action, NULL::timestamptz AS at, NULL::jsonb AS detail \
                     FROM latest l LEFT JOIN sec_user u ON u.id = l.updated_by \
                     WHERE {cond} ORDER BY l.updated_at DESC LIMIT $3 OFFSET $4"
                );
                let rows: Vec<TodoItem> = sqlx::query_as(&list_sql)
                    .bind(max_secret_level)
                    .bind(me)
                    .bind(size as i64)
                    .bind(off)
                    .fetch_all(&self.pool)
                    .await?;
                let count_sql =
                    format!("WITH {LATEST_CTE} SELECT count(*) FROM latest l WHERE {cond}");
                let total: i64 = sqlx::query_scalar(&count_sql)
                    .bind(max_secret_level)
                    .bind(me)
                    .fetch_one(&self.pool)
                    .await?;
                Ok((rows, total))
            }
            TodoBox::Done => {
                let list_sql = format!(
                    "SELECT a.object AS obj_no, l.name, l.state, \
                            NULL::int4 AS major_ver, NULL::int4 AS minor_ver, NULL::int2 AS secret_level, \
                            NULL::text AS submitter, NULL::timestamptz AS updated_at, \
                            a.action, a.at, a.detail \
                     FROM audit_log a \
                     LEFT JOIN LATERAL ( \
                       SELECT DISTINCT ON (obj_no) obj_no, state, attrs->>'name' AS name \
                       FROM cm_object \
                       WHERE obj_no = a.object AND deleted_at IS NULL \
                       ORDER BY obj_no, major_ver DESC, minor_ver DESC, iteration DESC \
                     ) l ON true \
                     WHERE a.username = $1 AND a.action IN {LC_ACTIONS} AND a.object IS NOT NULL \
                     ORDER BY a.id DESC LIMIT $2 OFFSET $3"
                );
                let rows: Vec<TodoItem> = sqlx::query_as(&list_sql)
                    .bind(username)
                    .bind(size as i64)
                    .bind(off)
                    .fetch_all(&self.pool)
                    .await?;
                let total: i64 = sqlx::query_scalar(&format!(
                    "SELECT count(*) FROM audit_log \
                     WHERE username = $1 AND action IN {LC_ACTIONS} AND object IS NOT NULL"
                ))
                .bind(username)
                .fetch_one(&self.pool)
                .await?;
                Ok((rows, total))
            }
        }
    }
}


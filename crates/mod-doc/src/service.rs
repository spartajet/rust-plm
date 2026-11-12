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
        self.upload_staged(obj_no, attrs, file_name, mime, hash, size)
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
        obj_no: String,
        attrs: AttrMap,
        file_name: String,
        mime: Option<String>,
        hash: String,
        size: i64,
    ) -> Result<DocDetail> {
        if file_name.trim().is_empty() {
            return Err(DomainError::FieldRequired("file.filename".into()));
        }
        let cmd = core_model::CreateCmd {
            type_code: Self::TYPE.into(),
            obj_no,
            attrs,
            owner: None,
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

    /// 修改 working 态对象属性（乐观锁；发布态由 ObjectStore 423 拒绝）
    pub async fn update_object(
        &self,
        obj_no: &str,
        patch: &AttrMap,
        expect_rev: i32,
        actor: uuid::Uuid,
    ) -> Result<BizObject> {
        self.objects
            .update(Self::TYPE, obj_no, 1, 0, patch, expect_rev, actor)
            .await
    }

    /// 生命周期转发（Task2：持 LifecycleStore 后实现；权限在网关把关）
    pub async fn transition(
        &self,
        store: &core_version::LifecycleStore,
        ev: core_version::LifecycleEvent,
        obj_no: &str,
    ) -> Result<BizObject> {
        store.transition(Self::TYPE, obj_no, ev, None).await
    }

    pub async fn detail(&self, obj_no: &str) -> Result<DocDetail> {
        let object = self.objects.get(Self::TYPE, obj_no, 1, 0).await?;
        let file = self.original_file(object.id).await?;
        Ok(DocDetail { object, file })
    }

    /// (原始文件名, 只读流, 字节大小)
    pub async fn open_original(
        &self,
        obj_no: &str,
    ) -> Result<(String, Box<dyn tokio::io::AsyncRead + Send + Unpin>, i64)> {
        let object = self.objects.get(Self::TYPE, obj_no, 1, 0).await?;
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

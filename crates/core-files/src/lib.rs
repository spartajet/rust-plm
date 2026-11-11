//! core-files —— 内容寻址文件库（ADR-0012 最小版）。
//!
//! 契约：`put` 流式写入（边写边 SHA-256），落盘路径 `vol/{hash前2位}/{hash}`，
//! 已存在内容直接去重（秒传）；`open` 返回只读流 + 大小。
//! 骨架版不做：GC / S3 / 分片续传 / 病毒扫描（后续增量）。

pub mod error;
pub mod local;

pub use error::{FileError, Result};
pub use local::LocalFileStore;

use async_trait::async_trait;
use serde::Serialize;
use tokio::io::AsyncRead;

/// 文件元数据（内容寻址后的指纹）
#[derive(Debug, Clone, Serialize)]
pub struct FileMeta {
    pub hash: String,
    pub size: u64,
}

/// 文件库抽象（本地卷 / 未来 S3 同构；dyn 兼容）
#[async_trait]
pub trait FileStore: Send + Sync {
    /// 流式写入：从 reader 读到 EOF，返回内容哈希与大小
    async fn put(&self, reader: &mut (dyn AsyncRead + Send + Unpin)) -> Result<FileMeta>;
    /// 打开已存在文件：只读流 + 字节大小
    async fn open(&self, hash: &str) -> Result<(Box<dyn AsyncRead + Send + Unpin>, u64)>;
    /// 内容是否已存在
    async fn exists(&self, hash: &str) -> Result<bool>;
}

//! 本地卷实现：SHA-256 内容寻址 + 临时文件原子落盘。

use crate::{FileError, FileMeta, FileStore, Result};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

pub struct LocalFileStore {
    root: PathBuf,
    tmp_seq: AtomicU64,
}

impl LocalFileStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            tmp_seq: AtomicU64::new(0),
        }
    }

    fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    fn vol_path(&self, hash: &str) -> PathBuf {
        self.root.join("vol").join(&hash[0..2]).join(hash)
    }
}

fn valid_hash(hash: &str) -> Result<()> {
    if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(FileError::BadHash(hash.to_string()))
    }
}

#[async_trait]
impl FileStore for LocalFileStore {
    async fn put(&self, reader: &mut (dyn AsyncRead + Send + Unpin)) -> Result<FileMeta> {
        tokio::fs::create_dir_all(self.tmp_dir()).await?;
        let seq = self.tmp_seq.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = self.tmp_dir().join(format!("up-{nanos}-{seq}"));

        let mut file = tokio::fs::File::create(&tmp).await?;
        let mut hasher = Sha256::new();
        let mut size: u64 = 0;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n]).await?;
            size += n as u64;
        }
        file.flush().await?;
        drop(file);

        let hash = hex::encode(hasher.finalize());
        let target = self.vol_path(&hash);
        if tokio::fs::try_exists(&target).await? {
            // 内容已存在：去重（秒传），丢弃临时文件
            tokio::fs::remove_file(&tmp).await?;
        } else {
            tokio::fs::create_dir_all(target.parent().expect("vol path has parent")).await?;
            tokio::fs::rename(&tmp, &target).await?;
        }
        Ok(FileMeta { hash, size })
    }

    async fn open(&self, hash: &str) -> Result<(Box<dyn AsyncRead + Send + Unpin>, u64)> {
        valid_hash(hash)?;
        let path = self.vol_path(hash);
        let f = match tokio::fs::File::open(&path).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(FileError::NotFound(hash.to_string()));
            }
            Err(e) => return Err(e.into()),
        };
        let size = f.metadata().await?.len();
        Ok((Box::new(f), size))
    }

    async fn exists(&self, hash: &str) -> Result<bool> {
        valid_hash(hash)?;
        Ok(tokio::fs::try_exists(self.vol_path(hash)).await?)
    }
}

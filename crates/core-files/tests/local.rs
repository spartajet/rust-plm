//! core-files 单元测试（无 DB）：roundtrip / 去重秒传 / 哈希校验 / 不存在。

use core_files::{FileError, FileStore, LocalFileStore};
use std::io::Cursor;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;

fn unique_root(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "corefiles-{tag}-{}",
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    ))
}

fn cursor(bytes: &'static [u8]) -> Cursor<&'static [u8]> {
    Cursor::new(bytes)
}

#[tokio::test]
async fn put_open_roundtrip_and_hash() {
    let root = unique_root("rt");
    let store = LocalFileStore::new(&root);
    let data: &'static [u8] = b"hello rustplm file store";
    let meta = store.put(&mut cursor(data)).await.unwrap();
    assert_eq!(meta.size as usize, data.len());
    let expect = {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(data))
    };
    assert_eq!(meta.hash, expect);
    let (mut r, size) = store.open(&meta.hash).await.unwrap();
    let mut out = Vec::new();
    r.read_to_end(&mut out).await.unwrap();
    assert_eq!(out, data);
    assert_eq!(size, data.len() as u64);
    assert!(store.exists(&meta.hash).await.unwrap());
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn dedup_same_content_same_hash() {
    let root = unique_root("dd");
    let store = LocalFileStore::new(&root);
    let data: &'static [u8] = b"same-content";
    let m1 = store.put(&mut cursor(data)).await.unwrap();
    let m2 = store.put(&mut cursor(data)).await.unwrap();
    assert_eq!(m1.hash, m2.hash);
    // 卷内只保留一份
    assert_eq!(std::fs::read_dir(root.join("vol").join(&m1.hash[0..2])).unwrap().count(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn open_missing_and_bad_hash() {
    let root = unique_root("miss");
    let store = LocalFileStore::new(&root);
    let missing = "0".repeat(64);
    match store.open(&missing).await {
        Err(FileError::NotFound(_)) => {}
        Err(e) => panic!("期望 NotFound，得到 {e}"),
        Ok(_) => panic!("期望 NotFound，却成功打开"),
    }
    assert!(store.open("short").await.is_err());
    let _ = std::fs::remove_dir_all(&root);
}

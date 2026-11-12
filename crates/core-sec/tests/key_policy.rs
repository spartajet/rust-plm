//! 密钥解析单测（终审 I1/I2 修复轮）：
//! I2 第三档——key_dir 不可用（如路径被普通文件占用）时降级为内存随机密钥并 WARN，不得拒绝启动；
//! I1 —— jwt.key 落盘权限收紧 0600（Unix；Windows 由 NTFS 默认用户私有语义覆盖，CI linux 执行该钉）。

use core_sec::resolve_secret;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn tmp(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "coresec-fix-{tag}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn i2_unwritable_key_dir_degrades_to_ephemeral_secret() {
    // key_dir 指向一个普通文件 → create_dir_all 失败 → 必须降级 Ok(随机)，而非 Err
    let occupied = tmp("occupied");
    std::fs::write(&occupied, b"i am a file").unwrap();
    let r = resolve_secret(None, &occupied);
    assert!(
        r.is_ok(),
        "落盘失败必须降级为内存随机密钥（终审 I2），得到 {r:?}"
    );
    let s1 = r.unwrap();
    let s2 = resolve_secret(None, &occupied).unwrap();
    assert_ne!(s1, s2, "降级档为一次性随机：两次调用各自独立");
    let _ = std::fs::remove_file(&occupied);
}

#[test]
fn i1_key_file_permission_owner_only() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp("perm");
        resolve_secret(None, &dir).unwrap();
        let mode = std::fs::metadata(dir.join("jwt.key"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "jwt.key 必须仅属主可读写（终审 I1）");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[cfg(not(unix))]
    {
        // Windows：NTFS 默认 ACL 已用户私有；行为钉在 CI linux 执行
        let dir = tmp("perm-win");
        let s = resolve_secret(None, &dir).unwrap();
        assert_eq!(resolve_secret(None, &dir).unwrap(), s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

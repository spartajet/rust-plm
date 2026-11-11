use thiserror::Error;

#[derive(Debug, Error)]
pub enum FileError {
    #[error("文件不存在: {0}")]
    NotFound(String),
    #[error("非法哈希: {0}")]
    BadHash(String),
    #[error("IO 错误: {0}")]
    Io(String),
}

impl From<std::io::Error> for FileError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

pub type Result<T, E = FileError> = std::result::Result<T, E>;

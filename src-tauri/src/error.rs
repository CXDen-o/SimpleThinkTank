// 全局错误类型定义

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("数据库错误: {0}")]
    Database(#[from] sqlx::Error),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("文档解析错误: {0}")]
    Parsing(String),

    #[error("文件格式不支持: {0}")]
    UnsupportedFormat(String),

    #[error("文件过大: {size} bytes, 上限 {limit} bytes")]
    FileTooLarge { size: u64, limit: u64 },

    #[error("知识库不存在: {0}")]
    KnowledgeBaseNotFound(String),

    #[error("文档不存在: {0}")]
    DocumentNotFound(String),

    #[error("Ollama 服务未运行")]
    OllamaNotRunning,

    #[error("HTTP 请求错误: {0}")]
    Http(#[from] reqwest::Error),

    #[error("序列化错误: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("其他错误: {0}")]
    Other(#[from] anyhow::Error),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

pub type AppResult<T> = Result<T, AppError>;

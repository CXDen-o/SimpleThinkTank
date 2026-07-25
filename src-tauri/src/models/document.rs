use serde::{Deserialize, Serialize};

/// 文档状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentStatus {
    Pending,
    Parsing,
    Parsed,
    Chunking,
    Chunked,
    Vectorizing,
    Indexed,
    Failed,
}

impl ToString for DocumentStatus {
    fn to_string(&self) -> String {
        match self {
            DocumentStatus::Pending => "pending",
            DocumentStatus::Parsing => "parsing",
            DocumentStatus::Parsed => "parsed",
            DocumentStatus::Chunking => "chunking",
            DocumentStatus::Chunked => "chunked",
            DocumentStatus::Vectorizing => "vectorizing",
            DocumentStatus::Indexed => "indexed",
            DocumentStatus::Failed => "failed",
        }
        .to_string()
    }
}

/// 文档实体
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Document {
    pub id: String,
    pub knowledge_base_id: String,
    pub file_name: String,
    pub file_path: String,
    pub file_size: i64,
    pub file_type: String,
    pub content_hash: Option<String>,
    pub status: String,
    pub chunk_count: i64,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 导入文档请求
#[derive(Debug, Deserialize)]
pub struct ImportDocumentsRequest {
    pub knowledge_base_id: String,
    pub file_paths: Vec<String>,
}

/// 导入任务进度
#[derive(Debug, Clone, Serialize)]
pub struct ImportProgress {
    pub task_id: String,
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub current_file: Option<String>,
    pub status: String,
}

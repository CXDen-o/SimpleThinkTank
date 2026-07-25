use serde::{Deserialize, Serialize};

/// 知识库实体
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct KnowledgeBase {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub storage_path: String,
    pub split_strategy: String,
    pub split_config: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 创建知识库请求
#[derive(Debug, Deserialize)]
pub struct CreateKnowledgeBaseRequest {
    pub name: String,
    pub description: Option<String>,
    pub storage_path: Option<String>,
    /// 切分策略 id,如 "fixed_size" / "recursive_char" / "structural"
    #[serde(default = "default_split_strategy")]
    pub split_strategy: String,
    /// 切分参数 JSON,如 {"chunk_size":512,"overlap":50}
    #[serde(default = "default_split_config")]
    pub split_config: String,
}

fn default_split_strategy() -> String {
    "fixed_size".to_string()
}

fn default_split_config() -> String {
    r#"{"chunk_size":512,"overlap":50}"#.to_string()
}

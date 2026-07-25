use serde::{Deserialize, Serialize};

/// 文本块实体
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Chunk {
    pub id: String,
    pub document_id: String,
    pub knowledge_base_id: String,
    pub content: String,
    pub chunk_index: i64,
    pub metadata: String,
    pub created_at: String,
}

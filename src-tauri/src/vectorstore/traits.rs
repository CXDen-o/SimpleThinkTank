// VectorStore 抽象 trait

use crate::error::AppResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// 检索到的片段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedChunk {
    pub chunk_id: String,
    pub document_id: String,
    pub content: String,
    pub score: f32,
    pub metadata: serde_json::Value,
}

/// 单个待存储的向量记录
#[derive(Debug, Clone)]
pub struct VectorRecord {
    pub chunk_id: String,
    pub knowledge_base_id: String,
    pub embedding: Vec<f32>,
}

/// 向量存储 trait（async）
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// 批量插入向量(若 chunk_id 已存在则覆盖)
    async fn add_vectors(&self, records: Vec<VectorRecord>) -> AppResult<()>;

    /// 删除指定 chunk_id 的向量
    async fn delete_by_chunk(&self, chunk_id: &str) -> AppResult<()>;

    /// 删除指定文档的所有向量
    async fn delete_by_document(&self, document_id: &str) -> AppResult<()>;

    /// 删除指定知识库的所有向量
    async fn delete_by_knowledge_base(&self, kb_id: &str) -> AppResult<()>;

    /// 相似性检索(top_k)
    async fn search(
        &self,
        kb_id: &str,
        query_vector: &[f32],
        top_k: usize,
    ) -> AppResult<Vec<RetrievedChunk>>;

    /// 统计向量数
    async fn count(&self, kb_id: &str) -> AppResult<usize>;
}

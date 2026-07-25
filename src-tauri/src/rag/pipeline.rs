// RAG 核心流水线
// 查询向量化 → 相似性检索 → 上下文组装 → LLM 生成 → 答案溯源

use crate::chunking::{get_strategy, SplitStrategyId};
use crate::db::Db;
use crate::error::AppResult;
use crate::ollama::{OllamaClient, GenerateOptions, DEFAULT_CHAT_MODEL, DEFAULT_EMBEDDING_MODEL};
use crate::vectorstore::{RetrievedChunk, SqliteVecStore, VectorStore, VectorRecord};
use serde::{Deserialize, Serialize};

/// RAG 查询选项
#[derive(Debug, Clone, Deserialize)]
pub struct QueryOptions {
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: i32,
    /// 是否包含对话历史
    #[serde(default = "default_true")]
    pub use_history: bool,
}

fn default_top_k() -> usize {
    4
}
fn default_temperature() -> f32 {
    0.7
}
fn default_max_tokens() -> i32 {
    1024
}
fn default_true() -> bool {
    true
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            top_k: default_top_k(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            use_history: true,
        }
    }
}

/// RAG 答案
#[derive(Debug, Clone, Serialize)]
pub struct RagAnswer {
    pub answer: String,
    pub references: Vec<RetrievedChunk>,
}

/// RAG 流水线
pub struct RagPipeline {
    db: Db,
    ollama: OllamaClient,
    vector_store: SqliteVecStore,
    chat_model: String,
    embedding_model: String,
}

impl RagPipeline {
    pub fn new(db: Db, ollama: OllamaClient) -> Self {
        Self::with_models(
            db,
            ollama,
            DEFAULT_CHAT_MODEL.to_string(),
            DEFAULT_EMBEDDING_MODEL.to_string(),
        )
    }

    /// 指定模型名构造(支持从 settings 注入)
    pub fn with_models(
        db: Db,
        ollama: OllamaClient,
        chat_model: String,
        embedding_model: String,
    ) -> Self {
        let vector_store = SqliteVecStore::new(db.clone());
        Self {
            db,
            ollama,
            vector_store,
            chat_model,
            embedding_model,
        }
    }

    /// 对单个文档执行切分 → 落库 chunks → 批量向量化
    pub async fn index_document(
        &self,
        kb_id: &str,
        doc_id: &str,
        text: &str,
        strategy_id: &str,
        split_config: &serde_json::Value,
    ) -> AppResult<usize> {
        // 1. 切分
        let sid = SplitStrategyId::from_str(strategy_id)?;
        let strategy = get_strategy(sid);
        let ctx = crate::chunking::SplitContext::new(
            self.ollama.clone(),
            self.chat_model.clone(),
            true, // 导入路径允许降级，不阻塞批次
        );
        let chunks = strategy.split(text, split_config, &ctx).await?;
        let chunk_count = chunks.len();

        // 2. 落库 chunks(若已存在先删除)
        sqlx::query("DELETE FROM chunks WHERE document_id = ?")
            .bind(doc_id)
            .execute(&self.db)
            .await?;

        let mut chunk_ids: Vec<String> = Vec::with_capacity(chunk_count);
        for c in &chunks {
            let chunk_id = uuid::Uuid::new_v4().to_string();
            let metadata = if c.metadata.is_null() {
                "{}".to_string()
            } else {
                serde_json::to_string(&c.metadata)?
            };
            sqlx::query(
                "INSERT INTO chunks (id, document_id, knowledge_base_id, content, chunk_index, metadata) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&chunk_id)
            .bind(doc_id)
            .bind(kb_id)
            .bind(&c.text)
            .bind(c.index as i64)
            .bind(&metadata)
            .execute(&self.db)
            .await?;
            chunk_ids.push(chunk_id);
        }

        // 3. 批量向量化
        let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
        if !texts.is_empty() {
            // 分批处理避免单次请求过大
            for (idx, batch) in texts.chunks(8).enumerate() {
                let embeddings = self
                    .ollama
                    .embed_batch(&self.embedding_model, batch)
                    .await?;
                let mut records = Vec::with_capacity(embeddings.len());
                for (i, emb) in embeddings.into_iter().enumerate() {
                    let global_idx = idx * 8 + i;
                    let chunk_id = chunk_ids[global_idx].clone();
                    records.push(VectorRecord {
                        chunk_id,
                        knowledge_base_id: kb_id.to_string(),
                        embedding: emb,
                    });
                }
                // 直接调用 async VectorStore
                self.vector_store.add_vectors(records).await?;
            }
        }

        // 4. 更新文档状态
        sqlx::query("UPDATE documents SET status = 'indexed', chunk_count = ?, updated_at = ? WHERE id = ?")
            .bind(chunk_count as i64)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(doc_id)
            .execute(&self.db)
            .await?;

        Ok(chunk_count)
    }

    /// RAG 问答
    pub async fn query(
        &self,
        kb_id: &str,
        question: &str,
        history: &[HistoryMessage],
        options: &QueryOptions,
    ) -> AppResult<RagAnswer> {
        // 1. 查询向量化
        let emb_req = crate::ollama::EmbeddingRequest {
            model: self.embedding_model.clone(),
            prompt: question.to_string(),
        };
        let emb_resp = self.ollama.embed(&emb_req).await?;
        let qv = emb_resp.embedding;

        // 2. 相似性检索
        let references = self
            .vector_store
            .search(kb_id, &qv, options.top_k)
            .await?;

        // 3. 上下文组装
        let context = assemble_context(&references);
        let system_prompt = build_system_prompt(&context);
        let prompt = build_user_prompt(question, history, options.use_history);

        // 4. LLM 生成
        let gen_req = crate::ollama::GenerateRequest {
            model: self.chat_model.clone(),
            prompt,
            system: Some(system_prompt),
            context: None,
            stream: Some(false),
            think: Some(false),
            options: Some(GenerateOptions {
                temperature: Some(options.temperature),
                top_k: Some(40),
                num_predict: Some(options.max_tokens),
                top_p: None,
            }),
            keep_alive: None,
        };
        let gen_resp = self.ollama.generate(&gen_req).await?;

        Ok(RagAnswer {
            answer: gen_resp.response,
            references,
        })
    }

    /// RAG 问答(流式版本)
    /// 先执行检索,然后流式生成答案,每个 token 通过 on_token 回调推送
    /// 返回完整的 RagAnswer(含引用片段)
    pub async fn query_stream<F>(
        &self,
        kb_id: &str,
        question: &str,
        history: &[HistoryMessage],
        options: &QueryOptions,
        mut on_token: F,
    ) -> AppResult<RagAnswer>
    where
        F: FnMut(&str) -> bool,
    {
        // 1. 查询向量化
        let emb_req = crate::ollama::EmbeddingRequest {
            model: self.embedding_model.clone(),
            prompt: question.to_string(),
        };
        let emb_resp = self.ollama.embed(&emb_req).await?;
        let qv = emb_resp.embedding;

        // 2. 相似性检索
        let references = self
            .vector_store
            .search(kb_id, &qv, options.top_k)
            .await?;

        // 3. 上下文组装
        let context = assemble_context(&references);
        let system_prompt = build_system_prompt(&context);
        let prompt = build_user_prompt(question, history, options.use_history);

        // 4. LLM 流式生成
        let gen_req = crate::ollama::GenerateRequest {
            model: self.chat_model.clone(),
            prompt,
            system: Some(system_prompt),
            context: None,
            stream: Some(true),
            think: Some(false),
            options: Some(GenerateOptions {
                temperature: Some(options.temperature),
                top_k: Some(40),
                num_predict: Some(options.max_tokens),
                top_p: None,
            }),
            keep_alive: None,
        };
        let gen_resp = self.ollama.generate_stream(&gen_req, |token| on_token(token)).await?;

        Ok(RagAnswer {
            answer: gen_resp.response,
            references,
        })
    }
}

/// 历史消息(用于查询时传入)
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
}

/// 组装上下文:把检索到的片段拼接成 context 文本
fn assemble_context(refs: &[RetrievedChunk]) -> String {
    let mut parts = Vec::with_capacity(refs.len());
    for (i, r) in refs.iter().enumerate() {
        parts.push(format!("[片段 {}]\n{}", i + 1, r.content));
    }
    parts.join("\n\n---\n\n")
}

/// 构建系统提示词
fn build_system_prompt(context: &str) -> String {
    format!(
        "你是「智识库」的问答助手。请基于以下检索到的知识片段回答用户问题。\
         如果知识片段中没有相关信息,请明确说明你不知道,不要编造内容。\
         引用片段时请使用 [片段 N] 的格式标注来源。\n\n\
         知识片段:\n{}",
        context
    )
}

/// 构建用户提示词
fn build_user_prompt(question: &str, history: &[HistoryMessage], use_history: bool) -> String {
    if !use_history || history.is_empty() {
        return question.to_string();
    }
    let mut buf = String::new();
    for m in history.iter().take(6) {
        // 仅保留最近 6 条
        let role = if m.role == "user" { "用户" } else { "助手" };
        buf.push_str(&format!("{}: {}\n", role, m.content));
    }
    buf.push_str(&format!("\n当前问题: {}", question));
    buf
}

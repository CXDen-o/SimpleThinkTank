// RAG 对话相关 Tauri Command

use super::pipeline::{HistoryMessage, QueryOptions, RagPipeline};
use crate::db::DbState;
use crate::error::AppError;
use crate::models::conversation::{Conversation, Message};
use crate::ollama::commands::OllamaState;
use crate::vectorstore::RetrievedChunk;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

/// 问答请求
#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    pub kb_id: String,
    pub question: String,
    /// 流式查询标识:前端生成,随 chat-token/chat-done 事件透传,
    /// 用于切换知识库后区分新旧查询,避免旧查询事件串扰
    #[serde(default)]
    pub query_id: Option<String>,
    #[serde(default)]
    pub history: Vec<HistoryMessage>,
    #[serde(default)]
    pub options: QueryOptions,
}

/// 问答响应
#[derive(Debug, Serialize)]
pub struct QueryResponse {
    pub answer: String,
    pub references: Vec<RetrievedChunk>,
}

/// 保存对话请求
#[derive(Debug, Deserialize)]
pub struct SaveConversationRequest {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
}

/// 对知识库发起一次问答
#[tauri::command]
pub async fn query_knowledge_base(
    db: State<'_, DbState>,
    ollama_state: State<'_, OllamaState>,
    req: QueryRequest,
) -> Result<QueryResponse, AppError> {
    let pool = db.0.clone();
    let manager = ollama_state.get().await;
    // 确保 Ollama 服务可用
    if !manager.is_running().await {
        return Err(crate::error::AppError::OllamaNotRunning);
    }
    let client = manager.client_clone();
    let pipeline = RagPipeline::new(pool, client);
    let answer = pipeline
        .query(&req.kb_id, &req.question, &req.history, &req.options)
        .await?;
    Ok(QueryResponse {
        answer: answer.answer,
        references: answer.references,
    })
}

/// 流式问答事件 payload
#[derive(Debug, Clone, Serialize)]
pub struct ChatTokenPayload {
    /// token 文本片段
    pub token: String,
    /// 关联的查询标识(透传 QueryRequest.query_id)
    pub query_id: Option<String>,
}

/// 流式问答完成事件 payload
#[derive(Debug, Clone, Serialize)]
pub struct ChatDonePayload {
    /// 完整答案
    pub answer: String,
    /// 引用片段
    pub references: Vec<RetrievedChunk>,
    /// 错误信息(若成功则为 None)
    pub error: Option<String>,
    /// 关联的查询标识(透传 QueryRequest.query_id)
    pub query_id: Option<String>,
}

/// 对知识库发起一次流式问答
/// 事件:
///   - "chat-token" { token: "..." }  每个 token
///   - "chat-done"  { answer, references, error }  完成/失败
#[tauri::command]
pub async fn query_knowledge_base_stream(
    app: AppHandle,
    db: State<'_, DbState>,
    ollama_state: State<'_, OllamaState>,
    req: QueryRequest,
) -> Result<(), AppError> {
    let pool = db.0.clone();
    let manager = ollama_state.get().await;
    if !manager.is_running().await {
        return Err(crate::error::AppError::OllamaNotRunning);
    }
    let client = manager.client_clone();
    let pipeline = RagPipeline::new(pool, client);

    let result = pipeline
        .query_stream(&req.kb_id, &req.question, &req.history, &req.options, |token| {
            // 推送 token 事件
            let _ = app.emit("chat-token", ChatTokenPayload {
                token: token.to_string(),
                query_id: req.query_id.clone(),
            });
            true // 不主动终止
        })
        .await;

    let payload = match result {
        Ok(answer) => ChatDonePayload {
            answer: answer.answer,
            references: answer.references,
            error: None,
            query_id: req.query_id.clone(),
        },
        Err(e) => {
            let msg = e.to_string();
            ChatDonePayload {
                answer: String::new(),
                references: vec![],
                error: Some(msg),
                query_id: req.query_id.clone(),
            }
        }
    };
    let _ = app.emit("chat-done", &payload);
    Ok(())
}

/// 获取指定知识库下的对话列表
#[tauri::command]
pub async fn get_conversations(
    db: State<'_, DbState>,
    kb_id: String,
) -> Result<Vec<Conversation>, AppError> {
    let convs: Vec<Conversation> = sqlx::query_as::<_, Conversation>(
        "SELECT * FROM conversations WHERE knowledge_base_id = ? ORDER BY updated_at DESC",
    )
    .bind(kb_id)
    .fetch_all(&db.0)
    .await?;
    Ok(convs)
}

/// 获取对话下的消息
#[tauri::command]
pub async fn get_messages(
    db: State<'_, DbState>,
    conversation_id: String,
) -> Result<Vec<Message>, AppError> {
    let msgs: Vec<Message> = sqlx::query_as::<_, Message>(
        "SELECT * FROM messages WHERE conversation_id = ? ORDER BY created_at ASC",
    )
    .bind(conversation_id)
    .fetch_all(&db.0)
    .await?;
    Ok(msgs)
}

/// 保存或更新对话及消息
#[tauri::command]
pub async fn save_conversation(
    db: State<'_, DbState>,
    req: SaveConversationRequest,
) -> Result<(), AppError> {
    let mut tx = db.0.begin().await?;

    // upsert conversation
    sqlx::query(
        "INSERT INTO conversations (id, knowledge_base_id, title, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET title = excluded.title, updated_at = excluded.updated_at",
    )
    .bind(&req.conversation.id)
    .bind(&req.conversation.knowledge_base_id)
    .bind(&req.conversation.title)
    .bind(&req.conversation.created_at)
    .bind(&req.conversation.updated_at)
    .execute(&mut *tx)
    .await?;

    // 删除旧消息
    sqlx::query("DELETE FROM messages WHERE conversation_id = ?")
        .bind(&req.conversation.id)
        .execute(&mut *tx)
        .await?;

    // 写入新消息
    for m in &req.messages {
        sqlx::query(
            "INSERT INTO messages (id, conversation_id, role, content, \"references\", created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&m.id)
        .bind(&m.conversation_id)
        .bind(&m.role)
        .bind(&m.content)
        .bind(&m.references)
        .bind(&m.created_at)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// 删除对话
#[tauri::command]
pub async fn delete_conversation(
    db: State<'_, DbState>,
    conversation_id: String,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM conversations WHERE id = ?")
        .bind(conversation_id)
        .execute(&db.0)
        .await?;
    Ok(())
}

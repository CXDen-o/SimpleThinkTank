// Ollama 模型服务管理模块
// 封装对本地 Ollama 服务的 HTTP 调用:嵌入、生成、模型管理、进程管理

pub mod client;
pub mod process;
pub mod commands;

pub use client::{EmbeddingRequest, GenerateRequest, GenerateOptions, OllamaClient};

/// 默认对话模型(锁定,UI 不允许随意切换)
pub const DEFAULT_CHAT_MODEL: &str = "qwen3:1.7b";

/// 默认嵌入模型(全局锁定,所有知识库共用)
pub const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";

/// Ollama 服务默认地址
pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";

/// 从 settings 解析生效的对话模型名(当前仍锁定,保留扩展点)
pub fn effective_chat_model(_s: &crate::config::settings::AppSettings) -> &'static str {
    DEFAULT_CHAT_MODEL
}

/// 从 settings 解析生效的嵌入模型名(当前仍锁定,保留扩展点)
pub fn effective_embedding_model(_s: &crate::config::settings::AppSettings) -> &'static str {
    DEFAULT_EMBEDDING_MODEL
}

/// 从 settings 解析生效的 Ollama URL(空串回退默认)
pub fn effective_ollama_url(s: &crate::config::settings::AppSettings) -> String {
    s.effective_ollama_url().to_string()
}

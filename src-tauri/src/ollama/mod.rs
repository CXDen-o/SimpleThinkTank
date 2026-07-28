// Ollama 模型服务管理模块
// 封装对本地 Ollama 服务的 HTTP 调用:嵌入、生成、模型管理、进程管理

pub mod client;
pub mod process;
pub mod commands;

pub use client::{EmbeddingRequest, GenerateRequest, GenerateOptions, OllamaClient};

/// 默认对话模型(未配置时的回退值)
pub const DEFAULT_CHAT_MODEL: &str = "qwen3:1.7b";

/// 默认嵌入模型(全局锁定,所有知识库共用;向量维度与 vec 表结构绑定,不可切换)
pub const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";

/// Ollama 服务默认地址
pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";

/// 推荐对话模型(分级;均为 Ollama 官方仓库真实存在的 tag)
/// 嵌入模型不在此列——全局锁定,见 DEFAULT_EMBEDDING_MODEL
pub const RECOMMENDED_CHAT_MODELS: &[RecommendedChatModel] = &[
    RecommendedChatModel {
        name: "qwen3:1.7b",
        tier_label: "轻量(默认)",
        size_hint: "约 1.4GB,8GB 内存可运行",
    },
    RecommendedChatModel {
        name: "qwen3:4b",
        tier_label: "均衡",
        size_hint: "约 2.6GB,建议 16GB 内存",
    },
    RecommendedChatModel {
        name: "qwen3:8b",
        tier_label: "高配",
        size_hint: "约 5.2GB,建议 16GB 内存 + 8GB 显存",
    },
    RecommendedChatModel {
        name: "deepseek-r1:14b",
        tier_label: "旗舰(推理增强)",
        size_hint: "约 9GB,建议 32GB 内存或 12GB+ 显存",
    },
];

/// 推荐对话模型条目(静态配置,供前端渲染候选项)
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecommendedChatModel {
    pub name: &'static str,
    pub tier_label: &'static str,
    pub size_hint: &'static str,
}

/// 从 settings 解析生效的对话模型名(空串回退默认)
pub fn effective_chat_model(s: &crate::config::settings::AppSettings) -> String {
    s.effective_chat_model().to_string()
}

/// 从 settings 解析生效的嵌入模型名(锁定,保留扩展点)
pub fn effective_embedding_model(_s: &crate::config::settings::AppSettings) -> &'static str {
    DEFAULT_EMBEDDING_MODEL
}

/// 从 settings 解析生效的 Ollama URL(空串回退默认)
pub fn effective_ollama_url(s: &crate::config::settings::AppSettings) -> String {
    s.effective_ollama_url().to_string()
}

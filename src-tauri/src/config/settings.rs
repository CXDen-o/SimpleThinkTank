// 应用设置管理:读写 app_settings 表
// 支持镜像源、代理、模型目录、下载重试等可配置项

use crate::db::Db;
use crate::error::AppResult;
use serde::{Deserialize, Serialize};

/// 应用设置(快照形式,启动时加载到内存)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Ollama 服务地址,空串表示默认 http://127.0.0.1:11434
    #[serde(default)]
    pub ollama_base_url: String,
    /// 镜像源 URL,例如 https://hf-mirror.com/ollama
    /// 注入 OLLAMA_REGISTRY 环境变量
    #[serde(default)]
    pub ollama_registry: String,
    /// HTTPS 代理地址,例如 http://127.0.0.1:7890
    /// 同时注入 OLLAMA 进程的 HTTPS_PROXY 和 Rust reqwest 客户端
    #[serde(default)]
    pub https_proxy: String,
    /// 是否使用自定义模型目录(AppPaths::models_dir)
    #[serde(default)]
    pub use_custom_models_dir: bool,
    /// 下载最大重试次数(含首次)
    #[serde(default = "default_max_retries")]
    pub download_max_retries: u32,
    /// 连接超时(秒)
    #[serde(default = "default_connect_timeout")]
    pub download_connect_timeout_secs: u64,
    /// 请求超时(秒)
    #[serde(default = "default_request_timeout")]
    pub download_request_timeout_secs: u64,
    /// 问答参数 JSON,如 {"top_k":4,"temperature":0.7,"max_tokens":1024,"use_history":true}
    #[serde(default = "default_query_options")]
    pub query_options: String,
    /// 对话模型名,空串表示默认(DEFAULT_CHAT_MODEL)
    /// 嵌入模型全局锁定不可切换(向量维度与表结构绑定)
    #[serde(default)]
    pub chat_model: String,
}

fn default_max_retries() -> u32 {
    3
}
fn default_connect_timeout() -> u64 {
    30
}
fn default_request_timeout() -> u64 {
    600
}
fn default_query_options() -> String {
    r#"{"top_k":4,"temperature":0.7,"max_tokens":1024,"use_history":true}"#.to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ollama_base_url: String::new(),
            ollama_registry: String::new(),
            https_proxy: String::new(),
            use_custom_models_dir: false,
            download_max_retries: default_max_retries(),
            download_connect_timeout_secs: default_connect_timeout(),
            download_request_timeout_secs: default_request_timeout(),
            query_options: default_query_options(),
            chat_model: String::new(),
        }
    }
}

impl AppSettings {
    /// 生效的 Ollama URL(空串回退到默认)
    pub fn effective_ollama_url(&self) -> &str {
        if self.ollama_base_url.is_empty() {
            crate::ollama::DEFAULT_OLLAMA_URL
        } else {
            &self.ollama_base_url
        }
    }

    /// 是否启用代理
    pub fn proxy_enabled(&self) -> bool {
        !self.https_proxy.is_empty()
    }

    /// 是否启用镜像源
    pub fn registry_enabled(&self) -> bool {
        !self.ollama_registry.is_empty()
    }

    /// 生效的对话模型名(空串回退到默认 DEFAULT_CHAT_MODEL)
    pub fn effective_chat_model(&self) -> &str {
        if self.chat_model.is_empty() {
            crate::ollama::DEFAULT_CHAT_MODEL
        } else {
            &self.chat_model
        }
    }
}

/// 设置 DAO:读写 app_settings 表
pub struct SettingsDao<'a> {
    db: &'a Db,
}

impl<'a> SettingsDao<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// 加载所有设置(缺失字段用默认值)
    pub async fn load_all(&self) -> AppResult<AppSettings> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT key, value FROM app_settings").fetch_all(self.db).await?;
        let mut s = AppSettings::default();
        for (k, v) in rows {
            match k.as_str() {
                "ollama_base_url" => s.ollama_base_url = v,
                "ollama_registry" => s.ollama_registry = v,
                "https_proxy" => s.https_proxy = v,
                "use_custom_models_dir" => s.use_custom_models_dir = v == "true",
                "download_max_retries" => s.download_max_retries = v.parse().unwrap_or(3),
                "download_connect_timeout_secs" => {
                    s.download_connect_timeout_secs = v.parse().unwrap_or(30)
                }
                "download_request_timeout_secs" => {
                    s.download_request_timeout_secs = v.parse().unwrap_or(600)
                }
                "query_options" => s.query_options = v,
                "chat_model" => s.chat_model = v,
                _ => {}
            }
        }
        Ok(s)
    }

    /// 批量更新(原子事务)
    pub async fn update_all(&self, settings: &AppSettings) -> AppResult<()> {
        let mut tx = self.db.begin().await?;
        let pairs = [
            ("ollama_base_url", settings.ollama_base_url.as_str()),
            ("ollama_registry", settings.ollama_registry.as_str()),
            ("https_proxy", settings.https_proxy.as_str()),
            (
                "use_custom_models_dir",
                if settings.use_custom_models_dir {
                    "true"
                } else {
                    "false"
                },
            ),
            (
                "download_max_retries",
                &settings.download_max_retries.to_string(),
            ),
            (
                "download_connect_timeout_secs",
                &settings.download_connect_timeout_secs.to_string(),
            ),
            (
                "download_request_timeout_secs",
                &settings.download_request_timeout_secs.to_string(),
            ),
            ("query_options", settings.query_options.as_str()),
            ("chat_model", settings.chat_model.as_str()),
        ];
        for (k, v) in pairs {
            sqlx::query(
                "INSERT INTO app_settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            )
            .bind(k)
            .bind(v)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

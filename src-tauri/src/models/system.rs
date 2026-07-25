// 系统信息模型

use serde::{Deserialize, Serialize};

/// 系统信息(应用启动时返回)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    /// Ollama 是否已安装
    pub ollama_installed: bool,
    /// Ollama 服务是否在运行
    pub ollama_running: bool,
    /// 默认模型(chat + embedding)是否已下载
    pub default_models_available: bool,
}

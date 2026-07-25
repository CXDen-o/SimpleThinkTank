// 应用路径管理模块
// 统一管理 ~/Documents/Zhishiku/ 下的目录结构

use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub mod settings;
pub mod state;
pub use state::SettingsState;

/// 应用路径管理
pub struct AppPaths;

impl AppPaths {
    /// 根存储目录: ~/Documents/Zhishiku/
    pub fn root_dir() -> PathBuf {
        let docs = dirs::document_dir().unwrap_or_else(|| dirs::home_dir().unwrap());
        docs.join("Zhishiku")
    }

    /// 知识库存储目录: ~/Documents/Zhishiku/knowledge_bases/{kb_id}/
    pub fn knowledge_base_dir(kb_id: &str) -> PathBuf {
        Self::root_dir().join("knowledge_bases").join(kb_id)
    }

    /// 知识库原始文档目录: .../{kb_id}/documents/
    pub fn documents_dir(kb_id: &str) -> PathBuf {
        Self::knowledge_base_dir(kb_id).join("documents")
    }

    /// 知识库向量索引目录: .../{kb_id}/vector_index/
    pub fn vector_index_dir(kb_id: &str) -> PathBuf {
        Self::knowledge_base_dir(kb_id).join("vector_index")
    }

    /// 知识库配置文件: .../{kb_id}/config.json
    pub fn knowledge_base_config(kb_id: &str) -> PathBuf {
        Self::knowledge_base_dir(kb_id).join("config.json")
    }

    /// 全局数据库文件: ~/Documents/Zhishiku/zhishiku.db
    pub fn database_path() -> PathBuf {
        Self::root_dir().join("zhishiku.db")
    }

    /// 日志目录: ~/Documents/Zhishiku/logs/
    pub fn logs_dir() -> PathBuf {
        Self::root_dir().join("logs")
    }

    /// 模型缓存目录: ~/Documents/Zhishiku/models/
    pub fn models_dir() -> PathBuf {
        Self::root_dir().join("models")
    }

    /// 确保所有基础目录存在
    pub fn ensure_dirs() -> std::io::Result<()> {
        std::fs::create_dir_all(Self::root_dir())?;
        std::fs::create_dir_all(Self::logs_dir())?;
        std::fs::create_dir_all(Self::models_dir())?;
        std::fs::create_dir_all(Self::root_dir().join("knowledge_bases"))?;
        Ok(())
    }

    /// 为新知识库创建目录结构
    pub fn ensure_knowledge_base_dirs(kb_id: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(Self::knowledge_base_dir(kb_id))?;
        std::fs::create_dir_all(Self::documents_dir(kb_id))?;
        std::fs::create_dir_all(Self::vector_index_dir(kb_id))?;
        Ok(())
    }

    /// 删除知识库目录（级联删除）
    pub fn remove_knowledge_base_dir(kb_id: &str) -> std::io::Result<()> {
        let dir = Self::knowledge_base_dir(kb_id);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }
}

/// 从 Tauri AppHandle 获取应用数据目录（备用方案）
pub fn app_data_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| AppPaths::root_dir())
}

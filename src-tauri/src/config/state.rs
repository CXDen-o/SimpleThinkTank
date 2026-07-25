// 应用设置状态(可热更新)
// Tauri State 注入,内存态 + DB 持久化

use crate::config::settings::AppSettings;
use crate::db::Db;
use crate::error::AppResult;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 全局设置状态(可热更新)
#[derive(Clone)]
pub struct SettingsState {
    inner: Arc<RwLock<AppSettings>>,
    db: Db,
}

impl SettingsState {
    pub fn new(db: Db, initial: AppSettings) -> Self {
        Self {
            inner: Arc::new(RwLock::new(initial)),
            db,
        }
    }

    pub async fn get(&self) -> AppSettings {
        self.inner.read().await.clone()
    }

    pub async fn update(&self, settings: AppSettings) -> AppResult<()> {
        let dao = crate::config::settings::SettingsDao::new(&self.db);
        dao.update_all(&settings).await?;
        *self.inner.write().await = settings;
        Ok(())
    }
}

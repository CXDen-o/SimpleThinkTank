// 知识库管理 Command

use crate::config::AppPaths;
use crate::db::DbState;
use crate::error::AppError;
use crate::models::knowledge_base::{CreateKnowledgeBaseRequest, KnowledgeBase};
use tauri::State;
use uuid::Uuid;

/// 获取所有知识库列表
#[tauri::command]
pub async fn get_knowledge_bases(
    db: State<'_, DbState>,
) -> Result<Vec<KnowledgeBase>, AppError> {
    let kbs: Vec<KnowledgeBase> = sqlx::query_as::<_, KnowledgeBase>(
        "SELECT * FROM knowledge_bases ORDER BY updated_at DESC",
    )
    .fetch_all(&db.0)
    .await?;
    Ok(kbs)
}

/// 创建知识库
#[tauri::command]
pub async fn create_knowledge_base(
    db: State<'_, DbState>,
    req: CreateKnowledgeBaseRequest,
) -> Result<KnowledgeBase, AppError> {
    let id = Uuid::new_v4().to_string();
    let storage_path = req
        .storage_path
        .unwrap_or_else(|| AppPaths::knowledge_base_dir(&id).to_string_lossy().to_string());

    // 创建目录
    AppPaths::ensure_knowledge_base_dirs(&id)?;

    let kb = KnowledgeBase {
        id: id.clone(),
        name: req.name,
        description: req.description,
        storage_path,
        split_strategy: req.split_strategy,
        split_config: req.split_config,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    sqlx::query(
        "INSERT INTO knowledge_bases (id, name, description, storage_path, split_strategy, split_config, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&kb.id)
    .bind(&kb.name)
    .bind(&kb.description)
    .bind(&kb.storage_path)
    .bind(&kb.split_strategy)
    .bind(&kb.split_config)
    .bind(&kb.created_at)
    .bind(&kb.updated_at)
    .execute(&db.0)
    .await?;

    tracing::info!("知识库已创建: id={}, name={}", kb.id, kb.name);
    Ok(kb)
}

/// 重命名知识库
#[tauri::command]
pub async fn rename_knowledge_base(
    db: State<'_, DbState>,
    kb_id: String,
    new_name: String,
) -> Result<(), AppError> {
    if new_name.trim().is_empty() {
        return Err(AppError::Other(anyhow::anyhow!("知识库名称不能为空")));
    }
    let result = sqlx::query(
        "UPDATE knowledge_bases SET name = ?, updated_at = ? WHERE id = ?",
    )
    .bind(new_name.trim())
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(&kb_id)
    .execute(&db.0)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::KnowledgeBaseNotFound(kb_id));
    }
    tracing::info!("知识库已重命名: id={}, new_name={}", kb_id, new_name);
    Ok(())
}

/// 删除知识库（级联删除文档、向量、目录）
#[tauri::command]
pub async fn delete_knowledge_base(
    db: State<'_, DbState>,
    kb_id: String,
) -> Result<(), AppError> {
    // 先查存在
    let exists: Option<(String,)> =
        sqlx::query_as("SELECT id FROM knowledge_bases WHERE id = ?")
            .bind(&kb_id)
            .fetch_optional(&db.0)
            .await?;

    if exists.is_none() {
        return Err(AppError::KnowledgeBaseNotFound(kb_id));
    }

    // 删除数据库记录（级联）
    sqlx::query("DELETE FROM knowledge_bases WHERE id = ?")
        .bind(&kb_id)
        .execute(&db.0)
        .await?;

    // 删除文件目录
    AppPaths::remove_knowledge_base_dir(&kb_id)?;

    tracing::info!("知识库已删除: id={}", kb_id);
    Ok(())
}

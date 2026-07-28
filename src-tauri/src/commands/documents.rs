// 文档管理 Command

use crate::chunking::SplitStrategyId;
use crate::db::DbState;
use crate::error::{AppError, AppResult};
use crate::models::document::{Document, ImportDocumentsRequest, ImportProgress};
use crate::models::knowledge_base::KnowledgeBase;
use crate::ollama::commands::OllamaState;
use crate::parsing;
use crate::rag::RagPipeline;
use sha2::{Digest, Sha256};
use sqlx::Sqlite;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// 导入任务的最大文件大小（100MB）
const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;

/// 导入任务句柄:进度 + 取消令牌
struct ImportTask {
    progress: ImportProgress,
    cancel: CancellationToken,
}

/// 全局导入任务表
static IMPORT_TASKS: std::sync::LazyLock<Mutex<HashMap<String, ImportTask>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// 支持的文档扩展名
const SUPPORTED_EXTENSIONS: &[&str] = &["pdf", "docx", "txt", "md", "markdown"];

/// 递归扫描路径列表,将文件夹展开为文件列表
/// - 文件:直接加入
/// - 文件夹:递归 BFS 扫描其中支持格式的文件
fn expand_paths(paths: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut queue: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    while let Some(p) = queue.pop() {
        if p.is_file() {
            result.push(p.to_string_lossy().to_string());
        } else if p.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&p) {
                for entry in entries.flatten() {
                    let child = entry.path();
                    if child.is_file() {
                        // 检查扩展名是否支持
                        let ext_ok = child
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_lowercase().as_str()))
                            .unwrap_or(false);
                        if ext_ok {
                            result.push(child.to_string_lossy().to_string());
                        }
                    } else if child.is_dir() {
                        queue.push(child);
                    }
                }
            }
        }
    }
    result
}

/// 导入文档(异步处理,立即返回 task_id)
/// file_paths 可包含文件和文件夹,文件夹会递归扫描支持格式的文件
#[tauri::command]
pub async fn import_documents(
    app: AppHandle,
    db: State<'_, DbState>,
    ollama_state: State<'_, OllamaState>,
    req: ImportDocumentsRequest,
) -> Result<String, AppError> {
    let task_id = Uuid::new_v4().to_string();

    // 展开文件夹为文件列表
    let file_paths = expand_paths(&req.file_paths);
    let total = file_paths.len();
    if total == 0 {
        return Err(AppError::Other(anyhow::anyhow!(
            "未找到可导入的文档(支持 PDF/DOCX/TXT/MD)"
        )));
    }

    // 初始化进度 + 取消令牌
    let cancel = CancellationToken::new();
    IMPORT_TASKS.lock().unwrap().insert(
        task_id.clone(),
        ImportTask {
            progress: ImportProgress {
                task_id: task_id.clone(),
                total,
                completed: 0,
                failed: 0,
                current_file: None,
                status: "running".to_string(),
            },
            cancel: cancel.clone(),
        },
    );

    let kb_id = req.knowledge_base_id.clone();
    let pool = db.0.clone();
    let task_id_clone = task_id.clone();
    let app_clone = app.clone();
    let ollama_manager = ollama_state.get().await;
    // 生效对话模型(切分策略中的 LLM 步骤使用;嵌入模型全局锁定)
    let chat_model = crate::ollama::effective_chat_model(&ollama_manager.settings_snapshot());

    // 读取知识库切分策略
    let kb: KnowledgeBase =
        sqlx::query_as::<_, KnowledgeBase>("SELECT * FROM knowledge_bases WHERE id = ?")
            .bind(&kb_id)
            .fetch_one(&pool)
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("读取知识库失败: {}", e)))?;
    let split_strategy = kb.split_strategy.clone();
    let split_config: serde_json::Value =
        serde_json::from_str(&kb.split_config).unwrap_or(serde_json::json!({}));
    // 预先校验策略 id 合法
    let _ = SplitStrategyId::from_str(&split_strategy)?;

    // 后台异步处理
    tokio::spawn(async move {
        let mut cancelled = false;
        for file_path in file_paths {
            // 取消检查点:每个文件开始前响应取消
            if cancel.is_cancelled() {
                cancelled = true;
                tracing::info!("导入任务 {} 已被用户取消", task_id_clone);
                break;
            }

            let path = PathBuf::from(&file_path);
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            // 更新当前文件并推送进度(让前端显示"正在处理: xxx")
            {
                let mut tasks = IMPORT_TASKS.lock().unwrap();
                if let Some(t) = tasks.get_mut(&task_id_clone) {
                    t.progress.current_file = Some(file_name.clone());
                }
            }
            // 推送"开始处理"进度事件
            if let Some(p) = IMPORT_TASKS
                .lock()
                .unwrap()
                .get(&task_id_clone)
                .map(|t| t.progress.clone())
            {
                let _ = app_clone.emit("import-progress", &p);
            }

            // 尝试拉起 Ollama(失败时降级为仅解析,不阻塞导入)
            let _ = ollama_manager.start().await;
            let ollama_ready = ollama_manager.is_running().await;
            let client = ollama_manager.client_clone();

            let result = process_single_document(
                &pool,
                &kb_id,
                &path,
                &split_strategy,
                &split_config,
                ollama_ready,
                client,
                &chat_model,
            )
            .await;

            // 更新进度
            {
                let mut tasks = IMPORT_TASKS.lock().unwrap();
                let p = &mut tasks.get_mut(&task_id_clone).unwrap().progress;
                match &result {
                    Ok(_) => p.completed += 1,
                    Err(e) => {
                        p.failed += 1;
                        tracing::error!("文档导入失败 {}: {}", file_name, e);
                    }
                }
            }

            // 推送进度事件到前端
            let current_progress = IMPORT_TASKS
                .lock()
                .unwrap()
                .get(&task_id_clone)
                .map(|t| t.progress.clone());
            if let Some(p) = current_progress {
                let _ = app_clone.emit("import-progress", &p);
            }
        }

        // 标记完成/取消
        let mut tasks = IMPORT_TASKS.lock().unwrap();
        if let Some(t) = tasks.get_mut(&task_id_clone) {
            t.progress.status = if cancelled {
                "cancelled".to_string()
            } else {
                "completed".to_string()
            };
            t.progress.current_file = None;
        }
        let final_progress = tasks.get(&task_id_clone).map(|t| t.progress.clone());
        drop(tasks);

        if let Some(p) = final_progress {
            let _ = app_clone.emit("import-progress", &p);
        }
    });

    Ok(task_id)
}

/// 处理单个文档：校验 → 解析 → 切分 → 向量化 → 落库
async fn process_single_document(
    pool: &sqlx::Pool<Sqlite>,
    kb_id: &str,
    path: &Path,
    split_strategy: &str,
    split_config: &serde_json::Value,
    ollama_ready: bool,
    ollama_client: crate::ollama::OllamaClient,
    chat_model: &str,
) -> AppResult<()> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // 文件大小校验
    let metadata = tokio::fs::metadata(path).await?;
    let file_size = metadata.len();
    if file_size > MAX_FILE_SIZE {
        return Err(AppError::FileTooLarge {
            size: file_size,
            limit: MAX_FILE_SIZE,
        });
    }

    // 格式探测
    let format = parsing::FileFormat::detect(path)?;
    let file_type = format.as_str().to_string();

    // 计算内容哈希
    let bytes = tokio::fs::read(path).await?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let content_hash = format!("{:x}", hasher.finalize());

    // 查重：同 KB 内相同哈希的文档跳过
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM documents WHERE knowledge_base_id = ? AND content_hash = ?")
            .bind(kb_id)
            .bind(&content_hash)
            .fetch_optional(pool)
            .await?;
    if existing.is_some() {
        tracing::info!("文档已存在（哈希重复），跳过: {}", file_name);
        return Ok(());
    }

    let doc_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // 先创建 parsing 记录
    sqlx::query(
        "INSERT INTO documents (id, knowledge_base_id, file_name, file_path, file_size, file_type, content_hash, status, chunk_count, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'parsing', 0, ?, ?)",
    )
    .bind(&doc_id)
    .bind(kb_id)
    .bind(&file_name)
    .bind(path.to_string_lossy().to_string())
    .bind(file_size as i64)
    .bind(&file_type)
    .bind(&content_hash)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    // 解析文档
    let parse_result = parsing::parse_document(path).await;

    match parse_result {
        Ok(text) => {
            // 更新状态为 parsed
            sqlx::query("UPDATE documents SET status = 'parsed', updated_at = ? WHERE id = ?")
                .bind(chrono::Utc::now().to_rfc3339())
                .bind(&doc_id)
                .execute(pool)
                .await?;

            tracing::info!(
                "文档解析成功: {} ({} 字符)",
                file_name,
                text.chars().count()
            );

            // 若 Ollama 就绪,执行切分 + 向量化
            if ollama_ready {
                sqlx::query("UPDATE documents SET status = 'vectorizing', updated_at = ? WHERE id = ?")
                    .bind(chrono::Utc::now().to_rfc3339())
                    .bind(&doc_id)
                    .execute(pool)
                    .await?;

                let pipeline = RagPipeline::with_models(
                    pool.clone(),
                    ollama_client,
                    chat_model.to_string(),
                    crate::ollama::DEFAULT_EMBEDDING_MODEL.to_string(),
                );
                match pipeline
                    .index_document(kb_id, &doc_id, &text, split_strategy, split_config)
                    .await
                {
                    Ok(n) => {
                        tracing::info!("文档向量化完成: {} ({} 个片段)", file_name, n);
                    }
                    Err(e) => {
                        tracing::warn!("文档向量化失败 {}: {}", file_name, e);
                        // 标记为 parsed(可后续手动重试)
                        sqlx::query(
                            "UPDATE documents SET status = 'parsed', error_message = ?, updated_at = ? WHERE id = ?",
                        )
                        .bind(format!("向量化失败: {}", e))
                        .bind(chrono::Utc::now().to_rfc3339())
                        .bind(&doc_id)
                        .execute(pool)
                        .await?;
                    }
                }
            } else {
                tracing::info!("Ollama 未就绪,跳过向量化: {}", file_name);
            }

            Ok(())
        }
        Err(e) => {
            // 标记失败
            let err_msg = e.to_string();
            sqlx::query(
                "UPDATE documents SET status = 'failed', error_message = ?, updated_at = ? WHERE id = ?",
            )
            .bind(&err_msg)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&doc_id)
            .execute(pool)
            .await?;
            Err(e)
        }
    }
}

/// 获取指定知识库的文档列表
#[tauri::command]
pub async fn get_documents(
    db: State<'_, DbState>,
    kb_id: String,
) -> Result<Vec<Document>, AppError> {
    let docs: Vec<Document> = sqlx::query_as::<_, Document>(
        "SELECT * FROM documents WHERE knowledge_base_id = ? ORDER BY created_at DESC",
    )
    .bind(kb_id)
    .fetch_all(&db.0)
    .await?;
    Ok(docs)
}

/// 查询导入任务进度
#[tauri::command]
pub async fn get_import_task_progress(
    task_id: String,
) -> Result<Option<ImportProgress>, AppError> {
    let progress = IMPORT_TASKS
        .lock()
        .unwrap()
        .get(&task_id)
        .map(|t| t.progress.clone());
    Ok(progress)
}

/// 取消导入任务(当前文件处理完后停止,已完成的文档保留)
#[tauri::command]
pub async fn cancel_import(task_id: String) -> Result<bool, AppError> {
    let tasks = IMPORT_TASKS.lock().unwrap();
    if let Some(t) = tasks.get(&task_id) {
        t.cancel.cancel();
        Ok(true)
    } else {
        Ok(false)
    }
}

/// 删除文档:连带清理 chunks 和向量(单事务,释放空间并允许同文件重新导入)
#[tauri::command]
pub async fn delete_document(db: State<'_, DbState>, doc_id: String) -> Result<(), AppError> {
    let mut tx = db.0.begin().await?;
    sqlx::query(
        "DELETE FROM vec_chunks WHERE chunk_id IN (SELECT id FROM chunks WHERE document_id = ?)",
    )
    .bind(&doc_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM chunks WHERE document_id = ?")
        .bind(&doc_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM documents WHERE id = ?")
        .bind(&doc_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

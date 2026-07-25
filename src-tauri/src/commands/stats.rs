// 存储统计命令
// 汇总应用数据目录(数据库/知识库文件/日志)与 Ollama 模型目录的磁盘占用

use crate::config::AppPaths;
use crate::db::DbState;
use crate::error::AppError;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

/// 单个知识库的存储统计
#[derive(Serialize)]
pub struct KbStorageStat {
    pub id: String,
    pub name: String,
    pub document_count: i64,
    pub chunk_count: i64,
    pub vector_count: i64,
    /// 原文件大小合计(取自 documents.file_size)
    pub documents_bytes: i64,
    /// KB 目录实际磁盘占用(递归遍历)
    pub dir_bytes: u64,
}

/// 单个 Ollama 模型目录的占用
#[derive(Serialize)]
pub struct ModelDirStat {
    pub path: String,
    pub exists: bool,
    pub bytes: u64,
}

#[derive(Serialize)]
pub struct StorageStats {
    /// zhishiku.db(含 -wal/-shm)
    pub database_bytes: u64,
    /// knowledge_bases/ 目录总占用
    pub knowledge_bases_bytes: u64,
    /// logs/ 目录占用
    pub logs_bytes: u64,
    /// 应用自身占用合计(数据库 + 知识库 + 日志,不含模型)
    pub total_bytes: u64,
    pub model_dirs: Vec<ModelDirStat>,
    pub knowledge_bases: Vec<KbStorageStat>,
}

/// 递归计算目录大小(不跟随符号链接,失败项跳过)
fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            total += dir_size(&entry.path());
        } else if meta.is_file() {
            total += meta.len();
        }
    }
    total
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// 数据库文件大小(含 WAL/SHM 伴生文件)
fn database_size() -> u64 {
    let db = AppPaths::database_path();
    let mut total = file_size(&db);
    for suffix in ["-wal", "-shm"] {
        let mut p = db.as_os_str().to_owned();
        p.push(suffix);
        total += file_size(Path::new(&p));
    }
    total
}

/// Ollama models 候选目录(与 check_models_on_disk 一致)
fn model_candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".ollama").join("models"));
    }
    dirs.push(AppPaths::models_dir());
    dirs
}

#[tauri::command]
pub async fn get_storage_stats(db: State<'_, DbState>) -> Result<StorageStats, AppError> {
    // 每 KB 文档聚合(文档数/分块数/原文件字节)
    let kb_rows = sqlx::query_as::<_, (String, String, i64, i64, i64)>(
        r#"SELECT kb.id, kb.name,
                  COUNT(d.id) AS document_count,
                  COALESCE(SUM(d.chunk_count), 0) AS chunk_count,
                  COALESCE(SUM(d.file_size), 0) AS documents_bytes
           FROM knowledge_bases kb
           LEFT JOIN documents d ON d.knowledge_base_id = kb.id
           GROUP BY kb.id, kb.name
           ORDER BY kb.created_at"#,
    )
    .fetch_all(&db.0)
    .await?;

    // 每 KB 向量数(vec0 虚拟表实查)
    let vec_rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT knowledge_base_id, COUNT(*) FROM vec_chunks GROUP BY knowledge_base_id",
    )
    .fetch_all(&db.0)
    .await?;

    let knowledge_bases = kb_rows
        .into_iter()
        .map(|(id, name, document_count, chunk_count, documents_bytes)| {
            let vector_count = vec_rows
                .iter()
                .find(|(kb_id, _)| kb_id == &id)
                .map(|(_, c)| *c)
                .unwrap_or(0);
            let dir_bytes = dir_size(&AppPaths::knowledge_base_dir(&id));
            KbStorageStat {
                id,
                name,
                document_count,
                chunk_count,
                vector_count,
                documents_bytes,
                dir_bytes,
            }
        })
        .collect();

    let database_bytes = database_size();
    let knowledge_bases_bytes = dir_size(&AppPaths::root_dir().join("knowledge_bases"));
    let logs_bytes = dir_size(&AppPaths::logs_dir());
    let total_bytes = database_bytes + knowledge_bases_bytes + logs_bytes;

    let model_dirs = model_candidate_dirs()
        .into_iter()
        .map(|p| {
            let exists = p.is_dir();
            ModelDirStat {
                path: p.to_string_lossy().to_string(),
                exists,
                bytes: if exists { dir_size(&p) } else { 0 },
            }
        })
        .collect();

    Ok(StorageStats {
        database_bytes,
        knowledge_bases_bytes,
        logs_bytes,
        total_bytes,
        model_dirs,
        knowledge_bases,
    })
}

/// 清空日志目录内容(保留目录本身)
/// 当前活动日志文件被 tracing appender 占用,删除失败时跳过,返回实际释放的字节数
#[tauri::command]
pub async fn clear_logs() -> Result<u64, AppError> {
    let dir = AppPaths::logs_dir();
    let mut freed = 0u64;
    if dir.is_dir() {
        for entry in std::fs::read_dir(&dir)?.flatten() {
            let path = entry.path();
            let size = if path.is_dir() {
                dir_size(&path)
            } else {
                file_size(&path)
            };
            let removed = if path.is_dir() {
                std::fs::remove_dir_all(&path).is_ok()
            } else {
                std::fs::remove_file(&path).is_ok()
            };
            if removed {
                freed += size;
            }
        }
    }
    Ok(freed)
}

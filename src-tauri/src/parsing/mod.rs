// 文档解析模块
// 支持 PDF / DOCX / TXT / MD

pub mod pdf;
pub mod docx;
pub mod text;

use crate::error::{AppError, AppResult};
use std::path::Path;

/// 支持的文件格式
#[derive(Debug, Clone, PartialEq)]
pub enum FileFormat {
    Pdf,
    Docx,
    Txt,
    Md,
}

impl FileFormat {
    /// 通过扩展名 + 魔数探测文件格式
    pub fn detect(path: &Path) -> AppResult<Self> {
        // 先用扩展名初判
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        let by_ext = match ext.as_str() {
            "pdf" => Some(FileFormat::Pdf),
            "docx" => Some(FileFormat::Docx),
            "txt" => Some(FileFormat::Txt),
            "md" | "markdown" => Some(FileFormat::Md),
            _ => None,
        };

        if let Some(fmt) = by_ext {
            // 魔数二次校验（仅对二进制格式）
            if fmt == FileFormat::Pdf || fmt == FileFormat::Docx {
                Self::verify_magic(path, &fmt)?;
            }
            return Ok(fmt);
        }

        // 扩展名未知，尝试魔数
        if Self::check_magic(path, b"%PDF") {
            return Ok(FileFormat::Pdf);
        }

        // 尝试作为文本读取
        if let Ok(content) = std::fs::read(path) {
            if std::str::from_utf8(&content).is_ok() {
                return Ok(FileFormat::Txt);
            }
        }

        Err(AppError::UnsupportedFormat(ext))
    }

    /// 验证魔数
    fn verify_magic(path: &Path, expected: &FileFormat) -> AppResult<()> {
        if !Self::check_magic(path, match expected {
            FileFormat::Pdf => b"%PDF",
            FileFormat::Docx => b"PK", // ZIP 魔数（docx 是 zip）
            _ => return Ok(()),
        }) {
            return Err(AppError::Parsing(format!(
                "文件 {:?} 的实际格式与扩展名不符",
                path.file_name()
            )));
        }
        Ok(())
    }

    /// 读取文件头判断魔数
    fn check_magic(path: &Path, magic: &[u8]) -> bool {
        match std::fs::File::open(path) {
            Ok(mut file) => {
                use std::io::Read;
                let mut buf = vec![0u8; magic.len()];
                if file.read_exact(&mut buf).is_ok() {
                    return buf == magic;
                }
                false
            }
            Err(_) => false,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            FileFormat::Pdf => "pdf",
            FileFormat::Docx => "docx",
            FileFormat::Txt => "txt",
            FileFormat::Md => "md",
        }
    }
}

/// 统一解析入口
pub async fn parse_document(path: &Path) -> AppResult<String> {
    let format = FileFormat::detect(path)?;
    let path = path.to_path_buf();

    // 文档解析是 CPU 密集型，放到阻塞线程池
    tokio::task::spawn_blocking(move || match format {
        FileFormat::Pdf => pdf::extract_text(&path),
        FileFormat::Docx => docx::extract_text(&path),
        FileFormat::Txt | FileFormat::Md => text::extract_text(&path),
    })
    .await
    .map_err(|e| AppError::Parsing(format!("解析任务失败: {}", e)))?
}

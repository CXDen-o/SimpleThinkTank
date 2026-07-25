// PDF 文档解析
use crate::error::{AppError, AppResult};
use std::path::Path;

pub fn extract_text(path: &Path) -> AppResult<String> {
    let text = pdf_extract::extract_text(path)
        .map_err(|e| AppError::Parsing(format!("PDF 解析失败: {}", e)))?;
    Ok(text)
}

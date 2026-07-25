// TXT / MD 文档解析
use crate::error::{AppError, AppResult};
use std::path::Path;

pub fn extract_text(path: &Path) -> AppResult<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| AppError::Parsing(format!("读取文件失败: {}", e)))?;

    // 1. 尝试 UTF-8(含 BOM 自动处理)
    if let Ok(text) = String::from_utf8(bytes.clone()) {
        return Ok(text);
    }

    // 2. 尝试 GBK / GB2312 / GB18030(Windows 中文环境常见)
    let gb18030 = encoding_rs::GB18030;
    let (text, _, had_errors) = gb18030.decode(&bytes);
    if had_errors {
        return Err(AppError::Parsing(
            "文件编码无法识别(非 UTF-8/GBK)".to_string(),
        ));
    }

    Ok(text.into_owned())
}

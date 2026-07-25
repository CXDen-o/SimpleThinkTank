// DOCX 文档解析
use crate::error::{AppError, AppResult};
use docx_rs::{
    DocumentChild, ParagraphChild, RunChild,
};
use std::path::Path;

pub fn extract_text(path: &Path) -> AppResult<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| AppError::Parsing(format!("读取 DOCX 文件失败: {}", e)))?;

    let docx = docx_rs::read_docx(&bytes)
        .map_err(|e| AppError::Parsing(format!("DOCX 解析失败: {}", e)))?;

    let mut text = String::new();
    for child in &docx.document.children {
        if let DocumentChild::Paragraph(para) = child {
            collect_paragraph_text(para, &mut text);
            text.push('\n');
        }
    }

    Ok(text)
}

fn collect_paragraph_text(para: &docx_rs::Paragraph, out: &mut String) {
    for child in &para.children {
        match child {
            ParagraphChild::Run(run) => {
                for rc in &run.children {
                    if let RunChild::Text(t) = rc {
                        out.push_str(&t.text);
                    }
                }
            }
            ParagraphChild::Hyperlink(h) => {
                for hc in &h.children {
                    if let ParagraphChild::Run(run) = hc {
                        for rc in &run.children {
                            if let RunChild::Text(t) = rc {
                                out.push_str(&t.text);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

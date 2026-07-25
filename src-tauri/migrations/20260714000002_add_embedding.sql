-- 为 vector_indices 表添加 embedding 字段(以 JSON 文本形式存储 Vec<f32>)
-- 默认向量存储实现为 SQLite 暴力余弦检索,后续可替换为 ChromaDB/FAISS
ALTER TABLE vector_indices ADD COLUMN embedding TEXT;
ALTER TABLE vector_indices ADD COLUMN document_id TEXT;

CREATE INDEX IF NOT EXISTS idx_vector_indices_doc ON vector_indices(document_id);

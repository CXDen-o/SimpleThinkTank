-- 清理 vector_indices 表中冗余的 document_id 字段
-- 该字段自添加以来从未被写入或读取（检索时通过 JOIN chunks.document_id 获取）
-- 同时移除关联的索引 idx_vector_indices_doc
DROP INDEX IF EXISTS idx_vector_indices_doc;
ALTER TABLE vector_indices DROP COLUMN document_id;

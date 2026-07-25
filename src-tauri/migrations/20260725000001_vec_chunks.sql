-- 向量存储迁移到 sqlite-vec(vec0 虚拟表,KNN 下推到 C 扩展)
-- 原 vector_indices 表为 JSON 文本存储 + 应用层暴力余弦,O(N) 全表扫描
-- vec0 直接接受 JSON 文本向量输入,自动压缩为 float blob,可纯 SQL 搬迁
-- 注意:embedding 维度 768 与嵌入模型 nomic-embed-text 绑定,换模型需重建此表

CREATE VIRTUAL TABLE vec_chunks USING vec0(
    chunk_id TEXT PRIMARY KEY,
    knowledge_base_id TEXT,
    embedding FLOAT[768] distance_metric=cosine
);

INSERT INTO vec_chunks(chunk_id, knowledge_base_id, embedding)
SELECT chunk_id, knowledge_base_id, embedding
FROM vector_indices
WHERE embedding IS NOT NULL;

DROP TABLE vector_indices;

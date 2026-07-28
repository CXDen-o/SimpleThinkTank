// 基于 sqlite-vec(vec0 虚拟表)的向量存储实现
// KNN 下推到 C 扩展(SIMD),向量以紧凑 float blob 存储
// 保留单文件/事务一致/零额外进程的桌面分发优势

use crate::db::Db;
use crate::error::AppResult;
use crate::vectorstore::traits::{RetrievedChunk, VectorRecord, VectorStore};
use async_trait::async_trait;

/// sqlite-vec 向量存储
#[derive(Clone)]
pub struct SqliteVecStore {
    db: Db,
}

impl SqliteVecStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// 在给定事务内写入向量(先删后插实现覆盖),由调用方控制事务边界
    pub async fn add_vectors_in(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        records: &[VectorRecord],
    ) -> AppResult<()> {
        for r in records {
            // vec0 接受 JSON 文本向量,自动压缩为 float blob;
            // vec0 虚拟表不支持 INSERT OR REPLACE 冲突策略,先删后插实现覆盖
            let emb_json = serde_json::to_string(&r.embedding)?;
            sqlx::query("DELETE FROM vec_chunks WHERE chunk_id = ?")
                .bind(&r.chunk_id)
                .execute(&mut **tx)
                .await?;
            sqlx::query(
                "INSERT INTO vec_chunks(chunk_id, knowledge_base_id, embedding) VALUES (?, ?, ?)",
            )
            .bind(&r.chunk_id)
            .bind(&r.knowledge_base_id)
            .bind(&emb_json)
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct VecSearchRow {
    chunk_id: String,
    distance: f64,
    document_id: String,
    content: String,
    metadata: String,
}

#[async_trait]
impl VectorStore for SqliteVecStore {
    async fn add_vectors(&self, records: Vec<VectorRecord>) -> AppResult<()> {
        let mut tx = self.db.begin().await?;
        self.add_vectors_in(&mut tx, &records).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_by_chunk(&self, chunk_id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM vec_chunks WHERE chunk_id = ?")
            .bind(chunk_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn delete_by_document(&self, document_id: &str) -> AppResult<()> {
        sqlx::query(
            "DELETE FROM vec_chunks WHERE chunk_id IN (SELECT id FROM chunks WHERE document_id = ?)",
        )
        .bind(document_id)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    async fn delete_by_knowledge_base(&self, kb_id: &str) -> AppResult<()> {
        sqlx::query(
            "DELETE FROM vec_chunks WHERE chunk_id IN (SELECT id FROM chunks WHERE knowledge_base_id = ?)",
        )
        .bind(kb_id)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    async fn search(
        &self,
        kb_id: &str,
        query_vector: &[f32],
        top_k: usize,
    ) -> AppResult<Vec<RetrievedChunk>> {
        let query_json = serde_json::to_string(query_vector)?;
        let rows: Vec<VecSearchRow> = sqlx::query_as(
            "SELECT v.chunk_id, v.distance, c.document_id, c.content, c.metadata
             FROM vec_chunks v
             JOIN chunks c ON c.id = v.chunk_id
             WHERE v.embedding MATCH ? AND k = ? AND v.knowledge_base_id = ?
             ORDER BY v.distance",
        )
        .bind(&query_json)
        .bind(top_k as i64)
        .bind(kb_id)
        .fetch_all(&self.db)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let metadata: serde_json::Value =
                serde_json::from_str(&row.metadata).unwrap_or(serde_json::json!({}));
            out.push(RetrievedChunk {
                chunk_id: row.chunk_id,
                document_id: row.document_id,
                content: row.content,
                // vec0 余弦距离 d = 1 − 余弦相似度,转换回"越大越好"的 score 语义
                score: (1.0 - row.distance) as f32,
                metadata,
            });
        }
        Ok(out)
    }

    async fn count(&self, kb_id: &str) -> AppResult<usize> {
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM vec_chunks WHERE knowledge_base_id = ?")
                .bind(kb_id)
                .fetch_one(&self.db)
                .await?;
        Ok(row.0 as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectorstore::traits::VectorStore;

    const DIM: usize = 768;

    /// 768 维向量:除指定位置外全零
    fn one_hot(idx: usize, val: f32) -> Vec<f32> {
        let mut v = vec![0.0f32; DIM];
        v[idx] = val;
        v
    }

    /// 全量迁移的临时库(含 chunks/vec_chunks 表)
    async fn setup() -> (Db, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("vec_store_{}.db", uuid::Uuid::new_v4()));
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = crate::db::init_database(&url).await.expect("初始化数据库失败");
        // 准备最小业务数据:知识库 + 文档 + chunks
        sqlx::query(
            "INSERT INTO knowledge_bases(id, name, storage_path) \
             VALUES ('kb1', '库1', '/kb1'), ('kb2', '库2', '/kb2')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO documents(id, knowledge_base_id, file_name, file_path, file_size, file_type) \
             VALUES ('doc1', 'kb1', 'a.md', '/a.md', 100, 'md'), ('doc2', 'kb2', 'b.md', '/b.md', 100, 'md')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chunks(id, document_id, knowledge_base_id, content, chunk_index) VALUES \
             ('c1', 'doc1', 'kb1', '苹果', 0), \
             ('c2', 'doc1', 'kb1', '香蕉', 1), \
             ('c3', 'doc2', 'kb2', '苹果', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        (pool, path)
    }

    async fn teardown(pool: Db, path: std::path::PathBuf) {
        drop(pool);
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn test_add_search_roundtrip_and_score() {
        let (pool, path) = setup().await;
        let store = SqliteVecStore::new(pool.clone());

        store
            .add_vectors(vec![
                VectorRecord { chunk_id: "c1".into(), knowledge_base_id: "kb1".into(), embedding: one_hot(0, 1.0) },
                VectorRecord { chunk_id: "c2".into(), knowledge_base_id: "kb1".into(), embedding: one_hot(1, 1.0) },
            ])
            .await
            .unwrap();

        // 查询与 c1 同向的向量
        let hits = store.search("kb1", &one_hot(0, 1.0), 5).await.unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].chunk_id, "c1");
        assert_eq!(hits[0].content, "苹果");
        // 同向余弦相似度 = 1
        assert!((hits[0].score - 1.0).abs() < 1e-4, "score={}", hits[0].score);
        // 正交余弦相似度 = 0
        assert!((hits[1].score - 0.0).abs() < 1e-4, "score={}", hits[1].score);
        // 降序
        assert!(hits[0].score > hits[1].score);

        teardown(pool, path).await;
    }

    #[tokio::test]
    async fn test_topk_and_kb_isolation() {
        let (pool, path) = setup().await;
        let store = SqliteVecStore::new(pool.clone());

        // 余弦只看方向不看长度,c2 用与 c1 近似但不同方向的向量
        let mut e2 = vec![0.0f32; DIM];
        e2[0] = 0.9;
        e2[1] = 0.1;

        store
            .add_vectors(vec![
                VectorRecord { chunk_id: "c1".into(), knowledge_base_id: "kb1".into(), embedding: one_hot(0, 1.0) },
                VectorRecord { chunk_id: "c2".into(), knowledge_base_id: "kb1".into(), embedding: e2 },
                VectorRecord { chunk_id: "c3".into(), knowledge_base_id: "kb2".into(), embedding: one_hot(0, 1.0) },
            ])
            .await
            .unwrap();

        // top_k=1 只回 1 条
        let hits = store.search("kb1", &one_hot(0, 1.0), 1).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk_id, "c1");

        // kb 隔离:kb2 只命中 c3,不命中 kb1 的同向量 c1
        let hits = store.search("kb2", &one_hot(0, 1.0), 5).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk_id, "c3");

        // count 按 kb 统计
        assert_eq!(store.count("kb1").await.unwrap(), 2);
        assert_eq!(store.count("kb2").await.unwrap(), 1);

        teardown(pool, path).await;
    }

    #[tokio::test]
    async fn test_upsert_same_chunk() {
        let (pool, path) = setup().await;
        let store = SqliteVecStore::new(pool.clone());

        let rec = |emb: Vec<f32>| VectorRecord {
            chunk_id: "c1".into(),
            knowledge_base_id: "kb1".into(),
            embedding: emb,
        };
        store.add_vectors(vec![rec(one_hot(0, 1.0))]).await.unwrap();
        // 同 chunk_id 再写:覆盖而非报错/重复
        store.add_vectors(vec![rec(one_hot(1, 1.0))]).await.unwrap();

        assert_eq!(store.count("kb1").await.unwrap(), 1);
        let hits = store.search("kb1", &one_hot(1, 1.0), 5).await.unwrap();
        assert!((hits[0].score - 1.0).abs() < 1e-4);

        teardown(pool, path).await;
    }

    #[tokio::test]
    async fn test_delete_cascades() {
        let (pool, path) = setup().await;
        let store = SqliteVecStore::new(pool.clone());

        store
            .add_vectors(vec![
                VectorRecord { chunk_id: "c1".into(), knowledge_base_id: "kb1".into(), embedding: one_hot(0, 1.0) },
                VectorRecord { chunk_id: "c2".into(), knowledge_base_id: "kb1".into(), embedding: one_hot(1, 1.0) },
                VectorRecord { chunk_id: "c3".into(), knowledge_base_id: "kb2".into(), embedding: one_hot(2, 1.0) },
            ])
            .await
            .unwrap();

        // 按 chunk 删
        store.delete_by_chunk("c1").await.unwrap();
        assert_eq!(store.count("kb1").await.unwrap(), 1);

        // 按文档删(doc1 的 c2)
        store.delete_by_document("doc1").await.unwrap();
        assert_eq!(store.count("kb1").await.unwrap(), 0);

        // 按知识库删(kb2 的 c3)
        store.delete_by_knowledge_base("kb2").await.unwrap();
        assert_eq!(store.count("kb2").await.unwrap(), 0);

        teardown(pool, path).await;
    }
}

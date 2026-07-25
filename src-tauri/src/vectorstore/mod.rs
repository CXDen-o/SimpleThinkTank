// 向量存储模块
// 抽象 VectorStore trait,实现为基于 sqlite-vec(vec0 虚拟表)的 KNN 向量存储

pub mod sqlite_vec_store;
pub mod traits;

pub use sqlite_vec_store::SqliteVecStore;
pub use traits::{RetrievedChunk, VectorRecord, VectorStore};

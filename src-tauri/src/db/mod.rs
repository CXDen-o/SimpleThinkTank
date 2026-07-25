// 数据库模块
// SQLite 持久化，使用 sqlx

use sqlx::{sqlite::SqlitePool, Pool, Sqlite};
use std::str::FromStr;

pub type Db = Pool<Sqlite>;

/// 数据库状态，用于注入 Tauri
#[derive(Clone)]
pub struct DbState(pub Db);

/// 进程级注册 sqlite-vec 扩展(幂等)
///
/// sqlite3_auto_extension 是进程级机制:注册后,所有新建立的连接
/// (含 sqlx pool 各连接、迁移连接)都会自动加载 vec0 模块。
/// 必须在任何数据库连接建立之前调用。
pub fn register_sqlite_vec() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| unsafe {
        libsqlite3_sys::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut libsqlite3_sys::sqlite3,
                *mut *mut i8,
                *const libsqlite3_sys::sqlite3_api_routines,
            ) -> i32,
        >(sqlite_vec::sqlite3_vec_init as *const ())));
    });
}

/// 初始化数据库：建立连接 + 运行迁移
pub async fn init_database(db_url: &str) -> Result<Db, sqlx::Error> {
    // 扩展注册必须先于任何连接建立
    register_sqlite_vec();

    // 确保父目录存在
    if let Some(path_str) = db_url.strip_prefix("sqlite://") {
        let path_str = path_str.split('?').next().unwrap_or(path_str);
        if let Some(parent) = std::path::Path::new(path_str).parent() {
            std::fs::create_dir_all(parent).ok();
        }
    }

    let options = sqlx::sqlite::SqliteConnectOptions::from_str(db_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePool::connect_with(options).await?;

    // 运行内嵌迁移
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    /// Spike: 验证 sqlite-vec 静态链接 + auto_extension 在 sqlx 下工作
    /// (vec0 建表、JSON 文本向量插入、KNN 查询)
    #[tokio::test]
    async fn test_sqlite_vec_spike() {
        // 内存库在 pool 多连接间不共享,用临时文件库
        let path = std::env::temp_dir().join(format!("vec_spike_{}.db", uuid::Uuid::new_v4()));
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = super::init_database(&url).await.expect("初始化数据库失败");

        // vec0 虚拟表可建
        sqlx::query(
            "CREATE VIRTUAL TABLE vec_spike USING vec0(embedding FLOAT[4] distance_metric=cosine)",
        )
        .execute(&pool)
        .await
        .expect("创建 vec0 虚拟表失败(sqlite-vec 扩展未加载?)");

        // JSON 文本向量直接插入(vec0 自动压缩存储)
        sqlx::query(
            "INSERT INTO vec_spike(rowid, embedding) VALUES \
             (1, '[1.0, 0.0, 0.0, 0.0]'), (2, '[0.0, 1.0, 0.0, 0.0]')",
        )
        .execute(&pool)
        .await
        .expect("插入向量失败");

        // KNN 查询:最近的应是 rowid=1 自身
        let rows: Vec<(i64, f64)> = sqlx::query_as(
            "SELECT rowid, distance FROM vec_spike \
             WHERE embedding MATCH '[1.0, 0.0, 0.0, 0.0]' AND k = 2 ORDER BY distance",
        )
        .fetch_all(&pool)
        .await
        .expect("KNN 查询失败");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, 1);
        assert!(rows[0].1 < rows[1].1, "距离应升序: {:?}", rows);

        drop(pool);
        std::fs::remove_file(&path).ok();
    }
}

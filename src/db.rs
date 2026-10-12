//! 数据库连接池。整个文件仅在 `server` feature 下编译。
//!
//! 连接池在 `main` 里通过 [`init_pool`] 异步创建一次，然后放入全局
//! `OnceLock`。之所以不在 `OnceLock::get_or_init` 里建池，是因为该 API
//! 只接受同步闭包，而 sqlx 建池是异步的。

use std::str::FromStr;
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;

static POOL: OnceLock<SqlitePool> = OnceLock::new();

/// sqlx 的 `create_if_missing` 只建文件、不建父目录，
/// 所以 `sqlite://data/mtab.db` 在 `data/` 不存在时会直接失败。
fn ensure_parent_dir(database_url: &str) -> Result<()> {
    let path = database_url
        .split('?')
        .next()
        .unwrap_or(database_url)
        .strip_prefix("sqlite://")
        .or_else(|| {
            database_url
                .split('?')
                .next()
                .unwrap_or(database_url)
                .strip_prefix("sqlite:")
        })
        .unwrap_or_else(|| database_url.split('?').next().unwrap_or(database_url));

    if path.is_empty() || path == ":memory:" {
        return Ok(());
    }

    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建数据库目录失败: {}", parent.display()))?;
        }
    }

    Ok(())
}

/// 建池 + 跑迁移 + 注册到全局。只应在服务端启动时调用一次。
pub async fn init_pool(database_url: &str) -> Result<()> {
    ensure_parent_dir(database_url)?;

    let options = SqliteConnectOptions::from_str(database_url)
        .with_context(|| format!("DATABASE_URL 无效: {database_url}"))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .context("连接 SQLite 失败")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("执行数据库迁移失败")?;

    POOL.set(pool)
        .map_err(|_| anyhow!("数据库连接池已初始化，重复调用 init_pool"))?;

    Ok(())
}

/// 取全局连接池。未初始化即 panic，属于启动顺序错误。
pub fn pool() -> &'static SqlitePool {
    POOL.get()
        .expect("数据库连接池尚未初始化，请先调用 db::init_pool")
}

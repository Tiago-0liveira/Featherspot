use std::{
    path::Path,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use mellowdeck_core::{AppError, CacheRepository, Cached, ErrorKind, Result};
use rusqlite::{Connection, OptionalExtension as _, params};

const MIGRATION_1: &str = include_str!("../../../migrations/0001_initial.sql");

#[derive(Debug)]
pub struct SqliteCache {
    connection: Mutex<Connection>,
}

impl SqliteCache {
    /// Opens a cache database and applies all forward migrations transactionally.
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` cannot open, configure, or migrate the database.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut connection = Connection::open(path).map_err(storage_error)?;
        connection.pragma_update(None, "journal_mode", "WAL").map_err(storage_error)?;
        connection.pragma_update(None, "foreign_keys", "ON").map_err(storage_error)?;
        apply_migrations(&mut connection)?;
        Ok(Self { connection: Mutex::new(connection) })
    }

    /// Creates a migrated in-memory cache, primarily for tests and ephemeral sessions.
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` cannot initialize or migrate the database.
    pub fn open_in_memory() -> Result<Self> {
        let mut connection = Connection::open_in_memory().map_err(storage_error)?;
        apply_migrations(&mut connection)?;
        Ok(Self { connection: Mutex::new(connection) })
    }

    fn read_sync(&self, key: &str) -> Result<Option<Cached<Vec<u8>>>> {
        let now = millis(SystemTime::now())?;
        let connection = self.connection.lock().map_err(|_| lock_error())?;
        let row = connection
            .query_row(
                "SELECT payload, fetched_at_ms, expires_at_ms, validator FROM cache_entries WHERE cache_key = ?1",
                [key],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?;
        if row.is_some() {
            connection
                .execute(
                    "UPDATE cache_entries SET accessed_at_ms = ?1 WHERE cache_key = ?2",
                    params![now, key],
                )
                .map_err(storage_error)?;
        }
        row.map(|(value, fetched, expires, validator)| {
            Ok(Cached {
                value,
                fetched_at: from_millis(fetched)?,
                expires_at: from_millis(expires)?,
                validator,
            })
        })
        .transpose()
    }

    fn write_sync(&self, key: &str, value: &Cached<Vec<u8>>) -> Result<()> {
        let fetched = millis(value.fetched_at)?;
        let expires = millis(value.expires_at)?;
        let accessed = millis(SystemTime::now())?;
        let size = i64::try_from(value.value.len())
            .map_err(|_| AppError::new(ErrorKind::Storage, "cache entry is too large"))?;
        self.connection
            .lock()
            .map_err(|_| lock_error())?
            .execute(
                "INSERT INTO cache_entries
                 (cache_key, payload, fetched_at_ms, expires_at_ms, validator, accessed_at_ms, size_bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(cache_key) DO UPDATE SET
                 payload=excluded.payload, fetched_at_ms=excluded.fetched_at_ms,
                 expires_at_ms=excluded.expires_at_ms, validator=excluded.validator,
                 accessed_at_ms=excluded.accessed_at_ms, size_bytes=excluded.size_bytes",
                params![key, value.value, fetched, expires, value.validator, accessed, size],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    fn invalidate_sync(&self, prefix: &str) -> Result<usize> {
        let escaped = prefix.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        self.connection
            .lock()
            .map_err(|_| lock_error())?
            .execute(
                "DELETE FROM cache_entries WHERE cache_key LIKE ?1 ESCAPE '\\'",
                [format!("{escaped}%")],
            )
            .map_err(storage_error)
    }

    fn size_sync(&self) -> Result<u64> {
        let bytes: i64 = self
            .connection
            .lock()
            .map_err(|_| lock_error())?
            .query_row("SELECT COALESCE(SUM(size_bytes), 0) FROM cache_entries", [], |row| {
                row.get(0)
            })
            .map_err(storage_error)?;
        u64::try_from(bytes).map_err(|_| AppError::new(ErrorKind::Storage, "cache size is invalid"))
    }

    fn trim_sync(&self, maximum_bytes: u64) -> Result<u64> {
        let mut connection = self.connection.lock().map_err(|_| lock_error())?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let mut size: i64 = transaction
            .query_row("SELECT COALESCE(SUM(size_bytes), 0) FROM cache_entries", [], |row| {
                row.get(0)
            })
            .map_err(storage_error)?;
        let maximum = i64::try_from(maximum_bytes).unwrap_or(i64::MAX);
        while size > maximum {
            let removed: Option<(String, i64)> = transaction
                .query_row(
                    "SELECT cache_key, size_bytes FROM cache_entries ORDER BY accessed_at_ms ASC, cache_key ASC LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(storage_error)?;
            let Some((key, bytes)) = removed else { break };
            transaction
                .execute("DELETE FROM cache_entries WHERE cache_key = ?1", [key])
                .map_err(storage_error)?;
            if bytes <= 0 {
                size = transaction
                    .query_row("SELECT COALESCE(SUM(size_bytes), 0) FROM cache_entries", [], |row| {
                        row.get(0)
                    })
                    .map_err(storage_error)?;
                if size <= maximum {
                    break;
                }
                // Also check if any remaining entries have size_bytes > 0; if not, break to prevent infinite loop
                let positive_count: i64 = transaction
                    .query_row("SELECT COUNT(*) FROM cache_entries WHERE size_bytes > 0", [], |row| row.get(0))
                    .map_err(storage_error)?;
                if positive_count == 0 {
                    break;
                }
            } else {
                size = size.saturating_sub(bytes);
            }
        }
        transaction.commit().map_err(storage_error)?;
        u64::try_from(size.max(0)).map_err(|_| AppError::new(ErrorKind::Storage, "cache size is invalid"))
    }
}

impl CacheRepository for SqliteCache {
    fn read(&self, key: String) -> mellowdeck_core::BoxFuture<'_, Result<Option<Cached<Vec<u8>>>>> {
        Box::pin(async move { self.read_sync(&key) })
    }

    fn write(
        &self,
        key: String,
        value: Cached<Vec<u8>>,
    ) -> mellowdeck_core::BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.write_sync(&key, &value) })
    }

    fn invalidate(&self, prefix: String) -> mellowdeck_core::BoxFuture<'_, Result<usize>> {
        Box::pin(async move { self.invalidate_sync(&prefix) })
    }

    fn size_bytes(&self) -> mellowdeck_core::BoxFuture<'_, Result<u64>> {
        Box::pin(async move { self.size_sync() })
    }

    fn trim_to(&self, maximum_bytes: u64) -> mellowdeck_core::BoxFuture<'_, Result<u64>> {
        Box::pin(async move { self.trim_sync(maximum_bytes) })
    }

    fn clear(&self) -> mellowdeck_core::BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.connection
                .lock()
                .map_err(|_| lock_error())?
                .execute("DELETE FROM cache_entries", [])
                .map_err(storage_error)?;
            Ok(())
        })
    }
}

fn apply_migrations(connection: &mut Connection) -> Result<()> {
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage_error)?;
    if version == 0 {
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction.execute_batch(MIGRATION_1).map_err(storage_error)?;
        transaction.pragma_update(None, "user_version", 1).map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
    }
    if version > 1 {
        return Err(AppError::new(
            ErrorKind::Storage,
            "cache database was created by a newer Mellowdeck version",
        ));
    }
    Ok(())
}

fn millis(time: SystemTime) -> Result<i64> {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AppError::new(ErrorKind::Storage, "timestamp predates Unix epoch"))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| AppError::new(ErrorKind::Storage, "timestamp is too large"))
}

fn from_millis(value: i64) -> Result<SystemTime> {
    let value = u64::try_from(value)
        .map_err(|_| AppError::new(ErrorKind::Storage, "stored timestamp is invalid"))?;
    UNIX_EPOCH
        .checked_add(Duration::from_millis(value))
        .ok_or_else(|| AppError::new(ErrorKind::Storage, "stored timestamp exceeds valid range"))
}

fn storage_error(error: rusqlite::Error) -> AppError {
    let message = error.to_string();
    drop(error);
    AppError::new(ErrorKind::Storage, message)
}

fn lock_error() -> AppError {
    AppError::new(ErrorKind::Storage, "cache lock was poisoned")
}

#[cfg(test)]
mod tests {
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    use super::*;

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = pin!(future);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("test future unexpectedly pending"),
        }
    }

    fn entry(bytes: usize) -> Cached<Vec<u8>> {
        let now = SystemTime::now();
        Cached {
            value: vec![7; bytes],
            fetched_at: now,
            expires_at: now + Duration::from_secs(60),
            validator: Some("etag".into()),
        }
    }

    #[test]
    fn writes_reads_and_invalidates() {
        let cache = SqliteCache::open_in_memory().unwrap();
        block_on(cache.write("album:one".into(), entry(4))).unwrap();
        assert_eq!(block_on(cache.read("album:one".into())).unwrap().unwrap().value, vec![7; 4]);
        assert_eq!(block_on(cache.invalidate("album:".into())).unwrap(), 1);
        assert!(block_on(cache.read("album:one".into())).unwrap().is_none());
    }

    #[test]
    fn lru_trim_reaches_requested_limit() {
        let cache = SqliteCache::open_in_memory().unwrap();
        block_on(cache.write("first".into(), entry(8))).unwrap();
        block_on(cache.write("second".into(), entry(8))).unwrap();
        assert!(block_on(cache.trim_to(8)).unwrap() <= 8);
    }

    #[test]
    fn from_millis_handles_boundaries_and_invalid_values() {
        assert!(from_millis(i64::MAX).is_err());
        assert!(from_millis(-1).is_err());
        assert_eq!(from_millis(0).unwrap(), UNIX_EPOCH);
        let sample = 1_700_000_000_000_i64;
        assert_eq!(
            from_millis(sample).unwrap(),
            UNIX_EPOCH + Duration::from_millis(sample as u64)
        );
    }

    #[test]
    fn trim_terminates_promptly_and_cleans_zero_byte_entries() {
        let cache = SqliteCache::open_in_memory().unwrap();
        block_on(cache.write("zero_1".into(), entry(0))).unwrap();
        block_on(cache.write("zero_2".into(), entry(0))).unwrap();
        block_on(cache.write("positive_1".into(), entry(10))).unwrap();
        block_on(cache.write("positive_2".into(), entry(10))).unwrap();

        {
            let conn = cache.connection.lock().unwrap();
            // Ensure zero-byte entries have earlier accessed timestamps so they are evicted first
            conn.execute(
                "UPDATE cache_entries SET accessed_at_ms = 1 WHERE cache_key = 'zero_1'",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE cache_entries SET accessed_at_ms = 2 WHERE cache_key = 'zero_2'",
                [],
            )
            .unwrap();
        }

        // Eviction will first hit 'zero_1' (bytes <= 0) and 'zero_2' (bytes <= 0).
        // It must cleanly delete them, recompute size, and terminate without looping infinitely.
        let remaining_size = cache.trim_sync(10).unwrap();
        assert!(remaining_size <= 10);

        let conn = cache.connection.lock().unwrap();
        let remaining_keys: Vec<String> = conn
            .prepare("SELECT cache_key FROM cache_entries ORDER BY cache_key ASC")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(!remaining_keys.contains(&"zero_1".to_string()));
        assert!(!remaining_keys.contains(&"zero_2".to_string()));
    }
}

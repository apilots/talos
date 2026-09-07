use talos_error::TalosError;

/// Database adapter projection shared by connection and Store operations.
/// Context must describe the operation, never contain SQL, paths, or row data.
pub fn database_error(context: &str, error: sqlx::Error) -> TalosError {
    let reason = match &error {
        sqlx::Error::Database(database) => match database.code().as_deref() {
            Some("5" | "6" | "261" | "262" | "517" | "773") => "database is locked",
            Some("8" | "1032") => "database is read-only",
            Some("13") => "database storage is full",
            Some("11" | "26") => "database data is corrupt or invalid",
            _ => "database operation failed; technical details were withheld",
        },
        sqlx::Error::PoolTimedOut => "timed out waiting for a database connection",
        sqlx::Error::PoolClosed => "database connection pool is closed",
        sqlx::Error::RowNotFound => "the expected database record was missing",
        _ => "database operation failed; technical details were withheld",
    };
    TalosError::internal_with_source(format!("Failed to {context}: {reason}."), error)
}

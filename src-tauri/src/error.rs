/// Crate-level error type for consistent error handling across modules.
///
/// All variants convert to `String` via `From<AppError>` so Tauri commands
/// can keep returning `Result<T, String>`.
#[derive(Debug)]
pub enum AppError {
    Db(rusqlite::Error),
    LockPoisoned(String),
    Other(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Db(e) => write!(f, "Database error: {}", e),
            AppError::LockPoisoned(msg) => write!(f, "Lock poisoned: {}", msg),
            AppError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Db(e)
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Other(s)
    }
}

impl From<AppError> for String {
    fn from(e: AppError) -> String {
        e.to_string()
    }
}

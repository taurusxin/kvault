use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn locked() -> Self {
        Self::new("locked", "保险库已锁定，请重新解锁")
    }
    pub fn storage(_: impl std::fmt::Display) -> Self {
        Self::new("storage", "无法读写保险库文件，请检查磁盘空间与访问权限")
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("validation", message)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for AppError {}
pub type Result<T> = std::result::Result<T, AppError>;

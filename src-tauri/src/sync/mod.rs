pub mod merge;
pub mod remote;
pub mod service;
#[cfg(test)]
pub mod test_support;
#[cfg(test)]
mod tests;
use crate::error::{AppError, Result};
pub use merge::{Conflict, Entity};
use serde::{Deserialize, Serialize};

pub const REMOTE_NAME: &str = "vault.kvault";

#[derive(
    Clone, PartialEq, Eq, Serialize, Deserialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop,
)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SyncConfig {
    S3 {
        endpoint: String,
        region: String,
        bucket: String,
        access_key: String,
        secret_key: String,
        session_token: Option<String>,
        prefix: String,
        path_style: bool,
    },
    Webdav {
        endpoint: String,
        directory: String,
        username: String,
        password: String,
    },
}
impl SyncConfig {
    pub fn validate(&self) -> Result<()> {
        let endpoint = match self {
            Self::S3 {
                endpoint,
                region,
                bucket,
                access_key,
                secret_key,
                ..
            } => {
                if region.trim().is_empty()
                    || bucket.trim().is_empty()
                    || access_key.trim().is_empty()
                    || secret_key.is_empty()
                {
                    return Err(AppError::invalid("请填写完整的 S3 连接信息"));
                }
                endpoint
            }
            Self::Webdav {
                endpoint,
                directory,
                ..
            } => {
                if directory.split('/').any(|p| p == ".." || p == ".") {
                    return Err(AppError::invalid("WebDAV 目录不能包含相对路径"));
                }
                endpoint
            }
        };
        let url =
            reqwest::Url::parse(endpoint).map_err(|_| AppError::invalid("请输入有效的服务地址"))?;
        if !matches!(url.scheme(), "https" | "http")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(AppError::invalid(
                "服务地址必须是 HTTP(S) URL，凭据请填写在独立字段中",
            ));
        }
        Ok(())
    }
    pub fn redacted(&self) -> Self {
        let mut result = self.clone();
        match &mut result {
            Self::S3 {
                secret_key,
                session_token,
                ..
            } => {
                zeroize::Zeroize::zeroize(secret_key);
                zeroize::Zeroize::zeroize(session_token);
                *session_token = None;
            }
            Self::Webdav { password, .. } => zeroize::Zeroize::zeroize(password),
        }
        result
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub state: String,
    pub message: String,
    pub last_synced_at: Option<u64>,
}

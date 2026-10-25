use super::SyncConfig;
use crate::{
    error::{AppError, Result},
    vault::{crypto::MAX_FILE_SIZE, model::id},
};
use aws_sdk_s3::{
    Client,
    config::{BehaviorVersion, Credentials, Region},
    primitives::ByteStream,
};
use reqwest::{Client as HttpClient, StatusCode};
use tokio::io::AsyncReadExt;

pub struct RemoteFile {
    pub bytes: Vec<u8>,
    pub etag: String,
}
pub enum RemoteStore {
    S3 {
        client: Client,
        bucket: String,
        prefix: String,
    },
    Webdav {
        client: HttpClient,
        base: reqwest::Url,
        directories: Vec<reqwest::Url>,
        username: String,
        password: String,
    },
}

fn network() -> AppError {
    AppError::new("network", "无法连接同步服务，请检查网络、地址或 TLS 证书")
}
fn unsupported() -> AppError {
    AppError::new(
        "unsupported",
        "服务未提供可靠的 ETag 或条件写入，已停止写同步",
    )
}
fn http_error(status: u16) -> AppError {
    match status {
        401 | 403 => AppError::new("credentials", "同步认证失败，请检查凭据与读写权限"),
        409 | 412 => AppError::new("precondition", "云端版本已变化，将重新拉取合并"),
        423 => AppError::new("remote_locked", "远端文件被锁定，请稍后重试"),
        _ => AppError::new("remote", format!("同步服务返回错误（HTTP {status}）")),
    }
}

impl RemoteStore {
    pub fn new(config: &SyncConfig) -> Result<Self> {
        config.validate()?;
        match config {
            SyncConfig::S3 {
                endpoint,
                region,
                bucket,
                access_key,
                secret_key,
                session_token,
                prefix,
                path_style,
            } => {
                let config = aws_sdk_s3::Config::builder()
                    .behavior_version(BehaviorVersion::latest())
                    .region(Region::new(region.clone()))
                    .endpoint_url(endpoint)
                    .credentials_provider(Credentials::new(
                        access_key,
                        secret_key,
                        session_token.clone().filter(|s| !s.is_empty()),
                        None,
                        "kvault",
                    ))
                    .force_path_style(*path_style)
                    .retry_config(
                        aws_sdk_s3::config::retry::RetryConfig::standard().with_max_attempts(1),
                    )
                    .timeout_config(
                        aws_sdk_s3::config::timeout::TimeoutConfig::builder()
                            .operation_timeout(std::time::Duration::from_secs(30))
                            .build(),
                    )
                    .build();
                Ok(Self::S3 {
                    client: Client::from_conf(config),
                    bucket: bucket.clone(),
                    prefix: prefix.trim_matches('/').into(),
                })
            }
            SyncConfig::Webdav {
                endpoint,
                directory,
                username,
                password,
            } => {
                let mut base = reqwest::Url::parse(&format!("{}/", endpoint.trim_end_matches('/')))
                    .map_err(|_| AppError::invalid("WebDAV 地址无效"))?;
                let mut directories = Vec::new();
                for part in directory
                    .trim_matches('/')
                    .split('/')
                    .filter(|s| !s.is_empty())
                {
                    base.path_segments_mut()
                        .map_err(|_| AppError::invalid("WebDAV 地址无效"))?
                        .pop_if_empty()
                        .push(part)
                        .push("");
                    directories.push(base.clone());
                }
                Ok(Self::Webdav {
                    client: HttpClient::builder()
                        .timeout(std::time::Duration::from_secs(30))
                        .redirect(reqwest::redirect::Policy::none())
                        .build()
                        .map_err(|_| network())?,
                    base,
                    directories,
                    username: username.clone(),
                    password: password.clone(),
                })
            }
        }
    }
    fn key(prefix: &str, name: &str) -> String {
        if prefix.is_empty() {
            name.into()
        } else {
            format!("{prefix}/{name}")
        }
    }
    pub async fn ensure_directory(&self) -> Result<()> {
        if let Self::Webdav {
            client,
            directories,
            username,
            password,
            ..
        } = self
        {
            for url in directories {
                let response = client
                    .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), url.clone())
                    .basic_auth(username, Some(password))
                    .send()
                    .await
                    .map_err(|_| network())?;
                if !response.status().is_success()
                    && response.status() != StatusCode::METHOD_NOT_ALLOWED
                {
                    return Err(http_error(response.status().as_u16()));
                }
            }
        }
        Ok(())
    }
    pub async fn fetch(&self, name: &str) -> Result<Option<RemoteFile>> {
        match self {
            Self::S3 {
                client,
                bucket,
                prefix,
            } => {
                let result = client
                    .get_object()
                    .bucket(bucket)
                    .key(Self::key(prefix, name))
                    .send()
                    .await;
                let output = match result {
                    Ok(o) => o,
                    Err(e) => {
                        if e.raw_response().is_some_and(|r| r.status().as_u16() == 404) {
                            return Ok(None);
                        }
                        return Err(e
                            .raw_response()
                            .map_or_else(network, |r| http_error(r.status().as_u16())));
                    }
                };
                let etag = output
                    .e_tag()
                    .filter(|s| !s.is_empty() && !s.starts_with("W/"))
                    .ok_or_else(unsupported)?
                    .to_string();
                if output.content_length().unwrap_or(0) > MAX_FILE_SIZE as i64 {
                    return Err(AppError::invalid("远端文件过大"));
                }
                let mut bytes = vec![];
                output
                    .body
                    .into_async_read()
                    .take(MAX_FILE_SIZE as u64 + 1)
                    .read_to_end(&mut bytes)
                    .await
                    .map_err(|_| network())?;
                if bytes.len() > MAX_FILE_SIZE {
                    return Err(AppError::invalid("远端文件过大"));
                }
                Ok(Some(RemoteFile { bytes, etag }))
            }
            Self::Webdav {
                client,
                base,
                username,
                password,
                ..
            } => {
                let response = client
                    .get(base.join(name).map_err(|_| network())?)
                    .basic_auth(username, Some(password))
                    .send()
                    .await
                    .map_err(|_| network())?;
                if response.status() == StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                if !response.status().is_success() {
                    return Err(http_error(response.status().as_u16()));
                }
                let etag = response
                    .headers()
                    .get(reqwest::header::ETAG)
                    .and_then(|h| h.to_str().ok())
                    .filter(|s| !s.starts_with("W/"))
                    .ok_or_else(unsupported)?
                    .to_string();
                if response.content_length().unwrap_or(0) > MAX_FILE_SIZE as u64 {
                    return Err(AppError::invalid("远端文件过大"));
                }
                let mut response = response;
                let mut bytes = Vec::new();
                while let Some(chunk) = response.chunk().await.map_err(|_| network())? {
                    if bytes.len() + chunk.len() > MAX_FILE_SIZE {
                        return Err(AppError::invalid("远端文件过大"));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok(Some(RemoteFile { bytes, etag }))
            }
        }
    }
    pub async fn put(&self, name: &str, bytes: Vec<u8>, etag: Option<&str>) -> Result<()> {
        match self {
            Self::S3 {
                client,
                bucket,
                prefix,
            } => {
                let mut request = client
                    .put_object()
                    .bucket(bucket)
                    .key(Self::key(prefix, name))
                    .body(ByteStream::from(bytes))
                    .content_type("application/octet-stream");
                request = match etag {
                    Some(tag) => request.if_match(tag),
                    None => request.if_none_match("*"),
                };
                request.send().await.map_err(|e| {
                    e.raw_response()
                        .map_or_else(network, |r| http_error(r.status().as_u16()))
                })?;
            }
            Self::Webdav {
                client,
                base,
                username,
                password,
                ..
            } => {
                let request = client
                    .put(base.join(name).map_err(|_| network())?)
                    .basic_auth(username, Some(password))
                    .header("Content-Type", "application/octet-stream")
                    .body(bytes);
                let request = match etag {
                    Some(tag) => request.header("If-Match", tag),
                    None => request.header("If-None-Match", "*"),
                };
                let response = request.send().await.map_err(|_| network())?;
                if !response.status().is_success() {
                    return Err(http_error(response.status().as_u16()));
                }
            }
        }
        Ok(())
    }
    async fn delete(&self, name: &str) -> Result<()> {
        match self {
            Self::S3 {
                client,
                bucket,
                prefix,
            } => {
                client
                    .delete_object()
                    .bucket(bucket)
                    .key(Self::key(prefix, name))
                    .send()
                    .await
                    .map_err(|_| network())?;
            }
            Self::Webdav {
                client,
                base,
                username,
                password,
                ..
            } => {
                let response = client
                    .delete(base.join(name).map_err(|_| network())?)
                    .basic_auth(username, Some(password))
                    .send()
                    .await
                    .map_err(|_| network())?;
                if !response.status().is_success() {
                    return Err(http_error(response.status().as_u16()));
                }
            }
        }
        Ok(())
    }
    pub async fn probe(&self) -> Result<()> {
        self.ensure_directory().await?;
        let name = format!(".kvault-probe-{}", id());
        self.put(&name, b"kvault conditional write probe".to_vec(), None)
            .await?;
        let check = async {
            let first = self.fetch(&name).await?.ok_or_else(unsupported)?;
            if !matches!(self.put(&name, b"must fail".to_vec(), None).await, Err(e) if e.code == "precondition") { return Err(unsupported()); }
            if !matches!(self.put(&name, b"must fail".to_vec(), Some("\"kvault-invalid-etag\"")).await, Err(e) if e.code == "precondition") { return Err(unsupported()); }
            self.put(&name, b"verified".to_vec(), Some(&first.etag)).await?;
            let after = self.fetch(&name).await?.ok_or_else(unsupported)?;
            if after.bytes != b"verified" || after.etag == first.etag { return Err(unsupported()); }
            Ok(())
        }.await;
        let cleanup = self.delete(&name).await;
        check.and(cleanup)
    }
}

use crate::{
    sync::SyncConfig,
    vault::{
        AppState, Shared,
        crypto::Keys,
        model::{LocalState, Table, id},
        storage::Storage,
    },
};
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tokio::sync::Notify;

#[derive(Default)]
pub struct MockData {
    pub files: Mutex<HashMap<String, (Vec<u8>, String)>>,
    pub version: AtomicU64,
    pub ignore_conditions: AtomicBool,
    pub signed_requests: AtomicU64,
    pub pause_upload: AtomicBool,
    pub entered: Notify,
    pub release: Notify,
    pub reject_put: AtomicBool,
    pub ambiguous_put: AtomicBool,
    pub fail_get: AtomicBool,
}
pub struct Server {
    pub endpoint: String,
    pub data: Arc<MockData>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    pub async fn start() -> Self {
        let data = Arc::new(MockData::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().fallback(handler).with_state(data.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            endpoint,
            data,
            task,
        }
    }
    pub fn config(&self) -> SyncConfig {
        SyncConfig::Webdav {
            endpoint: self.endpoint.clone(),
            directory: "vaults".into(),
            username: "user".into(),
            password: "private-service-password".into(),
        }
    }
    pub fn fixture(&self) -> (Shared, tempfile::TempDir, u64) {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(AppState::new(Storage::new(dir.path().into()).unwrap()));
        let keys = Keys::new("old-password", id(), 1).unwrap();
        let mut local = LocalState {
            config: Some(self.config()),
            ..Default::default()
        };
        local.document.tables.push(Table {
            id: id(),
            name: "工作".into(),
            created_at: 1,
            updated_at: 1,
            deleted: false,
        });
        let mut vault = state.vault.lock().unwrap();
        local.sync_profile = vault
            .storage
            .save_sync(&keys.header.vault_id, local.config.as_ref())
            .unwrap();
        vault.storage.save(&local, &keys).unwrap();
        let snapshot = vault.install(keys, local).unwrap();
        drop(vault);
        (state, dir, snapshot.session_id)
    }
}
async fn handler(
    State(data): State<Arc<MockData>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if headers.get(header::AUTHORIZATION).is_some_and(|v| {
        v.to_str()
            .unwrap_or_default()
            .starts_with("AWS4-HMAC-SHA256")
    }) {
        data.signed_requests.fetch_add(1, Ordering::SeqCst);
    }
    let path = uri.path().to_string();
    let real = path.ends_with("/vault.kvault");
    if method.as_str() == "MKCOL" {
        return StatusCode::CREATED.into_response();
    }
    if method == Method::GET {
        if real && data.fail_get.swap(false, Ordering::SeqCst) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        return match data.files.lock().unwrap().get(&path) {
            Some((bytes, etag)) => (
                StatusCode::OK,
                [(header::ETAG, etag.clone())],
                bytes.clone(),
            )
                .into_response(),
            None => (
                StatusCode::NOT_FOUND,
                "<Error><Code>NoSuchKey</Code></Error>",
            )
                .into_response(),
        };
    }
    if method == Method::DELETE {
        data.files.lock().unwrap().remove(&path);
        return StatusCode::NO_CONTENT.into_response();
    }
    if method == Method::PUT {
        if real && data.pause_upload.swap(false, Ordering::SeqCst) {
            data.entered.notify_one();
            data.release.notified().await;
        }
        if real && data.reject_put.swap(false, Ordering::SeqCst) {
            return StatusCode::PRECONDITION_FAILED.into_response();
        }
        let mut files = data.files.lock().unwrap();
        if !data.ignore_conditions.load(Ordering::SeqCst) {
            let etag = files.get(&path).map(|(_, tag)| tag.as_str());
            if headers.get(header::IF_NONE_MATCH).is_some() && etag.is_some() {
                return StatusCode::PRECONDITION_FAILED.into_response();
            }
            if let Some(expected) = headers.get(header::IF_MATCH)
                && etag != expected.to_str().ok()
            {
                return StatusCode::PRECONDITION_FAILED.into_response();
            }
        }
        let version = data.version.fetch_add(1, Ordering::SeqCst) + 1;
        let etag = format!("\"v{version}\"");
        files.insert(path, (body.to_vec(), etag.clone()));
        if real && data.ambiguous_put.swap(false, Ordering::SeqCst) {
            data.fail_get.store(true, Ordering::SeqCst);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        return (StatusCode::OK, [(header::ETAG, etag)], "").into_response();
    }
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}

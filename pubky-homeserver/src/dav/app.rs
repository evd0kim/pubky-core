use std::{convert::Infallible, net::SocketAddr};
use dav_server::{fakels::FakeLs, localfs::LocalFs, DavHandler};


use crate::app_context::AppContext;
use crate::{AppContextConversionError, PersistentDataDir};
use axum::{Router, ServiceExt};
use axum::body::Body;
use axum::http::Request;
use tokio::task::JoinHandle;

use std::sync::Arc;

/// Errors that can occur when building a `DavServer`.
#[derive(thiserror::Error, Debug)]
pub enum DavServerBuildError {
    /// Failed to create admin server.
    #[error("Failed to create Dav server: {0}")]
    Server(anyhow::Error),
    /// Failed to boostrap from the data directory.
    #[error("Failed to boostrap from the data directory: {0}")]
    DataDir(AppContextConversionError),
}

/// WebDAV server
///
/// Set with Axum
pub struct DavServer {
    dav_handle: Arc<DavHandler>,
    join_handle: JoinHandle<()>,
    socket: SocketAddr,
    password: String,
}

impl DavServer {
    /// Create a new admin server from a data directory.
    pub async fn from_data_dir(data_dir: PersistentDataDir) -> Result<Self, DavServerBuildError> {
        let context = AppContext::try_from(data_dir).map_err(DavServerBuildError::DataDir)?;
        Self::start(&context).await
    }

    /// Run the admin server.
    pub async fn start(context: &AppContext) -> Result<Self, DavServerBuildError> {
        //let socket = context.config_toml.admin.listen_socket;

        let dir = "/tmp";
        let password = context.config_toml.admin.admin_password.clone();
        let socket: SocketAddr = ([127, 0, 0, 1], 10000).into();

        let dav_handler = DavHandler::builder()
            .filesystem(LocalFs::new(dir, false, false, false))
            .locksystem(FakeLs::new())
            .build_handler();

        let dav_handle = Arc::new(dav_handler.clone());

        let dav_handle_clone = dav_handle.clone();
        let webdav_service = tower::service_fn(move |req: Request<Body>| {
            let webdav_server = dav_handle_clone.clone();
            async move {
                Ok::<_, Infallible>(webdav_server.handle(req).await)
            }
        });

        // Create the Axum router with WebDAV handler
        let app = Router::new()
            .route_service("/webdav", webdav_service)
            .into_make_service();

        // Create TCP listener
        let listener = tokio::net::TcpListener::bind(socket).await.map_err(|e| DavServerBuildError::Server(e.into()))?;
        let socket = listener.local_addr().map_err(|e| DavServerBuildError::Server(e.into()))?;

        println!("Launching Axum");
        // Spawn the server
        let join_handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!("WebDAV server error: {}", e);
            }
        });

        Ok(Self {
            dav_handle,
            join_handle,
            socket,
            password,
        })
    }

    /// Get the socket address of the admin server.
    pub fn listen_socket(&self) -> SocketAddr {
        self.socket
    }
}

impl Drop for DavServer {
    fn drop(&mut self) {
        self.join_handle.abort();
    }
}

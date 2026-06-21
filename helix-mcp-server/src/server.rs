//! MCP server: Unix socket listener and accept loop.
//!
//! The `HelixMcpServer` binds to a Unix domain socket (or TCP on Windows),
//! accepts incoming connections (up to `max_connections`), and spawns a
//! `Session` for each.

use std::sync::Arc;

use crate::config::McpConfig;
use crate::context::McpContext;
use crate::security::ConfirmationRequest;
use crate::session::Session;

/// The MCP server for Helix.
///
/// Binds to a Unix domain socket (or TCP on Windows) and accepts client
/// connections. Each connection is handled by a `Session` that implements
/// the MCP protocol.
pub struct HelixMcpServer {
    context: Arc<McpContext>,
    config: McpConfig,
    confirmation_tx: tokio::sync::mpsc::UnboundedSender<ConfirmationRequest>,
}

impl HelixMcpServer {
    /// Create a new MCP server with the given context, configuration, and
    /// confirmation channel.
    pub fn new(
        context: Arc<McpContext>,
        config: McpConfig,
        confirmation_tx: tokio::sync::mpsc::UnboundedSender<ConfirmationRequest>,
    ) -> Self {
        HelixMcpServer {
            context,
            config,
            confirmation_tx,
        }
    }

    /// Start the server: bind to the Unix socket and accept connections.
    ///
    /// This method runs indefinitely until the server is shut down or an
    /// unrecoverable error occurs.
    #[cfg(unix)]
    pub async fn bind_and_serve(self) -> anyhow::Result<()> {
        let socket_path = self.config.socket_path();
        log::info!("MCP server starting on {}", socket_path.display());

        // Remove stale socket file if it exists
        let _ = std::fs::remove_file(&socket_path);

        let listener = tokio::net::UnixListener::bind(&socket_path)?;
        log::info!("MCP server listening on {}", socket_path.display());

        self.accept_loop_unix(listener).await
    }

    /// Start the server: bind to a TCP port and accept connections.
    ///
    /// This is used on non-Unix platforms (Windows) where Unix domain sockets
    /// are not available. A metadata file is written for auto-discovery.
    #[cfg(not(unix))]
    pub async fn bind_and_serve(self) -> anyhow::Result<()> {
        let addr = format!("127.0.0.1:{}", self.config.tcp_port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        let local_addr = listener.local_addr()?;
        let port = local_addr.port();

        log::info!(
            "MCP server listening on {} (Windows TCP fallback)",
            local_addr
        );

        // Write metadata file for auto-discovery
        let mut meta_dir = std::env::temp_dir();
        meta_dir.push("helix-mcp");
        std::fs::create_dir_all(&meta_dir).ok();

        let meta_file = meta_dir.join(format!("{}.json", std::process::id()));
        let meta = serde_json::json!({
            "pid": std::process::id(),
            "worktree": std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            "port": port,
            "open_files": [],
            "mode": "normal",
            "started_at": {
                let dur = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                dur.as_secs().to_string()
            }
        });
        if let Ok(json) = serde_json::to_string_pretty(&meta) {
            let _ = std::fs::write(&meta_file, json);
            log::info!("MCP metadata written to {}", meta_file.display());
        }

        self.accept_loop_tcp(listener).await
    }

    #[cfg(unix)]
    async fn accept_loop_unix(self, listener: tokio::net::UnixListener) -> anyhow::Result<()> {
        let max = self.config.max_connections;

        loop {
            let (stream, addr) = listener.accept().await?;
            log::info!("MCP connection from {:?}", addr);

            let current = self.context.connection_count();
            if current >= max {
                log::warn!(
                    "MCP connection limit reached ({}/{}), rejecting new connection",
                    current,
                    max
                );
                continue;
            }

            let context = Arc::clone(&self.context);
            let confirmation_tx = self.confirmation_tx.clone();

            tokio::spawn(async move {
                let mut session = Session::new(stream, context, confirmation_tx);
                if let Err(e) = session.run().await {
                    log::error!("MCP session error: {}", e);
                }
                log::info!("MCP session ended");
            });
        }
    }

    #[cfg(not(unix))]
    async fn accept_loop_tcp(self, listener: tokio::net::TcpListener) -> anyhow::Result<()> {
        let max = self.config.max_connections;

        loop {
            let (stream, addr) = listener.accept().await?;
            log::info!("MCP connection from {}", addr);

            let current = self.context.connection_count();
            if current >= max {
                log::warn!(
                    "MCP connection limit reached ({}/{}), rejecting new connection",
                    current,
                    max
                );
                continue;
            }

            let context = Arc::clone(&self.context);
            let confirmation_tx = self.confirmation_tx.clone();

            tokio::spawn(async move {
                let mut session = Session::new(stream, context, confirmation_tx);
                if let Err(e) = session.run().await {
                    log::error!("MCP session error: {}", e);
                }
                log::info!("MCP session ended");
            });
        }
    }
}

//! Per-connection MCP session handler.
//!
//! Each `Session` reads JSON-RPC messages from a single client connection,
//! dispatches them to the appropriate handler, and writes responses back.
//! Uses newline-delimited JSON framing via `helix_mcp::transport`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use helix_mcp::{
    jsonrpc::{self, Call, Id, Output},
    transport,
};
use parking_lot::RwLock;
use serde_json::Value;

use crate::audit::AuditEntry;
use crate::context::McpContext;
use crate::prompts;
use crate::rate_limit::RateLimiter;
use crate::resources;
use crate::security::{self, ConfirmationRequest, OperationTier};
use crate::tools;

#[cfg(unix)]
type SessionStream = tokio::net::UnixStream;
#[cfg(not(unix))]
type SessionStream = tokio::net::TcpStream;

/// An active MCP session for a single client connection.
pub struct Session {
    context: Arc<McpContext>,
    stream: Option<SessionStream>,
    initialized: AtomicBool,
    confirmation_tx: tokio::sync::mpsc::UnboundedSender<ConfirmationRequest>,
    subscriptions: Arc<RwLock<HashSet<String>>>,
    broadcast_rx: tokio::sync::broadcast::Receiver<()>,
    rate_limiter: RateLimiter,
    audit_logger: Option<Arc<crate::audit::AuditLogger>>,
}

impl Session {
    /// Create a new session for the given transport stream, context, and
    /// confirmation channel.
    pub fn new(
        stream: SessionStream,
        context: Arc<McpContext>,
        confirmation_tx: tokio::sync::mpsc::UnboundedSender<ConfirmationRequest>,
    ) -> Self {
        let broadcast_rx = context.subscribe_to_updates();
        let audit_logger = context.audit_logger.clone();
        // Bump the connection count
        context.increment_connections();
        Session {
            context,
            stream: Some(stream),
            initialized: AtomicBool::new(false),
            confirmation_tx,
            subscriptions: Arc::new(RwLock::new(HashSet::new())),
            broadcast_rx,
            rate_limiter: RateLimiter::new(100),
            audit_logger,
        }
    }

    /// Run the session: read messages, dispatch, write responses.
    ///
    /// Continues until the connection is closed or an unrecoverable error occurs.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        let stream = self.stream.take().expect("session already running");
        let (reader, mut writer) = tokio::io::split(stream);
        let mut reader = tokio::io::BufReader::new(reader);

        loop {
            // Check for broadcast updates before blocking on read
            match self.broadcast_rx.try_recv() {
                Ok(()) => {
                    let subs = self.subscriptions.read();
                    for uri in subs.iter() {
                        let notification = serde_json::json!({
                            "jsonrpc": "2.0",
                            "method": "notifications/resources/updated",
                            "params": {
                                "uri": uri
                            }
                        });
                        if let Ok(resp) = serde_json::to_string(&notification) {
                            let _ = transport::write_message(&mut writer, &resp).await;
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {}
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                    // Sender is gone — no more updates will come
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                    // We missed some updates; still notify current subscriptions
                }
            }

            let msg = match transport::read_message(&mut reader).await {
                Ok(m) => m,
                Err(transport::TransportError::StreamClosed) => break,
                Err(e) => {
                    log::error!("MCP read error: {}", e);
                    break;
                }
            };

            let call: Call = match serde_json::from_str(&msg) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("MCP parse error: {}", e);
                    let error = jsonrpc::Error {
                        code: helix_mcp::protocol::PARSE_ERROR,
                        message: format!("Parse error: {}", e),
                        data: None,
                    };
                    let failure = Output::Failure(jsonrpc::Failure {
                        jsonrpc: Some(jsonrpc::Version::V2),
                        error,
                        id: Id::Null,
                    });
                    if let Ok(resp) = serde_json::to_string(&failure) {
                        let _ = transport::write_message(&mut writer, &resp).await;
                    }
                    continue;
                }
            };

            match call {
                Call::MethodCall(mc) => {
                    // Apply rate limiting before dispatching
                    self.rate_limiter.acquire();

                    let output = self.dispatch_method(&mc.method, &mc.params, &mc.id).await;
                    if let Ok(resp) = serde_json::to_string(&output) {
                        let _ = transport::write_message(&mut writer, &resp).await;
                    }
                }
                Call::Notification(n) => {
                    log::debug!("MCP notification received: {}", n.method);
                }
            }
        }

        Ok(())
    }

    /// Dispatch a method call to the appropriate handler.
    async fn dispatch_method(&self, method: &str, params: &Option<Value>, id: &Id) -> Output {
        match method {
            "initialize" => self.handle_initialize(params, id).await,
            "tools/list" => self.handle_tools_list(params, id).await,
            "tools/call" => self.handle_tools_call(params, id).await,
            "resources/list" => self.handle_resources_list(params, id).await,
            "resources/read" => self.handle_resources_read(params, id).await,
            "resources/subscribe" => self.handle_resources_subscribe(params, id).await,
            "resources/unsubscribe" => self.handle_resources_unsubscribe(params, id).await,
            "prompts/list" => self.handle_prompts_list(params, id).await,
            "prompts/get" => self.handle_prompts_get(params, id).await,
            "ping" => jsonrpc::success(id.clone(), Value::Object(serde_json::Map::new())),
            _ => jsonrpc::failure(
                id.clone(),
                jsonrpc::Error::method_not_found(format!("Method not found: {}", method)),
            ),
        }
    }

    async fn handle_initialize(&self, _params: &Option<Value>, id: &Id) -> Output {
        if self.initialized.load(Ordering::Relaxed) {
            return jsonrpc::failure(
                id.clone(),
                jsonrpc::Error {
                    code: helix_mcp::protocol::INVALID_REQUEST,
                    message: "Server already initialized".to_string(),
                    data: None,
                },
            );
        }

        self.initialized.store(true, Ordering::Relaxed);

        let result = serde_json::to_value(helix_mcp::protocol::InitializeResult {
            protocol_version: helix_mcp::MCP_VERSION.to_string(),
            capabilities: helix_mcp::protocol::ServerCapabilities {
                tools: Some(helix_mcp::protocol::ToolCapabilities { list_changed: true }),
                resources: Some(helix_mcp::protocol::ResourceCapabilities {
                    subscribe: true,
                    list_changed: true,
                }),
                prompts: Some(helix_mcp::protocol::PromptCapabilities { list_changed: true }),
                logging: None,
            },
            server_info: Some(helix_mcp::protocol::Implementation {
                name: "helix-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            }),
        })
        .unwrap_or(Value::Null);

        jsonrpc::success(id.clone(), result)
    }

    async fn handle_tools_list(&self, _params: &Option<Value>, id: &Id) -> Output {
        let tools = tools::all_tools();
        let result = serde_json::to_value(helix_mcp::protocol::ToolsListResult { tools })
            .unwrap_or(Value::Null);
        jsonrpc::success(id.clone(), result)
    }

    async fn handle_tools_call(&self, params: &Option<Value>, id: &Id) -> Output {
        let request: helix_mcp::protocol::ToolsCallRequest = match params {
            Some(p) => match serde_json::from_value(p.clone()) {
                Ok(r) => r,
                Err(e) => {
                    return jsonrpc::failure(
                        id.clone(),
                        jsonrpc::Error::invalid_params(format!("Invalid tool call params: {}", e)),
                    );
                }
            },
            None => {
                return jsonrpc::failure(
                    id.clone(),
                    jsonrpc::Error::invalid_params("Missing tool call parameters"),
                );
            }
        };

        // --- Three-tier security gating ---
        let tier = security::tool_tier(&request.name);

        if tier == OperationTier::Mutate {
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();

            let snapshot = self.context.snapshot();
            let summary = tools::confirmation_summary(&request.name, &request.arguments, &snapshot);

            let confirm_req = ConfirmationRequest {
                tool_name: request.name.clone(),
                summary,
                tier,
                response_tx,
            };

            if let Err(e) = self.confirmation_tx.send(confirm_req) {
                log::error!("Failed to send confirmation request: {}", e);
                return jsonrpc::failure(
                    id.clone(),
                    jsonrpc::Error {
                        code: helix_mcp::protocol::INTERNAL_ERROR,
                        message: "Confirmation channel closed".to_string(),
                        data: None,
                    },
                );
            }

            match response_rx.await {
                Ok(true) => {
                    let result = self.context.mutate(|snap| {
                        tools::apply_mutation(&request.name, &request.arguments, snap)
                    });
                    return match result {
                        Ok(content) => {
                            // Audit logging for successful Mutate-tier operations
                            if let Some(ref logger) = self.audit_logger {
                                let doc_id = request
                                    .arguments
                                    .as_ref()
                                    .and_then(|a| a.get("doc_id"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                let summary = tools::confirmation_summary(
                                    &request.name,
                                    &request.arguments,
                                    &self.context.snapshot(),
                                );
                                let ts = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs().to_string())
                                    .unwrap_or_default();
                                let entry = AuditEntry {
                                    timestamp: ts,
                                    tool: request.name.clone(),
                                    doc_id: doc_id.to_string(),
                                    summary,
                                    client_id: None,
                                };
                                logger.log(&entry);
                            }

                            let result =
                                serde_json::to_value(helix_mcp::protocol::ToolsCallResult {
                                    content,
                                })
                                .unwrap_or(Value::Null);
                            jsonrpc::success(id.clone(), result)
                        }
                        Err(e) => jsonrpc::failure(
                            id.clone(),
                            jsonrpc::Error {
                                code: helix_mcp::protocol::INTERNAL_ERROR,
                                message: e.to_string(),
                                data: None,
                            },
                        ),
                    };
                }
                Ok(false) => {
                    return jsonrpc::failure(
                        id.clone(),
                        jsonrpc::Error {
                            code: helix_mcp::protocol::INVALID_REQUEST,
                            message: "User denied the operation".to_string(),
                            data: None,
                        },
                    );
                }
                Err(_) => {
                    return jsonrpc::failure(
                        id.clone(),
                        jsonrpc::Error {
                            code: helix_mcp::protocol::INVALID_REQUEST,
                            message: "Confirmation timed out".to_string(),
                            data: None,
                        },
                    );
                }
            }
        }
        // --- End security gating ---

        let snapshot = self.context.snapshot();
        match tools::call_tool(&request.name, &request.arguments, &snapshot) {
            Ok(content) => {
                let result = serde_json::to_value(helix_mcp::protocol::ToolsCallResult { content })
                    .unwrap_or(Value::Null);
                jsonrpc::success(id.clone(), result)
            }
            Err(e) => jsonrpc::failure(
                id.clone(),
                jsonrpc::Error {
                    code: helix_mcp::protocol::INTERNAL_ERROR,
                    message: e.to_string(),
                    data: None,
                },
            ),
        }
    }

    async fn handle_resources_list(&self, _params: &Option<Value>, id: &Id) -> Output {
        let snapshot = self.context.snapshot();
        let resources = resources::all_resources(&snapshot);
        let result = serde_json::to_value(helix_mcp::protocol::ResourcesListResult { resources })
            .unwrap_or(Value::Null);
        jsonrpc::success(id.clone(), result)
    }

    async fn handle_resources_read(&self, params: &Option<Value>, id: &Id) -> Output {
        let request: helix_mcp::protocol::ReadResourceRequest = match params {
            Some(p) => match serde_json::from_value(p.clone()) {
                Ok(r) => r,
                Err(e) => {
                    return jsonrpc::failure(
                        id.clone(),
                        jsonrpc::Error::invalid_params(format!(
                            "Invalid resource read params: {}",
                            e
                        )),
                    );
                }
            },
            None => {
                return jsonrpc::failure(
                    id.clone(),
                    jsonrpc::Error::invalid_params("Missing resource read parameters"),
                );
            }
        };

        let snapshot = self.context.snapshot();
        match resources::read_resource(&request.uri, &snapshot) {
            Ok(contents) => {
                let result =
                    serde_json::to_value(helix_mcp::protocol::ReadResourceResult { contents })
                        .unwrap_or(Value::Null);
                jsonrpc::success(id.clone(), result)
            }
            Err(e) => jsonrpc::failure(
                id.clone(),
                jsonrpc::Error {
                    code: helix_mcp::protocol::INTERNAL_ERROR,
                    message: e.to_string(),
                    data: None,
                },
            ),
        }
    }

    async fn handle_resources_subscribe(&self, params: &Option<Value>, id: &Id) -> Output {
        let request: helix_mcp::protocol::SubscribeRequest = match params {
            Some(p) => match serde_json::from_value(p.clone()) {
                Ok(r) => r,
                Err(e) => {
                    return jsonrpc::failure(
                        id.clone(),
                        jsonrpc::Error::invalid_params(format!("Invalid subscribe params: {}", e)),
                    );
                }
            },
            None => {
                return jsonrpc::failure(
                    id.clone(),
                    jsonrpc::Error::invalid_params("Missing subscribe parameters"),
                );
            }
        };

        self.subscriptions.write().insert(request.uri);
        jsonrpc::success(id.clone(), Value::Object(serde_json::Map::new()))
    }

    async fn handle_resources_unsubscribe(&self, params: &Option<Value>, id: &Id) -> Output {
        let request: helix_mcp::protocol::UnsubscribeRequest = match params {
            Some(p) => match serde_json::from_value(p.clone()) {
                Ok(r) => r,
                Err(e) => {
                    return jsonrpc::failure(
                        id.clone(),
                        jsonrpc::Error::invalid_params(format!(
                            "Invalid unsubscribe params: {}",
                            e
                        )),
                    );
                }
            },
            None => {
                return jsonrpc::failure(
                    id.clone(),
                    jsonrpc::Error::invalid_params("Missing unsubscribe parameters"),
                );
            }
        };

        self.subscriptions.write().remove(&request.uri);
        jsonrpc::success(id.clone(), Value::Object(serde_json::Map::new()))
    }

    async fn handle_prompts_list(&self, _params: &Option<Value>, id: &Id) -> Output {
        let prompts = prompts::all_prompts();
        let result = serde_json::to_value(helix_mcp::protocol::PromptsListResult { prompts })
            .unwrap_or(Value::Null);
        jsonrpc::success(id.clone(), result)
    }

    async fn handle_prompts_get(&self, params: &Option<Value>, id: &Id) -> Output {
        let request: helix_mcp::protocol::GetPromptRequest = match params {
            Some(p) => match serde_json::from_value(p.clone()) {
                Ok(r) => r,
                Err(e) => {
                    return jsonrpc::failure(
                        id.clone(),
                        jsonrpc::Error::invalid_params(format!("Invalid prompt get params: {}", e)),
                    );
                }
            },
            None => {
                return jsonrpc::failure(
                    id.clone(),
                    jsonrpc::Error::invalid_params("Missing prompt get parameters"),
                );
            }
        };

        let snapshot = self.context.snapshot();
        match prompts::get_prompt(&request.name, &request.arguments, &snapshot) {
            Ok(result) => {
                let value = serde_json::to_value(result).unwrap_or(Value::Null);
                jsonrpc::success(id.clone(), value)
            }
            Err(e) => jsonrpc::failure(
                id.clone(),
                jsonrpc::Error {
                    code: helix_mcp::protocol::INTERNAL_ERROR,
                    message: e,
                    data: None,
                },
            ),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.context.decrement_connections();
    }
}

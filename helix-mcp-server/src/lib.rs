//! Helix MCP server — bridges the MCP protocol to the Helix editor.
//!
//! This crate provides:
//! - An MCP server that listens on a Unix domain socket
//! - Tool and resource implementations backed by editor state
//! - A snapshot-based context for thread-safe editor state access
//! - Three-tier security with confirmation gating

pub mod audit;
pub mod config;
pub mod context;
pub mod prompts;
pub mod rate_limit;
pub mod resources;
pub mod security;
pub mod server;
pub mod session;
pub mod tools;

pub use config::McpConfig;
pub use context::McpContext;
pub use server::HelixMcpServer;

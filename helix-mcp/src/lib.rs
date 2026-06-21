//! Model Context Protocol (MCP) implementation for Helix.
//!
//! This crate provides the pure MCP protocol types and transport abstraction
//! with zero dependencies on any `helix-*` crate. It can be used by external
//! consumers to build MCP clients or servers.

pub mod jsonrpc;
pub mod protocol;
pub mod transport;

/// The MCP protocol version implemented by this crate.
pub const MCP_VERSION: &str = "2024-11-05";

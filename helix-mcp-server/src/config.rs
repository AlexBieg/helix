use std::path::PathBuf;

/// Configuration for the MCP server.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
pub struct McpConfig {
    /// Whether the MCP server is enabled.
    pub enable: bool,
    /// Maximum number of simultaneous client connections.
    pub max_connections: u32,
    /// Custom Unix socket path. If `None`, the default path is used.
    pub socket: Option<PathBuf>,
    /// TCP port for Windows fallback transport (0 = OS picks).
    pub tcp_port: u16,
    /// Maximum requests per second per session (token bucket rate limit).
    pub rate_limit: u64,
}

impl Default for McpConfig {
    fn default() -> Self {
        McpConfig {
            enable: false,
            max_connections: 4,
            socket: None,
            tcp_port: 0,
            rate_limit: 100,
        }
    }
}

impl McpConfig {
    /// Resolve the Unix domain socket path for this Helix instance.
    ///
    /// Resolution order:
    /// 1. Explicit override from `config.socket`
    /// 2. `$XDG_RUNTIME_DIR/helix/mcp/{pid}.sock`
    /// 3. `$TMPDIR/helix-mcp/{pid}.sock`
    pub fn socket_path(&self) -> PathBuf {
        if let Some(ref custom) = self.socket {
            return custom.clone();
        }

        let dir = std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let mut tmp = std::env::temp_dir();
                tmp.push("helix-mcp");
                tmp
            })
            .join("mcp");

        std::fs::create_dir_all(&dir).ok();
        dir.join(format!("{}.sock", std::process::id()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_disabled() {
        let config = McpConfig::default();
        assert!(!config.enable);
        assert_eq!(config.max_connections, 4);
        assert!(config.socket.is_none());
        assert_eq!(config.tcp_port, 0);
        assert_eq!(config.rate_limit, 100);
    }

    #[test]
    fn test_socket_path_custom() {
        let config = McpConfig {
            socket: Some(PathBuf::from("/tmp/custom.sock")),
            ..McpConfig::default()
        };
        assert_eq!(config.socket_path(), PathBuf::from("/tmp/custom.sock"));
    }

    #[test]
    fn test_socket_path_default_includes_pid() {
        let config = McpConfig::default();
        let path = config.socket_path();
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(filename.ends_with(".sock"));
        assert!(filename.contains(&std::process::id().to_string()));
    }

    #[test]
    fn test_socket_path_default_in_mcp_dir() {
        let config = McpConfig::default();
        let path = config.socket_path();
        assert!(path.to_string_lossy().contains("mcp"));
    }

    #[test]
    fn test_deserialize_from_toml() {
        let toml = r#"
enable = true
max-connections = 8
socket = "/tmp/test.sock"
"#;
        let config: McpConfig = toml::from_str(toml).unwrap();
        assert!(config.enable);
        assert_eq!(config.max_connections, 8);
        assert_eq!(config.socket, Some(PathBuf::from("/tmp/test.sock")));
    }

    #[test]
    fn test_deserialize_partial() {
        let config: McpConfig = toml::from_str("enable = true").unwrap();
        assert!(config.enable);
        assert_eq!(config.max_connections, 4); // default
        assert!(config.socket.is_none()); // default
    }
}

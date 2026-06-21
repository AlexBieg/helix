use anyhow::{Context, Error, Result};
use helix_loader::VERSION_AND_GIT_HASH;
use helix_term::application::Application;
use helix_term::args::Args;
use helix_term::config::{Config, ConfigLoadError};

fn setup_logging(verbosity: u64) -> Result<()> {
    let level = match verbosity {
        0 => log::LevelFilter::Warn,
        1 => log::LevelFilter::Info,
        2 => log::LevelFilter::Debug,
        _3_or_more => log::LevelFilter::Trace,
    };

    helix_term::logging::init_file(level, &helix_loader::log_file())?;

    Ok(())
}

fn main() -> Result<()> {
    let exit_code = main_impl()?;
    std::process::exit(exit_code);
}

#[tokio::main]
async fn main_impl() -> Result<i32> {
    #[cfg_attr(not(feature = "mcp"), allow(unused_mut))]
    let mut args = Args::parse_args().context("could not parse arguments")?;

    helix_loader::initialize_config_file(args.config_file.clone());
    helix_loader::initialize_log_file(args.log_file.clone());

    // Help has a higher priority and should be handled separately.
    if args.display_help {
        print!(
            "\
{} {}
{}
{}

USAGE:
    hx [FLAGS] [files]...

ARGS:
    <files>...    Set the input file to use, position can also be specified via file[:row[:col]]

FLAGS:
    -h, --help                     Print help information
    --strict                       Bail on error for commands that can fail.
    --tutor                        Load the tutorial
    --health [CATEGORY]            Check for potential errors in editor setup
                                   CATEGORY can be a language or one of 'clipboard', 'languages',
                                   'all-languages' or 'all'. 'languages' is filtered according to
                                   user config, 'all-languages' and 'all' are not. If not specified,
                                   the default is the same as 'all', but with languages filtering.
    -g, --grammar {{fetch|build}}    Fetch or builds tree-sitter grammars listed in languages.toml.
    -c, --config <file>            Specify a file to use for configuration
    -v                             Increase logging verbosity each use for up to 3 times
    --log <file>                   Specify a file to use for logging
                                   (default file: {})
    -V, --version                  Print version information
    --vsplit                       Split all given files vertically into different windows
    --hsplit                       Split all given files horizontally into different windows
    -w, --working-dir <path>       Specify an initial working directory
    +[N]                           Open the first given file at line number N, or the last line, if
                                   N is not specified.
",
            env!("CARGO_PKG_NAME"),
            VERSION_AND_GIT_HASH,
            env!("CARGO_PKG_AUTHORS"),
            env!("CARGO_PKG_DESCRIPTION"),
            helix_loader::default_log_file().display(),
        );
        std::process::exit(0);
    }

    if args.display_version {
        println!("helix {}", VERSION_AND_GIT_HASH);
        std::process::exit(0);
    }

    if args.health {
        if let Err(err) = helix_term::health::print_health(args.health_arg) {
            // Piping to for example `head -10` requires special handling:
            // https://stackoverflow.com/a/65760807/7115678
            if err.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(err.into());
            }
        }

        std::process::exit(0);
    }

    if args.fetch_grammars {
        helix_loader::grammar::fetch_grammars(args.strict)?;
        return Ok(0);
    }

    if args.build_grammars {
        helix_loader::grammar::build_grammars(None, args.strict)?;
        return Ok(0);
    }

    #[cfg(feature = "mcp")]
    if args.mcp_info {
        let socket_path = {
            let cfg = helix_mcp_server::McpConfig::default();
            cfg.socket_path()
        };
        let info = serde_json::json!({
            "pid": std::process::id(),
            "socket": socket_path.to_string_lossy(),
            "worktree": std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            "connections": 0,
        });
        println!("{}", serde_json::to_string_pretty(&info).unwrap());
        return Ok(0);
    }

    #[cfg(feature = "mcp")]
    if args.mcp_list {
        let dir = {
            let cfg = helix_mcp_server::McpConfig::default();
            let socket = cfg.socket_path();
            socket.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| {
                let mut tmp = std::env::temp_dir();
                tmp.push("helix-mcp");
                tmp
            })
        };
        let mut instances = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if let Some(ext) = entry_path.extension() {
                    if ext == "sock" {
                        let pid_str = entry_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        let socket_path = entry_path.to_string_lossy().to_string();
                        let instance = serde_json::json!({
                            "pid": pid_str,
                            "socket": socket_path,
                        });
                        instances.push(instance);
                    }
                }
            }
        }
        // Also check temp dir for Windows metadata files
        let mut tmp_dir = std::env::temp_dir();
        tmp_dir.push("helix-mcp");
        if let Ok(entries) = std::fs::read_dir(&tmp_dir) {
            for entry in entries.flatten() {
                if let Some(ext) = entry.path().extension() {
                    if ext == "json" {
                        if let Ok(content) = std::fs::read_to_string(entry.path()) {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                                instances.push(val);
                            }
                        }
                    }
                }
            }
        }
        println!("{}", serde_json::to_string_pretty(&instances).unwrap());
        return Ok(0);
    }

    setup_logging(args.verbosity).context("failed to initialize logging")?;

    // NOTE: Set the working directory early so the correct configuration is loaded. Be aware that
    // Application::new() depends on this logic so it must be updated if this changes.
    if let Some(path) = &args.working_directory {
        helix_stdx::env::set_current_working_dir(path)?;
    } else if let Some((path, _)) = args.files.first().filter(|p| p.0.is_dir()) {
        // If the first file is a directory, it will be the working directory unless -w was specified
        helix_stdx::env::set_current_working_dir(path)?;
    } else if let Err(err) = std::env::current_dir() {
        eprintln!("Couldn't determine the current working directory: {err}");
        eprintln!("Check that it still exists, or pass an initial directory with `--working-dir`");
        return Ok(1);
    }

    let config = match Config::load_default() {
        Ok(config) => config,
        Err(ConfigLoadError::Error(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            Config::default()
        }
        Err(ConfigLoadError::Error(err)) => return Err(Error::new(err)),
        Err(ConfigLoadError::BadConfig(err)) => {
            eprintln!("Bad config: {}", err);
            eprintln!("Press <ENTER> to continue with default config");
            use std::io::Read;
            let _ = std::io::stdin().read(&mut []);
            Config::default()
        }
    };

    let lang_loader =
        helix_core::config::user_lang_loader(config.editor.insecure).unwrap_or_else(|err| {
            eprintln!("{}", err);
            eprintln!("Press <ENTER> to continue with default language config");
            use std::io::Read;
            // This waits for an enter press.
            let _ = std::io::stdin().read(&mut []);
            helix_core::config::default_lang_loader()
        });

    #[cfg(feature = "mcp")]
    if args.headless {
        // Headless mode: MCP server only, no TUI.
        args.mcp = true;

        let (confirmation_tx, mut confirmation_rx) = tokio::sync::mpsc::unbounded_channel::<
            helix_mcp_server::security::ConfirmationRequest,
        >();
        let mcp_context = std::sync::Arc::new(helix_mcp_server::McpContext::new());
        let mut mcp_config = helix_mcp_server::McpConfig::default();
        mcp_config.enable = true;
        if let Some(ref socket) = args.mcp_socket {
            mcp_config.socket = Some(socket.clone());
        }

        // Open file arguments in the snapshot so they are accessible via MCP tools
        let mut next_id: usize = 1;
        for (path, _) in &args.files {
            if !path.is_dir() {
                let doc_id = next_id.to_string();
                mcp_context.load_file(&doc_id, &path.to_string_lossy());
                next_id += 1;
            }
        }

        let server =
            helix_mcp_server::HelixMcpServer::new(mcp_context.clone(), mcp_config, confirmation_tx);
        tokio::spawn(async move {
            if let Err(e) = server.bind_and_serve().await {
                log::error!("MCP server error: {}", e);
            }
        });

        // Auto-accept confirmation requests in headless mode
        tokio::spawn(async move {
            while let Some(req) = confirmation_rx.recv().await {
                let _ = req.response_tx.send(true);
            }
        });

        log::info!(
            "Headless MCP server started on {} (pid: {})",
            helix_mcp_server::McpConfig::default()
                .socket_path()
                .display(),
            std::process::id()
        );

        use signal_hook::consts::signal::{SIGINT, SIGTERM};
        use signal_hook_tokio::Signals;
        let mut signals = Signals::new([SIGINT, SIGTERM]).context("build signal handler")?;
        use futures_util::StreamExt;
        signals.next().await;
        log::info!("Shutting down headless MCP server");
        return Ok(0);
    }

    // TODO: use the thread local executor to spawn the application task separately from the work pool
    let mut app = Application::new(args, config, lang_loader).context("unable to start Helix")?;
    let mut events = app.event_stream();

    let exit_code = app.run(&mut events).await?;

    Ok(exit_code)
}

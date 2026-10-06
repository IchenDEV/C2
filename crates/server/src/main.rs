//! `codetwo-server` — run one C2 Core with either the compact remote or the full React Web UI.
//!
//! Env: `CODETWO_HOST` (default 0.0.0.0), `CODETWO_PORT` (default 4599), `CODETWO_PAIR_TTL`
//! (pairing-token lifetime in seconds, default 900), `CODETWO_DATA_DIR`, and
//! `CODETWO_WEB_UI_DIR`. `serve` is the headless daemon for a remote machine, and `pair` asks a
//! running daemon for a fresh pairing link.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use codetwo_core::plugins::{
    AppConfig, CanvasService, CoreApp, EngineService, EventBus, PluginManager, StoreService,
};
use codetwo_server::daemon::{
    pairing_base_url, request_pairing_link, write_pairing_file, InstanceLock,
};
use codetwo_server::{
    bind_and_serve_with_canvas, bind_and_serve_with_web_ui, pairing_endpoints,
    pairing_url_for_endpoint, print_pairing, print_pairing_link, AuthState, KernelWebUiCommands,
    DEFAULT_PAIRING_TTL,
};

const HELP: &str = r#"Usage:
  codetwo-server
  codetwo-server webui [--ui-dir <path>] [--data-dir <path>] [--no-open]
  codetwo-server serve [--host <addr>] [--port <port>] [--data-dir <path>]
                       [--ui-dir <path>] [--public-url <url>]
  codetwo-server pair [--data-dir <path>]

Commands:
  webui            Serve the shared React UI and open its one-time pairing link.
  serve            Headless daemon for a server: no browser, one owner per data directory,
                   binds 127.0.0.1 unless --host says otherwise. Another C2 (desktop, browser,
                   phone) pairs with it and shares its sessions.
  pair             Ask the running daemon that owns --data-dir for a fresh one-time pairing link.

Options:
  --ui-dir <path>  Vite Web build directory. Defaults to CODETWO_WEB_UI_DIR or
                   a web-ui directory next to the executable.
  --data-dir <path>
                   Standalone C2 data directory. Defaults to CODETWO_DATA_DIR or ~/.codetwo.
  --no-open        Print the pairing link without opening a browser.
  --host <addr>    serve: bind address. Defaults to CODETWO_HOST or 127.0.0.1.
  --port <port>    serve: port. Defaults to CODETWO_PORT or 4599.
  --public-url <url>
                   serve: address clients should use (reverse proxy / TLS / tailnet name).
                   Defaults to CODETWO_PUBLIC_URL, else derived from the bind address.
  -h, --help       Show this help.

Network environment:
  CODETWO_HOST, CODETWO_PORT, CODETWO_PAIR_TTL
"#;

#[derive(Debug, PartialEq, Eq)]
enum Surface {
    Compact,
    WebUi,
    Serve,
}

#[derive(Debug, PartialEq, Eq)]
struct Cli {
    surface: Surface,
    ui_dir: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    open_browser: bool,
    host: Option<String>,
    port: Option<u16>,
    public_url: Option<String>,
}

impl Cli {
    fn new(surface: Surface) -> Self {
        Self {
            open_browser: surface == Surface::WebUi,
            surface,
            ui_dir: None,
            data_dir: None,
            host: None,
            port: None,
            public_url: None,
        }
    }
}

enum Parsed {
    Run(Cli),
    Pair { data_dir: Option<PathBuf> },
    Help,
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut arguments = arguments.into_iter();
    let Some(first) = arguments.next() else {
        return Ok(Parsed::Run(Cli::new(Surface::Compact)));
    };
    if first == "--help" || first == "-h" {
        if arguments.next().is_some() {
            return Err("--help does not accept additional arguments".into());
        }
        return Ok(Parsed::Help);
    }
    let surface = match first.as_str() {
        "webui" => Some(Surface::WebUi),
        "serve" => Some(Surface::Serve),
        "pair" => None,
        _ => return Err(format!("unknown command: {first}\n\n{HELP}")),
    };

    let mut cli = Cli::new(surface.unwrap_or(Surface::Compact));
    let command = first.as_str();
    while let Some(argument) = arguments.next() {
        let mut value = |flag: &str| {
            arguments
                .next()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match argument.as_str() {
            "--help" | "-h" => return Ok(Parsed::Help),
            "--data-dir" => cli.data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--ui-dir" if command != "pair" => {
                cli.ui_dir = Some(PathBuf::from(value("--ui-dir")?));
            }
            "--no-open" if command == "webui" => cli.open_browser = false,
            "--host" if command == "serve" => cli.host = Some(value("--host")?),
            "--port" if command == "serve" => {
                cli.port = Some(
                    value("--port")?
                        .parse()
                        .map_err(|_| "--port must be an integer from 0 to 65535".to_string())?,
                );
            }
            "--public-url" if command == "serve" => cli.public_url = Some(value("--public-url")?),
            _ => return Err(format!("unknown {command} option: {argument}\n\n{HELP}")),
        }
    }
    if command == "pair" {
        return Ok(Parsed::Pair {
            data_dir: cli.data_dir,
        });
    }
    Ok(Parsed::Run(cli))
}

fn default_data_dir() -> PathBuf {
    let home = codetwo_core::provider::home_dir().unwrap_or_else(std::env::temp_dir);
    home.join(".codetwo")
}

fn resolve_data_dir(explicit: Option<PathBuf>) -> PathBuf {
    explicit
        .or_else(|| std::env::var_os("CODETWO_DATA_DIR").map(PathBuf::from))
        .unwrap_or_else(default_data_dir)
}

fn resolve_ui_dir(
    explicit: Option<PathBuf>,
    configured: Option<PathBuf>,
    executable: &Path,
) -> Result<PathBuf, String> {
    let candidate = explicit
        .or(configured)
        .or_else(|| executable.parent().map(|parent| parent.join("web-ui")))
        .ok_or_else(|| "cannot resolve the C2 Web UI directory".to_string())?;
    if !candidate.join("index.html").is_file() {
        return Err(format!(
            "C2 Web UI assets are missing at {}. Run ./script/build/hosts.sh release or pass --ui-dir <path>.",
            candidate.display()
        ));
    }
    candidate.canonicalize().map_err(|error| {
        format!(
            "cannot resolve C2 Web UI assets at {}: {error}",
            candidate.display()
        )
    })
}

fn local_pairing_url(port: u16, pairing_token: &str) -> String {
    let endpoints = pairing_endpoints(port);
    let endpoint = endpoints
        .iter()
        .find(|endpoint| endpoint.id == "loopback")
        .or_else(|| endpoints.first())
        .expect("pairing endpoints include loopback");
    pairing_url_for_endpoint(&endpoint.url, pairing_token)
}

fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command.spawn().map(|_| ())
}

/// `serve` always exposes the command route; a UI directory is optional because a desktop or
/// phone client may bring its own renderer. An explicit `--ui-dir` that is missing is an error.
fn serve_ui_dir(cli_dir: Option<PathBuf>) -> Result<Option<PathBuf>, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot resolve codetwo-server executable: {error}"))?;
    let configured = std::env::var_os("CODETWO_WEB_UI_DIR").map(PathBuf::from);
    let explicit = cli_dir.is_some() || configured.is_some();
    match resolve_ui_dir(cli_dir, configured, &executable) {
        Ok(dir) => Ok(Some(dir)),
        Err(error) if explicit => Err(error),
        Err(_) => Ok(None),
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

async fn run(cli: Cli) -> Result<(), String> {
    let serve = cli.surface == Surface::Serve;
    let web_ui_dir = match cli.surface {
        Surface::Compact => None,
        Surface::Serve => serve_ui_dir(cli.ui_dir)?,
        Surface::WebUi => {
            let executable = std::env::current_exe()
                .map_err(|error| format!("cannot resolve codetwo-server executable: {error}"))?;
            Some(resolve_ui_dir(
                cli.ui_dir,
                std::env::var_os("CODETWO_WEB_UI_DIR").map(PathBuf::from),
                &executable,
            )?)
        }
    };

    // A headless daemon is reachable only where the operator says so; the interactive launchers
    // keep their historical LAN-wide default.
    let default_host = if serve { "127.0.0.1" } else { "0.0.0.0" };
    let host = cli
        .host
        .or_else(|| std::env::var("CODETWO_HOST").ok())
        .unwrap_or_else(|| default_host.into());
    let port: u16 = cli
        .port
        .or_else(|| {
            std::env::var("CODETWO_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(4599);
    let pair_ttl = std::env::var("CODETWO_PAIR_TTL")
        .ok()
        .and_then(|value| value.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_PAIRING_TTL);

    let data_dir = resolve_data_dir(cli.data_dir);
    std::fs::create_dir_all(&data_dir).map_err(|error| {
        format!(
            "cannot create data directory {}: {error}",
            data_dir.display()
        )
    })?;
    // One data directory, one live Core owner. Held until the process exits.
    let _instance = if serve {
        Some(InstanceLock::acquire(&data_dir)?)
    } else {
        None
    };
    let core = Arc::new(
        CoreApp::boot(AppConfig::new(&data_dir))
            .await
            .map_err(|error| error.to_string())?,
    );
    let engine = core
        .service::<EngineService>()
        .ok_or_else(|| "engine plugin did not load".to_string())?
        .0
        .clone();
    let store = core
        .service::<StoreService>()
        .ok_or_else(|| "store plugin did not load".to_string())?
        .0
        .clone();
    let events = core
        .service::<EventBus>()
        .ok_or_else(|| "bus plugin did not load".to_string())?
        .0
        .clone();
    let canvas_gate = core
        .service::<CanvasService>()
        .ok_or_else(|| "canvas service did not load".to_string())?
        .gate;

    let auth = Arc::new(AuthState::load(Some(data_dir.join("remote-devices.json"))));
    let pairing_token = auth.issue_pairing_token(pair_ttl);
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|error| format!("invalid C2 server address {host}:{port}: {error}"))?;

    let (local, mut handle) = if serve || web_ui_dir.is_some() {
        let plugin_manager = core
            .service::<PluginManager>()
            .ok_or_else(|| "plugin manager did not load".to_string())?;
        bind_and_serve_with_web_ui(
            engine,
            events,
            addr,
            auth.clone(),
            store,
            canvas_gate,
            None,
            Some(Arc::new(KernelWebUiCommands::new(plugin_manager))),
            web_ui_dir,
        )
        .await
        .map_err(|error| error.to_string())?
    } else {
        bind_and_serve_with_canvas(engine, events, addr, auth.clone(), store, canvas_gate)
            .await
            .map_err(|error| error.to_string())?
    };

    let paired = auth.list_devices().len();
    if serve {
        let public_url = cli
            .public_url
            .or_else(|| std::env::var("CODETWO_PUBLIC_URL").ok());
        let (base, shareable) = pairing_base_url(&host, local.port(), public_url.as_deref());
        println!("\n  C2 server is live on {local}");
        println!("  data directory: {}", data_dir.display());
        print_pairing_link(
            &pairing_url_for_endpoint(&base, &pairing_token),
            shareable,
            pair_ttl,
        );
        if paired > 0 {
            println!("  {paired} previously paired device(s) can reconnect without a new link.");
        }
        println!(
            "  More devices: run `codetwo-server pair --data-dir {}`\n",
            data_dir.display()
        );
        #[cfg(unix)]
        {
            let auth = auth.clone();
            let data_dir = data_dir.clone();
            tokio::spawn(async move {
                let Ok(mut requests) =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined1())
                else {
                    return;
                };
                while requests.recv().await.is_some() {
                    let token = auth.issue_pairing_token(pair_ttl);
                    let link = pairing_url_for_endpoint(&base, &token);
                    if let Err(error) = write_pairing_file(&data_dir, &link) {
                        eprintln!("codetwo-server: {error}");
                    }
                }
            });
        }
    } else {
        print_pairing(local.port(), &pairing_token);
        if paired > 0 {
            println!("  {paired} previously paired device(s) can reconnect without a new link.\n");
        }
        println!("  listening on {local}");
        println!("  data directory: {}\n", data_dir.display());

        if cli.surface == Surface::WebUi && cli.open_browser {
            let url = local_pairing_url(local.port(), &pairing_token);
            if let Err(error) = open_browser(&url) {
                eprintln!("  could not open the browser: {error}");
                eprintln!("  open this URL manually: {url}\n");
            }
        }
    }

    if serve {
        tokio::select! {
            _ = &mut handle => {}
            _ = shutdown_signal() => handle.abort(),
        }
        // Stop providers and flush the store before the data directory is released.
        if let Ok(core) = Arc::try_unwrap(core) {
            core.stop().await;
        }
    } else {
        let _core = core;
        let _ = handle.await;
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    if std::env::args().any(|argument| argument == "--codetwo-assistant-mcp") {
        if let Err(error) = codetwo_core::assistant_bridge::run_stdio() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }

    let parsed = match parse_args(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("codetwo-server: {error}");
            std::process::exit(2);
        }
    };
    match parsed {
        Parsed::Help => print!("{HELP}"),
        Parsed::Pair { data_dir } => {
            match request_pairing_link(&resolve_data_dir(data_dir), Duration::from_secs(5)) {
                Ok(link) => println!("{link}"),
                Err(error) => {
                    eprintln!("codetwo-server: {error}");
                    std::process::exit(1);
                }
            }
        }
        Parsed::Run(cli) => {
            if let Err(error) = run(cli).await {
                eprintln!("codetwo-server: {error}");
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_args, resolve_ui_dir, Cli, Parsed, Surface};
    use std::path::PathBuf;

    fn parsed(arguments: &[&str]) -> Cli {
        match parse_args(arguments.iter().map(|argument| argument.to_string())).unwrap() {
            Parsed::Run(cli) => cli,
            _ => panic!("expected runnable CLI"),
        }
    }

    #[test]
    fn no_arguments_preserve_the_compact_remote() {
        assert_eq!(parsed(&[]), Cli::new(Surface::Compact));
    }

    #[test]
    fn webui_accepts_only_its_small_launch_interface() {
        assert_eq!(
            parsed(&[
                "webui",
                "--ui-dir",
                "/tmp/ui",
                "--data-dir",
                "/tmp/data",
                "--no-open",
            ]),
            Cli {
                ui_dir: Some(PathBuf::from("/tmp/ui")),
                data_dir: Some(PathBuf::from("/tmp/data")),
                open_browser: false,
                ..Cli::new(Surface::WebUi)
            }
        );
        assert!(parse_args(["webui".into(), "--ui-dir".into()]).is_err());
        assert!(parse_args(["unknown".into()]).is_err());
        assert!(parse_args(["webui".into(), "--host".into(), "::1".into()]).is_err());
    }

    #[test]
    fn serve_is_headless_and_takes_network_options() {
        let cli = parsed(&[
            "serve",
            "--host",
            "100.64.0.9",
            "--port",
            "0",
            "--data-dir",
            "/srv/c2",
            "--public-url",
            "https://c2.example.com",
        ]);
        assert_eq!(
            cli,
            Cli {
                host: Some("100.64.0.9".into()),
                port: Some(0),
                data_dir: Some(PathBuf::from("/srv/c2")),
                public_url: Some("https://c2.example.com".into()),
                ..Cli::new(Surface::Serve)
            }
        );
        assert!(!cli.open_browser);
        assert!(parse_args(["serve".into(), "--no-open".into()]).is_err());
        assert!(parse_args(["serve".into(), "--port".into(), "http".into()]).is_err());
    }

    #[test]
    fn pair_only_needs_the_data_directory() {
        match parse_args(["pair".into(), "--data-dir".into(), "/srv/c2".into()]).unwrap() {
            Parsed::Pair { data_dir } => assert_eq!(data_dir, Some(PathBuf::from("/srv/c2"))),
            _ => panic!("expected pair"),
        }
        assert!(parse_args(["pair".into(), "--port".into(), "1".into()]).is_err());
    }

    #[test]
    fn webui_assets_resolve_explicit_then_configured_then_adjacent() {
        let root = tempfile::tempdir().unwrap();
        let explicit = root.path().join("explicit");
        let configured = root.path().join("configured");
        let adjacent = root.path().join("web-ui");
        for directory in [&explicit, &configured, &adjacent] {
            std::fs::create_dir_all(directory).unwrap();
            std::fs::write(directory.join("index.html"), "ok").unwrap();
        }
        let executable = root.path().join("codetwo-server");

        assert_eq!(
            resolve_ui_dir(
                Some(explicit.clone()),
                Some(configured.clone()),
                &executable
            )
            .unwrap(),
            explicit.canonicalize().unwrap()
        );
        assert_eq!(
            resolve_ui_dir(None, Some(configured.clone()), &executable).unwrap(),
            configured.canonicalize().unwrap()
        );
        assert_eq!(
            resolve_ui_dir(None, None, &executable).unwrap(),
            adjacent.canonicalize().unwrap()
        );
        assert!(
            resolve_ui_dir(Some(root.path().join("missing")), None, &executable)
                .unwrap_err()
                .contains("--ui-dir")
        );
    }
}

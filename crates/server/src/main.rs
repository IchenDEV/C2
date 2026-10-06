//! `codetwo-server` — run one C2 Core with either the compact remote or the full React Web UI.
//!
//! Env: `CODETWO_HOST` (default 0.0.0.0, or 127.0.0.1 for `serve`), `CODETWO_PORT` (default 4599), `CODETWO_PAIR_TTL`
//! (pairing-token lifetime in seconds, default 900), `CODETWO_DATA_DIR`, and
//! `CODETWO_WEB_UI_DIR`. `serve` is the headless daemon for a remote machine, and `pair` asks a
//! running daemon for a fresh pairing link.

use std::path::PathBuf;
use std::time::Duration;

use codetwo_server::daemon::request_pairing_link;
use codetwo_server::serve::{resolve_data_dir, ServeConfig, ServeOptions, ServeSurface};

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

/// Every launcher goes through `codetwo_server::serve`, the one Core boot path shared with
/// `codetwo serve`; this binary only maps its commands onto that configuration.
async fn run(cli: Cli) -> Result<(), String> {
    let serve = cli.surface == Surface::Serve;
    // A headless daemon is reachable only where the operator says so; the interactive launchers
    // keep advertising a LAN-reachable pairing link.
    let host = cli
        .host
        .or_else(|| std::env::var("CODETWO_HOST").ok())
        .unwrap_or_else(|| if serve { "127.0.0.1" } else { "0.0.0.0" }.into());
    let port: u16 = cli
        .port
        .or_else(|| {
            std::env::var("CODETWO_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(4599);
    let surface = match cli.surface {
        Surface::Compact => ServeSurface::Compact,
        Surface::WebUi => ServeSurface::WebUi,
        Surface::Serve => ServeSurface::Daemon,
    };
    codetwo_server::serve::run_with(
        ServeConfig {
            surface,
            ui_dir: cli.ui_dir,
            data_dir: cli.data_dir,
            open_browser: cli.open_browser,
            host,
            port,
            external_mcp: false,
            // No tunnel from this binary: remote access is `codetwo serve --remote ...` only.
            remote_args: Vec::new(),
        },
        ServeOptions {
            instance_lock: serve,
            public_url: cli.public_url,
        },
    )
    .await
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
    use super::{parse_args, Cli, Parsed, Surface};
    use codetwo_server::serve::resolve_ui_dir;
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

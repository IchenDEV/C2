//! `codetwo-server` — run one C2 Core with either the compact remote or the full React Web UI.
//!
//! Env: `CODETWO_HOST` (default 0.0.0.0), `CODETWO_PORT` (default 4599), `CODETWO_PAIR_TTL`
//! (pairing-token lifetime in seconds, default 900), `CODETWO_DATA_DIR`, and
//! `CODETWO_WEB_UI_DIR`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use codetwo_core::plugins::{
    AppConfig, CanvasService, CoreApp, EngineService, EventBus, PluginManager, StoreService,
};
use codetwo_server::{
    bind_and_serve_with_canvas, bind_and_serve_with_web_ui, pairing_endpoints,
    pairing_url_for_endpoint, print_pairing, AuthState, KernelWebUiCommands, DEFAULT_PAIRING_TTL,
};

const HELP: &str = r#"Usage:
  codetwo-server
  codetwo-server webui [--ui-dir <path>] [--data-dir <path>] [--no-open]

Commands:
  webui            Serve the shared React UI and open its one-time pairing link.

Options:
  --ui-dir <path>  Vite Web build directory. Defaults to CODETWO_WEB_UI_DIR or
                   a web-ui directory next to the executable.
  --data-dir <path>
                   Standalone C2 data directory. Defaults to CODETWO_DATA_DIR or ~/.codetwo.
  --no-open        Print the pairing link without opening a browser.
  -h, --help       Show this help.

Network environment:
  CODETWO_HOST, CODETWO_PORT, CODETWO_PAIR_TTL
"#;

#[derive(Debug, PartialEq, Eq)]
enum Surface {
    Compact,
    WebUi,
}

#[derive(Debug, PartialEq, Eq)]
struct Cli {
    surface: Surface,
    ui_dir: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    open_browser: bool,
}

enum Parsed {
    Run(Cli),
    Help,
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut arguments = arguments.into_iter();
    let Some(first) = arguments.next() else {
        return Ok(Parsed::Run(Cli {
            surface: Surface::Compact,
            ui_dir: None,
            data_dir: None,
            open_browser: false,
        }));
    };
    if first == "--help" || first == "-h" {
        if arguments.next().is_some() {
            return Err("--help does not accept additional arguments".into());
        }
        return Ok(Parsed::Help);
    }
    if first != "webui" {
        return Err(format!("unknown command: {first}\n\n{HELP}"));
    }

    let mut cli = Cli {
        surface: Surface::WebUi,
        ui_dir: None,
        data_dir: None,
        open_browser: true,
    };
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--ui-dir" => {
                let path = arguments
                    .next()
                    .ok_or_else(|| "--ui-dir requires a path".to_string())?;
                cli.ui_dir = Some(PathBuf::from(path));
            }
            "--data-dir" => {
                let path = arguments
                    .next()
                    .ok_or_else(|| "--data-dir requires a path".to_string())?;
                cli.data_dir = Some(PathBuf::from(path));
            }
            "--no-open" => cli.open_browser = false,
            "--help" | "-h" => return Ok(Parsed::Help),
            _ => return Err(format!("unknown webui option: {argument}\n\n{HELP}")),
        }
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

async fn run(cli: Cli) -> Result<(), String> {
    let web_ui_dir = if cli.surface == Surface::WebUi {
        let executable = std::env::current_exe()
            .map_err(|error| format!("cannot resolve codetwo-server executable: {error}"))?;
        Some(resolve_ui_dir(
            cli.ui_dir,
            std::env::var_os("CODETWO_WEB_UI_DIR").map(PathBuf::from),
            &executable,
        )?)
    } else {
        None
    };

    let host = std::env::var("CODETWO_HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = std::env::var("CODETWO_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
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

    let (local, handle) = if let Some(web_ui_dir) = web_ui_dir {
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
            Some(web_ui_dir),
        )
        .await
        .map_err(|error| error.to_string())?
    } else {
        bind_and_serve_with_canvas(engine, events, addr, auth.clone(), store, canvas_gate)
            .await
            .map_err(|error| error.to_string())?
    };

    print_pairing(local.port(), &pairing_token);
    let paired = auth.list_devices().len();
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

    let _core = core;
    let _ = handle.await;
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
            Parsed::Help => panic!("expected runnable CLI"),
        }
    }

    #[test]
    fn no_arguments_preserve_the_compact_remote() {
        assert_eq!(
            parsed(&[]),
            Cli {
                surface: Surface::Compact,
                ui_dir: None,
                data_dir: None,
                open_browser: false,
            }
        );
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
                surface: Surface::WebUi,
                ui_dir: Some(PathBuf::from("/tmp/ui")),
                data_dir: Some(PathBuf::from("/tmp/data")),
                open_browser: false,
            }
        );
        assert!(parse_args(["webui".into(), "--ui-dir".into()]).is_err());
        assert!(parse_args(["unknown".into()]).is_err());
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

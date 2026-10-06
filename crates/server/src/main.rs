//! `codetwo-server` — run one C2 Core with either the compact remote or the full React Web UI.
//!
//! Env: `CODETWO_HOST` (default 0.0.0.0), `CODETWO_PORT` (default 4599), `CODETWO_PAIR_TTL`
//! (pairing-token lifetime in seconds, default 900), `CODETWO_DATA_DIR`, and
//! `CODETWO_WEB_UI_DIR`.

use std::path::PathBuf;

use codetwo_server::serve::{ServeConfig, ServeSurface};

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
struct Cli {
    surface: ServeSurface,
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
            surface: ServeSurface::Compact,
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
        surface: ServeSurface::WebUi,
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

async fn run(cli: Cli) -> Result<(), String> {
    let host = std::env::var("CODETWO_HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = std::env::var("CODETWO_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(4599);
    let remote_args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = codetwo_server::remote::RemoteConfig::from_env_and_args(remote_args.clone())
    {
        eprintln!("codetwo-server: remote configuration error: {error}");
        std::process::exit(2);
    }
    codetwo_server::serve::run(ServeConfig {
        surface: cli.surface,
        ui_dir: cli.ui_dir,
        data_dir: cli.data_dir,
        open_browser: cli.open_browser,
        host,
        port,
        external_mcp: false,
        remote_args,
    })
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
    use super::{parse_args, Cli, Parsed, ServeSurface};
    use codetwo_server::serve::resolve_ui_dir;
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
                surface: ServeSurface::Compact,
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
                surface: ServeSurface::WebUi,
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

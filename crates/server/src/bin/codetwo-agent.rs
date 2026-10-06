//! Native C2 remote programming agent.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use codetwo_core::plugins::{AppConfig, CanvasService, CoreApp, EngineService, EventBus, StoreService};
use codetwo_server::{
    bind_and_serve_with_canvas, pairing_endpoints, pairing_qr_svg, pairing_url_for_endpoint,
    select_pairing_endpoint, AuthState, PairingEndpoint,
};
use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum Protocol {
    T3,
    Legacy,
}

struct Options {
    workspace: PathBuf,
    data_dir: PathBuf,
    port: u16,
    pair: bool,
    protocol: Protocol,
    pairing_ttl: u64,
    json: bool,
}

#[derive(Serialize)]
struct PairingDetails {
    endpoint_id: String,
    url: String,
    token: String,
    expires_in: u64,
    qr_svg: String,
}

#[derive(Serialize)]
struct Startup<'a> {
    kind: &'static str,
    pid: u32,
    workspace: &'a Path,
    data_dir: &'a Path,
    port: u16,
    endpoints: &'a [PairingEndpoint],
    protocol: Protocol,
    pairing: Option<PairingDetails>,
}

fn usage() -> &'static str {
    "C2 remote programming agent\n\n\
Usage: codetwo-agent [options] [workspace]\n\n\
Options:\n\
  --data-dir <path>       Durable agent state (default: ~/.codetwo-agent)\n\
  --port <port>           Listen port (default: 4599; 0 chooses a free port)\n\
  --protocol <t3|legacy>  Pairing client (default: t3)\n\
  --pair-ttl <seconds>    One-time pairing lifetime (default: 900)\n\
  --no-pair               Start without printing a pairing credential\n\
  --json                  Emit startup details as one JSON object\n\
  -h, --help              Show this help"
}

fn absolute(path: PathBuf, base: &Path) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn required_value(args: &[String], index: usize, flag: &str) -> Result<String, String> {
    args.get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn positive_integer(value: &str, flag: &str, allow_zero: bool) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("{flag} must be an integer"))?;
    if !allow_zero && parsed == 0 {
        return Err(format!("{flag} must be a positive integer"));
    }
    Ok(parsed)
}

fn parse_args(args: Vec<String>) -> Result<Option<Options>, String> {
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    let default_data_dir = std::env::var_os("CODETWO_AGENT_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| codetwo_core::provider::home_dir().map(|home| home.join(".codetwo-agent")))
        .unwrap_or_else(|| std::env::temp_dir().join("codetwo-agent"));
    let mut workspace = current.clone();
    let mut data_dir = default_data_dir;
    let mut port = 4599_u16;
    let mut pair = true;
    let mut protocol = Protocol::T3;
    let mut pairing_ttl = 900_u64;
    let mut json = false;
    let mut workspace_seen = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-h" | "--help" => return Ok(None),
            "--no-pair" => pair = false,
            "--json" => json = true,
            "--data-dir" => {
                data_dir = PathBuf::from(required_value(&args, index, "--data-dir")?);
                index += 1;
            }
            "--port" => {
                let value = required_value(&args, index, "--port")?;
                let parsed = positive_integer(&value, "--port", true)?;
                port = u16::try_from(parsed)
                    .map_err(|_| "--port must not exceed 65535".to_string())?;
                index += 1;
            }
            "--pair-ttl" => {
                let value = required_value(&args, index, "--pair-ttl")?;
                pairing_ttl = positive_integer(&value, "--pair-ttl", false)?;
                index += 1;
            }
            "--protocol" => {
                protocol = match required_value(&args, index, "--protocol")?.as_str() {
                    "t3" => Protocol::T3,
                    "legacy" => Protocol::Legacy,
                    _ => return Err("--protocol must be t3 or legacy".into()),
                };
                index += 1;
            }
            argument if argument.starts_with('-') => {
                return Err(format!("unknown option: {argument}"));
            }
            argument if workspace_seen => {
                return Err(format!(
                    "only one workspace may be supplied (unexpected {argument})"
                ));
            }
            argument => {
                workspace = PathBuf::from(argument);
                workspace_seen = true;
            }
        }
        index += 1;
    }

    let workspace = absolute(workspace, &current)
        .canonicalize()
        .map_err(|error| format!("could not open workspace: {error}"))?;
    if !workspace.is_dir() {
        return Err("workspace is not a directory".into());
    }
    let data_dir = absolute(data_dir, &current);
    Ok(Some(Options {
        workspace,
        data_dir,
        port,
        pair,
        protocol,
        pairing_ttl,
        json,
    }))
}

fn pairing_details(
    auth: &AuthState,
    endpoints: &[PairingEndpoint],
    protocol: Protocol,
    ttl: u64,
) -> Result<PairingDetails, String> {
    let endpoint = select_pairing_endpoint(endpoints, None)?;
    let token = match protocol {
        Protocol::T3 => auth.issue_t3_pairing_token(Duration::from_secs(ttl)),
        Protocol::Legacy => auth.issue_pairing_token(Duration::from_secs(ttl)),
    };
    let url = pairing_url_for_endpoint(&endpoint.url, &token);
    let qr_svg = endpoint
        .qr_shareable
        .then(|| pairing_qr_svg(&url))
        .flatten()
        .unwrap_or_default();
    Ok(PairingDetails {
        endpoint_id: endpoint.id.clone(),
        url,
        token,
        expires_in: ttl,
        qr_svg,
    })
}

async fn shutdown_signal() -> Result<(), String> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .map_err(|error| error.to_string())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.map_err(|error| error.to_string()),
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .map_err(|error| error.to_string())
    }
}

fn wire_external_mcp(engine: &codetwo_core::Engine, data_dir: &Path) {
    use codetwo_core::external_mcp::state::EXTERNAL_MCP_ENV;
    if !std::env::var(EXTERNAL_MCP_ENV)
        .ok()
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
    {
        return;
    }
    engine.external_mcp_state().set_enabled(true);
    match engine.external_mcp_state().configure_data_dir(data_dir) {
        Ok(()) => eprintln!("external MCP enabled (POST /external-mcp on this server)"),
        Err(error) => eprintln!("external MCP not started: {error}"),
    }
}

async fn run() -> Result<(), String> {
    let Some(options) = parse_args(std::env::args().skip(1).collect())? else {
        println!("{}", usage());
        return Ok(());
    };
    std::fs::create_dir_all(&options.data_dir)
        .map_err(|error| format!("could not create agent data directory: {error}"))?;
    std::env::set_current_dir(&options.workspace)
        .map_err(|error| format!("could not enter workspace: {error}"))?;
    codetwo_core::provider::augment_search_path();

    let core = CoreApp::boot(AppConfig::new(&options.data_dir))
        .await
        .map_err(|error| error.to_string())?;
    let engine = core
        .service::<EngineService>()
        .ok_or_else(|| "engine plugin did not load".to_string())?
        .0
        .clone();
    wire_external_mcp(&engine, &options.data_dir);
    let store = core
        .service::<StoreService>()
        .ok_or_else(|| "store plugin did not load".to_string())?
        .0
        .clone();
    let events = core
        .service::<EventBus>()
        .ok_or_else(|| "event bus plugin did not load".to_string())?
        .0
        .clone();
    let canvas_gate = core
        .service::<CanvasService>()
        .ok_or_else(|| "canvas plugin did not load".to_string())?
        .gate;
    let auth = Arc::new(AuthState::load(Some(
        options.data_dir.join("remote-devices.json"),
    )));
    let address: SocketAddr = format!("0.0.0.0:{}", options.port)
        .parse()
        .map_err(|error| format!("invalid listen address: {error}"))?;
    let (local, mut server) =
        bind_and_serve_with_canvas(engine, events, address, auth.clone(), store, canvas_gate)
            .await
            .map_err(|error| error.to_string())?;
    let endpoints = pairing_endpoints(local.port());
    let pairing = options
        .pair
        .then(|| pairing_details(&auth, &endpoints, options.protocol, options.pairing_ttl))
        .transpose()?;
    let startup = Startup {
        kind: "codetwo-agent-ready",
        pid: std::process::id(),
        workspace: &options.workspace,
        data_dir: &options.data_dir,
        port: local.port(),
        endpoints: &endpoints,
        protocol: options.protocol,
        pairing,
    };
    if options.json {
        println!(
            "{}",
            serde_json::to_string(&startup).map_err(|error| error.to_string())?
        );
    } else {
        println!(
            "C2 remote programming agent is listening on port {}",
            startup.port
        );
        for endpoint in startup.endpoints {
            println!("  {}: {}", endpoint.label, endpoint.url);
        }
        if let Some(pairing) = &startup.pairing {
            println!("Pairing URL: {}", pairing.url);
        }
    }

    tokio::select! {
        result = &mut server => {
            result.map_err(|error| format!("remote server task failed: {error}"))?;
        }
        result = shutdown_signal() => {
            result?;
            server.abort();
        }
    }
    core.stop().await;
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_cli_contract() {
        let parsed = parse_args(vec![
            "--port".into(),
            "0".into(),
            "--protocol".into(),
            "legacy".into(),
            "--no-pair".into(),
            ".".into(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(parsed.port, 0);
        assert!(!parsed.pair);
        assert!(matches!(parsed.protocol, Protocol::Legacy));
    }
}

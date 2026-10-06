//! Unified headless CLI: `codetwo serve` and external-MCP client commands.
//!
//! Exit codes: 0 ok, 1 error, 2 timeout, 3 needs attention (approval or question pending).

use std::path::PathBuf;
use std::process;

use codetwo_server::cli::{
    admin::{self, McpClientAction},
    client::{EventsOpts, ProjectAction, SendOpts, SessionAction, WaitOpts},
    config::GlobalOpts,
    output::{CliOutcome, ExitCode},
    run_answer, run_approve, run_events, run_mcp_client, run_pair, run_pending, run_project,
    run_send, run_session, run_status, run_stop, run_wait, PairSource,
};
use codetwo_server::serve::{ServeConfig, ServeSurface};

const HELP: &str = r#"Usage:
  codetwo serve [options]
  codetwo status|pending ...
  codetwo events [--since CURSOR] [--follow]
  codetwo session list|read ID|create PROJECT [--provider P]
  codetwo send ID [--mode prompt|queue|steer] TEXT...   (TEXT `-` reads stdin)
  codetwo wait ID [--timeout SECS]
  codetwo stop ID
  codetwo approve REQUEST_ID [--deny]
  codetwo answer REQUEST_ID TEXT...
  codetwo project list|create PATH
  codetwo mcp client create --name N [--scopes read,operate,approve,admin]
                            [--projects all|P1,P2] [--ttl 30d|<N>d|permanent]
                            [--token-out FILE]
  codetwo mcp client list|revoke ID
  codetwo pair --url URL (--token-file FILE | --stdin)
                            Import an external MCP credential (ctmcp_...) for a server and
                            save it as <data-dir>/cli.token (0600) after a read-only check.
                            Web UI device/session tokens are not accepted.

Global options (client commands):
  --json           Emit one JSON object on stdout
  --url URL        Server base URL (or CODETWO_URL, or <data-dir>/server.json)
  --token-file F   External MCP credential file (0600)
  --data-dir P     Data directory (or CODETWO_DATA_DIR)

Server URL: --url, CODETWO_URL, <data-dir>/cli.url (from `pair`), then <data-dir>/server.json.
Credentials: CODETWO_TOKEN or <data-dir>/cli.token (0600). Never pass tokens on argv.
Plain http is only sent to loopback; remote servers need https.
Exit codes: 0 ok, 1 error, 2 timeout, 3 needs attention (approval or question pending).

Run `codetwo serve --help` for server flags.
"#;

const SERVE_HELP: &str = r#"Usage: codetwo serve [webui] [options]

Options:
  --data-dir P
  --bind ADDR:PORT   Default 127.0.0.1:4599 (also --port N, or CODETWO_PORT)
  --external-mcp     Enable POST /external-mcp (or CODETWO_EXTERNAL_MCP=1)
  webui              Full React UI (--ui-dir P, --no-open)
  --remote cloudflared --remote-host H --tunnel-token-file F
  --cloudflared-path P --allow-quick-tunnel
"#;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print!("{HELP}");
        return;
    }
    let exit = match args[0].as_str() {
        "serve" => run_serve_command(&args[1..]),
        other => run_client_command(other, &args[1..]),
    };
    process::exit(exit.as_i32());
}

// ---------------------------------------------------------------- serve

fn run_serve_command(args: &[String]) -> ExitCode {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{SERVE_HELP}");
        return ExitCode::Ok;
    }
    let config = match build_serve_config(args) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("codetwo serve: {message}");
            return ExitCode::Error;
        }
    };
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    match runtime.block_on(codetwo_server::serve::run(config)) {
        Ok(()) => ExitCode::Ok,
        Err(error) => {
            eprintln!("codetwo serve: {error}");
            ExitCode::Error
        }
    }
}

fn build_serve_config(args: &[String]) -> Result<ServeConfig, String> {
    let mut surface = ServeSurface::Compact;
    let mut ui_dir = None;
    let mut data_dir = None;
    let mut open_browser = true;
    let mut host = "127.0.0.1".to_string();
    let mut port: u16 = std::env::var("CODETWO_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4599);
    // The shared serve path also honors CODETWO_EXTERNAL_MCP.
    let mut external_mcp = false;
    let mut remote_args: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        i += 1;
        let value = |i: &mut usize| -> Result<String, String> {
            let v = args
                .get(*i)
                .cloned()
                .ok_or_else(|| format!("{flag} requires a value"))?;
            *i += 1;
            Ok(v)
        };
        match flag {
            "webui" => surface = ServeSurface::WebUi,
            "--data-dir" => data_dir = Some(PathBuf::from(value(&mut i)?)),
            "--ui-dir" => ui_dir = Some(PathBuf::from(value(&mut i)?)),
            "--bind" => parse_bind(&value(&mut i)?, &mut host, &mut port)?,
            "--port" | "-p" => {
                port = value(&mut i)?
                    .parse()
                    .map_err(|_| format!("invalid port for {flag}"))?;
            }
            "--external-mcp" => external_mcp = true,
            "--no-open" => open_browser = false,
            "--allow-quick-tunnel" => remote_args.push(flag.to_string()),
            "--remote" | "--remote-host" | "--tunnel-token-file" | "--cloudflared-path" => {
                remote_args.push(flag.to_string());
                remote_args.push(value(&mut i)?);
            }
            unknown => return Err(format!("unknown option: {unknown}")),
        }
    }
    Ok(ServeConfig {
        surface,
        ui_dir,
        data_dir,
        open_browser,
        host,
        port,
        external_mcp,
        remote_args,
    })
}

fn parse_bind(bind: &str, host: &mut String, port: &mut u16) -> Result<(), String> {
    let (h, p) = bind.rsplit_once(':').unwrap_or(("", bind));
    if !h.is_empty() {
        *host = h.to_string();
    }
    *port = p.parse().map_err(|_| format!("invalid --bind: {bind}"))?;
    Ok(())
}

// --------------------------------------------------------------- client

fn run_client_command(top: &str, args: &[String]) -> ExitCode {
    // Honor --json for argument errors too.
    let json = args.iter().any(|a| a == "--json");
    let outcome = match parse_globals(args) {
        Ok((globals, _)) if globals.help => {
            print!("{HELP}");
            return ExitCode::Ok;
        }
        Ok((globals, rest)) => {
            let opts = GlobalOpts {
                json: globals.json,
                url: globals.url,
                token_file: globals.token_file,
                data_dir: globals.data_dir,
            };
            return dispatch_client(top, rest, &opts).emit(opts.json);
        }
        Err(outcome) => outcome,
    };
    outcome.emit(json)
}

#[derive(Default)]
struct ParsedGlobals {
    json: bool,
    url: Option<String>,
    token_file: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    help: bool,
}

fn parse_globals(args: &[String]) -> Result<(ParsedGlobals, Vec<String>), CliOutcome> {
    let mut globals = ParsedGlobals::default();
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        i += 1;
        let value = |i: &mut usize| -> Result<String, CliOutcome> {
            let v = args.get(*i).cloned().ok_or_else(|| {
                CliOutcome::err(ExitCode::Error, "usage", format!("{flag} requires a value"))
            })?;
            *i += 1;
            Ok(v)
        };
        match flag {
            "--json" => globals.json = true,
            "--help" | "-h" => globals.help = true,
            "--url" => globals.url = Some(value(&mut i)?),
            "--token-file" => globals.token_file = Some(PathBuf::from(value(&mut i)?)),
            "--data-dir" => globals.data_dir = Some(PathBuf::from(value(&mut i)?)),
            f if f == "--token" || f.starts_with("--token=") => {
                return Err(CliOutcome::err(
                    ExitCode::Error,
                    "invalid_flag",
                    "pass --token-file or CODETWO_TOKEN; tokens are not accepted on argv",
                ));
            }
            other => rest.push(other.to_string()),
        }
    }
    Ok((globals, rest))
}

fn usage_error(cmd: &str) -> CliOutcome {
    CliOutcome::err(
        ExitCode::Error,
        "usage",
        format!("invalid arguments for `{cmd}`"),
    )
}

fn dispatch_client(top: &str, args: Vec<String>, globals: &GlobalOpts) -> CliOutcome {
    // `mcp client ...` edits the local credential file and needs no running server or token.
    if top == "mcp" {
        return dispatch_mcp(globals, &args);
    }
    if top == "pair" {
        return dispatch_pair(globals, &args);
    }
    // Validate arguments before touching credentials so usage errors are reported as such.
    let action = match parse_action(top, &args) {
        Ok(action) => action,
        Err(outcome) => return outcome,
    };
    let config = match globals.resolve_client() {
        Ok(config) => config,
        Err(outcome) => return outcome,
    };
    match action {
        Action::Status => run_status(&config),
        Action::Pending => run_pending(&config),
        Action::Session(a) => run_session(&config, a),
        Action::Project(a) => run_project(&config, a),
        Action::Send(o) => run_send(&config, o),
        Action::Wait(o) => run_wait(&config, o),
        Action::Stop(id) => run_stop(&config, &id),
        Action::Approve { id, deny } => run_approve(&config, &id, deny),
        Action::Answer { id, text } => run_answer(&config, &id, &text),
        Action::Events(o) => run_events(&config, o),
    }
}

enum Action {
    Status,
    Pending,
    Session(SessionAction),
    Project(ProjectAction),
    Send(SendOpts),
    Wait(WaitOpts),
    Stop(String),
    Approve { id: String, deny: bool },
    Answer { id: String, text: String },
    Events(EventsOpts),
}

fn parse_action(top: &str, args: &[String]) -> Result<Action, CliOutcome> {
    let usage = || usage_error(top);
    let strs: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(match (top, strs.as_slice()) {
        ("status", []) => Action::Status,
        ("pending", []) => Action::Pending,
        ("session", ["list"]) => Action::Session(SessionAction::List),
        ("session", ["read", id]) => Action::Session(SessionAction::Read {
            session_id: id.to_string(),
        }),
        ("session", ["create", rest @ ..]) => parse_session_create(rest).ok_or_else(usage)?,
        ("project", ["list"]) => Action::Project(ProjectAction::List),
        ("project", ["create", path]) => Action::Project(ProjectAction::Create {
            path: path.to_string(),
        }),
        ("send", [id, rest @ ..]) => parse_send(id, rest)?,
        ("wait", [id, rest @ ..]) => {
            let timeout_secs = match rest {
                [] => 55,
                ["--timeout", secs] => secs.parse().map_err(|_| usage())?,
                _ => return Err(usage()),
            };
            Action::Wait(WaitOpts {
                session_id: id.to_string(),
                timeout_secs,
            })
        }
        ("stop", [id]) => Action::Stop(id.to_string()),
        ("approve", [id]) if !id.starts_with('-') => Action::Approve {
            id: id.to_string(),
            deny: false,
        },
        ("approve", [id, "--deny"]) | ("approve", ["--deny", id]) if !id.starts_with('-') => {
            Action::Approve {
                id: id.to_string(),
                deny: true,
            }
        }
        ("answer", [id, text @ ..]) if !text.is_empty() => Action::Answer {
            id: id.to_string(),
            text: text.join(" "),
        },
        ("events", rest) => parse_events(rest).ok_or_else(usage)?,
        _ => return Err(usage()),
    })
}

fn parse_session_create(args: &[&str]) -> Option<Action> {
    let mut project_path = None;
    let mut provider = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match *arg {
            "--project" | "-p" => project_path = Some(iter.next()?.to_string()),
            "--provider" => provider = Some(iter.next()?.to_string()),
            path if project_path.is_none() && !path.starts_with('-') => {
                project_path = Some(path.to_string())
            }
            _ => return None,
        }
    }
    Some(Action::Session(SessionAction::Create {
        project_path: project_path?,
        provider,
    }))
}

fn parse_send(session_id: &str, args: &[&str]) -> Result<Action, CliOutcome> {
    let mut mode = "prompt".to_string();
    let mut text_parts = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match *arg {
            "--mode" => match iter.next() {
                Some(m) => mode = m.to_string(),
                None => return Err(usage_error("send")),
            },
            other => text_parts.push(other),
        }
    }
    let text = if text_parts == ["-"] {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf).map_err(|e| {
            CliOutcome::err(ExitCode::Error, "stdin", format!("cannot read stdin: {e}"))
        })?;
        buf
    } else {
        text_parts.join(" ")
    };
    if text.trim().is_empty() {
        return Err(usage_error("send"));
    }
    Ok(Action::Send(SendOpts {
        session_id: session_id.to_string(),
        text,
        mode,
    }))
}

fn parse_events(args: &[&str]) -> Option<Action> {
    let mut cursor = None;
    let mut follow = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match *arg {
            "--since" => cursor = Some(iter.next()?.to_string()),
            "--follow" => follow = true,
            _ => return None,
        }
    }
    Some(Action::Events(EventsOpts { cursor, follow }))
}

/// `pair --url URL` plus exactly one credential source: global `--token-file F` or `--stdin`.
fn pair_source(globals: &GlobalOpts, args: &[String]) -> Result<(String, PairSource), CliOutcome> {
    let stdin = match args {
        [] => false,
        [flag] if flag == "--stdin" => true,
        _ => return Err(usage_error("pair")),
    };
    let url = globals
        .url
        .clone()
        .ok_or_else(|| CliOutcome::err(ExitCode::Error, "usage", "pair requires --url"))?;
    match (&globals.token_file, stdin) {
        (Some(path), false) => Ok((url, PairSource::File(path.clone()))),
        (None, true) => Ok((url, PairSource::Stdin)),
        _ => Err(CliOutcome::err(
            ExitCode::Error,
            "usage",
            "pair needs exactly one of --token-file FILE or --stdin",
        )),
    }
}

fn dispatch_pair(globals: &GlobalOpts, args: &[String]) -> CliOutcome {
    match pair_source(globals, args) {
        Ok((url, source)) => run_pair(globals, &url, source),
        Err(outcome) => outcome,
    }
}

fn dispatch_mcp(globals: &GlobalOpts, args: &[String]) -> CliOutcome {
    let strs: Vec<&str> = args.iter().map(String::as_str).collect();
    match strs.as_slice() {
        ["client", "list"] => run_mcp_client(globals, McpClientAction::List),
        ["client", "revoke", id] => {
            run_mcp_client(globals, McpClientAction::Revoke { id: id.to_string() })
        }
        ["client", "create", rest @ ..] => match parse_client_create(rest) {
            Ok(action) => run_mcp_client(globals, action),
            Err(outcome) => outcome,
        },
        _ => usage_error("mcp client"),
    }
}

fn parse_client_create(args: &[&str]) -> Result<McpClientAction, CliOutcome> {
    let usage = || usage_error("mcp client create");
    let (mut name, mut scopes, mut projects, mut ttl, mut token_out) =
        (None, None, None, None, None);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let slot = match *arg {
            "--name" => &mut name,
            "--scopes" => &mut scopes,
            "--projects" => &mut projects,
            "--ttl" => &mut ttl,
            "--token-out" => &mut token_out,
            _ => return Err(usage()),
        };
        *slot = Some(iter.next().ok_or_else(usage)?.to_string());
    }
    let name =
        name.ok_or_else(|| CliOutcome::err(ExitCode::Error, "usage", "--name is required"))?;
    Ok(McpClientAction::Create {
        name,
        scopes: admin::parse_scopes(scopes.as_deref())?,
        projects: admin::parse_projects(projects.as_deref())?,
        ttl: admin::parse_ttl(ttl.as_deref())?,
        token_out: token_out.map(PathBuf::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn serve_config_defaults_to_loopback_and_honors_port_flags() {
        let config = build_serve_config(&argv(&["--port", "5001"])).unwrap();
        assert_eq!((config.host.as_str(), config.port), ("127.0.0.1", 5001));
        let config =
            build_serve_config(&argv(&["--bind", "127.0.0.1:5002", "--external-mcp"])).unwrap();
        assert_eq!(config.port, 5002);
        assert!(config.external_mcp);
        let config = build_serve_config(&argv(&["--bind", "[::1]:5003"])).unwrap();
        assert_eq!((config.host.as_str(), config.port), ("[::1]", 5003));
    }

    #[test]
    fn serve_config_forwards_only_remote_flags() {
        let config = build_serve_config(&argv(&[
            "webui",
            "--no-open",
            "--remote",
            "cloudflared",
            "--remote-host",
            "h.example",
            "--allow-quick-tunnel",
        ]))
        .unwrap();
        assert_eq!(config.surface, ServeSurface::WebUi);
        assert!(!config.open_browser);
        assert_eq!(
            config.remote_args,
            argv(&[
                "--remote",
                "cloudflared",
                "--remote-host",
                "h.example",
                "--allow-quick-tunnel"
            ])
        );
    }

    #[test]
    fn serve_config_rejects_bad_input() {
        for bad in [
            &["--bogus"][..],
            &["--bind", "x:y"],
            &["--data-dir"],
            &["--port", "99999"],
        ] {
            assert!(build_serve_config(&argv(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn token_on_argv_is_refused_in_both_forms() {
        for bad in [&["--token", "x"][..], &["--token=x"]] {
            assert!(parse_globals(&argv(bad)).is_err());
        }
    }

    #[test]
    fn pair_requires_url_and_exactly_one_source() {
        let with = |url: Option<&str>, file: Option<&str>, args: &[&str]| {
            let globals = GlobalOpts {
                url: url.map(String::from),
                token_file: file.map(PathBuf::from),
                ..Default::default()
            };
            pair_source(&globals, &argv(args)).is_ok()
        };
        assert!(with(Some("https://h"), Some("f"), &[]));
        assert!(with(Some("https://h"), None, &["--stdin"]));
        assert!(!with(None, Some("f"), &[]));
        assert!(!with(Some("https://h"), None, &[]));
        assert!(!with(Some("https://h"), Some("f"), &["--stdin"]));
        assert!(!with(Some("https://h"), Some("f"), &["extra"]));
    }

    #[test]
    fn client_usage_errors_are_exit_1() {
        for (top, args) in [
            ("status", &["extra"][..]),
            ("send", &["only-id"]),
            ("send", &["id", "--mode"]),
            ("wait", &["id", "--timeout", "soon"]),
            ("approve", &["--deny"]),
            ("answer", &["id"]),
            ("events", &["--bogus"]),
            ("nope", &[]),
        ] {
            let err = parse_action(top, &argv(args))
                .err()
                .unwrap_or_else(|| panic!("{top} {args:?}"));
            assert_eq!(err.exit, ExitCode::Error);
        }
        assert!(parse_action("wait", &argv(&["id", "--timeout", "3"])).is_ok());
        assert!(parse_action("approve", &argv(&["rid", "--deny"])).is_ok());
    }
}

//! Local admin: edit `external-mcp-clients.json` without a running Core.

use std::path::PathBuf;

use codetwo_core::external_mcp::clients::{
    ExternalClientRegistry, ExternalScope, ProjectScope, TtlChoice,
};
use codetwo_core::external_mcp::state::CLIENTS_FILE;
use serde_json::{json, Value};

use super::client::probe_capabilities;
use super::config::{
    cli_token_path, cli_url_path, data_dir_for_admin, normalize_server_url, read_token_file,
    read_token_stdin, write_private_file, write_token_file, ClientConfig, GlobalOpts,
};
use super::output::{CliOutcome, ExitCode};

pub enum McpClientAction {
    Create {
        name: String,
        scopes: Vec<ExternalScope>,
        projects: ProjectScope,
        ttl: TtlChoice,
        token_out: Option<PathBuf>,
    },
    List,
    Revoke {
        id: String,
    },
}

pub fn parse_scopes(raw: Option<&str>) -> Result<Vec<ExternalScope>, CliOutcome> {
    let Some(raw) = raw else {
        return Ok(vec![
            ExternalScope::Read,
            ExternalScope::Operate,
            ExternalScope::Approve,
        ]);
    };
    if raw.is_empty() {
        return Err(CliOutcome::err(
            ExitCode::Error,
            "invalid_scopes",
            "scopes must not be empty",
        ));
    }
    let mut scopes = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        let scope = match part {
            "read" => ExternalScope::Read,
            "operate" => ExternalScope::Operate,
            "approve" => ExternalScope::Approve,
            "admin" => ExternalScope::Admin,
            _ => {
                return Err(CliOutcome::err(
                    ExitCode::Error,
                    "invalid_scopes",
                    format!("unknown scope: {part}"),
                ));
            }
        };
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    Ok(scopes)
}

/// `30d` (default), any `<N>d` (N >= 1), or an explicit `permanent`.
pub fn parse_ttl(raw: Option<&str>) -> Result<TtlChoice, CliOutcome> {
    let raw = raw.unwrap_or("30d");
    if raw == "permanent" {
        return Ok(TtlChoice::Permanent);
    }
    match raw.strip_suffix('d').and_then(|n| n.parse::<u32>().ok()) {
        Some(30) => Ok(TtlChoice::Default30Days),
        Some(days) if days >= 1 => Ok(TtlChoice::Days(days)),
        _ => Err(CliOutcome::err(
            ExitCode::Error,
            "invalid_ttl",
            format!("unsupported ttl: {raw} (use <N>d with N >= 1, or permanent)"),
        )),
    }
}

pub fn parse_projects(raw: Option<&str>) -> Result<ProjectScope, CliOutcome> {
    let Some(raw) = raw else {
        return Ok(ProjectScope::All);
    };
    if raw.eq_ignore_ascii_case("all") || raw == "*" {
        return Ok(ProjectScope::All);
    }
    let paths: Vec<PathBuf> = raw
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    if paths.is_empty() {
        return Err(CliOutcome::err(
            ExitCode::Error,
            "invalid_projects",
            "projects list is empty",
        ));
    }
    Ok(ProjectScope::Paths(paths))
}

/// Where `codetwo pair` takes the external MCP credential from. Never argv.
pub enum PairSource {
    File(PathBuf),
    Stdin,
}

/// Import an *external MCP* credential (`ctmcp_...`, issued by `mcp client create` or Settings)
/// for a possibly remote server. Web UI device Bearers and session tokens are a different
/// credential class and cannot be exchanged for one, so they are refused by shape.
///
/// The credential is checked with one read-only `codetwo_capabilities` call (no retry); only a
/// success saves `cli.token` (0600) and `cli.url`, so an invalid credential leaves nothing behind.
pub fn run_pair(opts: &GlobalOpts, url: &str, source: PairSource) -> CliOutcome {
    let base_url = match normalize_server_url(url) {
        Ok(url) => url,
        Err(outcome) => return outcome,
    };
    let token = match source {
        PairSource::File(path) => read_token_file(&path),
        PairSource::Stdin => read_token_stdin(),
    };
    let token = match token {
        Ok(token) => token,
        Err(outcome) => return outcome,
    };
    let config = ClientConfig { base_url, token };
    let capabilities = match probe_capabilities(&config) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let data_dir = data_dir_for_admin(opts);
    let token_path = cli_token_path(&data_dir);
    let url_path = cli_url_path(&data_dir);
    if let Err(error) = create_private_dir(&data_dir) {
        return CliOutcome::err(ExitCode::Error, "pair_save", error);
    }
    let previous_url = std::fs::read_to_string(&url_path).ok();
    if let Err(error) = write_private_file(&url_path, &format!("{}\n", config.base_url)) {
        return CliOutcome::err(ExitCode::Error, "pair_save", error);
    }
    if let Err(error) = write_token_file(&token_path, &config.token) {
        // Keep url and token consistent: put the old url back.
        match previous_url {
            Some(old) => drop(write_private_file(&url_path, &old)),
            None => drop(std::fs::remove_file(&url_path)),
        }
        return CliOutcome::err(ExitCode::Error, "pair_save", error);
    }
    CliOutcome::ok(json!({
        "paired": true,
        "url": config.base_url,
        "token_file": token_path,
        "client_scopes": capabilities.get("client_scopes").cloned().unwrap_or(Value::Null),
    }))
}

fn create_private_dir(dir: &std::path::Path) -> Result<(), String> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir).map_err(|e| e.to_string())
}

pub fn run_mcp_client(opts: &GlobalOpts, action: McpClientAction) -> CliOutcome {
    let data_dir = data_dir_for_admin(opts);
    let path = data_dir.join(CLIENTS_FILE);
    if let Err(error) = create_private_dir(&data_dir) {
        return CliOutcome::err(ExitCode::Error, "data_dir", error);
    }
    let mut registry = match ExternalClientRegistry::open(&path) {
        Ok(r) => r,
        Err(error) => {
            return CliOutcome::err(ExitCode::Error, "registry_error", format!("{error:?}"));
        }
    };

    match action {
        McpClientAction::Create {
            name,
            scopes,
            projects,
            ttl,
            token_out,
        } => {
            // The token is shown once; with --json it may only go to a file, so refuse before
            // creating a credential nobody could ever read.
            if opts.json && token_out.is_none() {
                return CliOutcome::err(
                    ExitCode::Error,
                    "token_out_required",
                    "with --json, pass --token-out for the one-time credential",
                );
            }
            let (record, token) = match registry.create(&name, scopes, projects, ttl) {
                Ok(pair) => pair,
                Err(error) => {
                    return CliOutcome::err(ExitCode::Error, "create_failed", format!("{error:?}"));
                }
            };
            match token_out {
                Some(out) => {
                    if let Err(error) = write_token_file(&out, &token) {
                        // Do not leave a live credential whose secret was lost.
                        if registry.revoke(&record.id).is_err() {
                            return CliOutcome::err(ExitCode::Error, "revoke_failed", format!("credential file could not be written; revoke client {} locally before using the server", record.id));
                        }
                        return CliOutcome::err(ExitCode::Error, "token_out", error);
                    }
                }
                None => println!("{token}"),
            }
            CliOutcome::ok(serde_json::to_value(&record).unwrap_or(Value::Null))
        }
        McpClientAction::List => match registry.list() {
            Ok(list) => CliOutcome::ok(json!({ "clients": list })),
            Err(error) => CliOutcome::err(ExitCode::Error, "list_failed", format!("{error:?}")),
        },
        McpClientAction::Revoke { id } => match registry.revoke(&id) {
            Ok(()) => CliOutcome::ok(json!({ "revoked": id })),
            Err(error) => CliOutcome::err(ExitCode::Error, "revoke_failed", format!("{error:?}")),
        },
    }
}

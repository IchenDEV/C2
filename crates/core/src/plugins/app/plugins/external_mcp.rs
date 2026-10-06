//! Desktop/CLI commands for the external MCP credential surface.

use crate::external_mcp::clients::{
    ExternalClientRecord, ExternalScope, ProjectScope, RegistryError, TtlChoice,
};
use crate::kernel::{async_trait, Context, Injection, Plugin, PluginError, PluginResult};
use crate::plugins::app::service::{EngineService, Paths};
use crate::plugins::app::{json, take_args};
use crate::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json as jval, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const SETTINGS_FILE: &str = "external-mcp-settings.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ExternalMcpSettings {
    #[serde(default)]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TtlArg {
    Default,
    Days { days: u32 },
    Permanent,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ProjectsArg {
    All(String),
    Paths(Vec<String>),
}

#[derive(Debug, Clone, Serialize)]
struct ExternalMcpStatus {
    enabled: bool,
    endpoint: Option<String>,
    data_dir_configured: bool,
    client_count: usize,
    active_client_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct ClientListItem {
    #[serde(flatten)]
    record: ExternalClientRecord,
    status: &'static str,
    expires_never: bool,
    projects_label: String,
}

#[derive(Debug, Clone, Serialize)]
struct CreateClientResult {
    record: ExternalClientRecord,
    token: String,
    config_snippet: Value,
    expires_never: bool,
}

pub struct ExternalMcpPlugin;

#[async_trait]
impl Plugin for ExternalMcpPlugin {
    fn name(&self) -> &str {
        "external-mcp"
    }

    fn description(&self) -> Option<&str> {
        Some("Issue and manage external MCP client credentials.")
    }

    fn inject(&self) -> Injection {
        Injection::required(["engine", "paths"])
    }

    async fn apply(&self, ctx: Context, _config: Value) -> PluginResult {
        let engine = ctx.expect::<EngineService>()?;
        let paths = ctx.expect::<Paths>()?;
        let settings_path = paths.data_dir.join(SETTINGS_FILE);
        let settings = Arc::new(Mutex::new(load_settings(&settings_path)));

        let scope = ctx.weak();
        engine.0.set_plugin_command_bridge(Some(Arc::new(move |name, args| {
            let ctx = scope.upgrade().ok_or("scene command scope is closed")?;
            crate::external_mcp::ops_write::run_async(async {
                ctx.call(name, args).await.map_err(|_| crate::external_mcp::ctx::ToolError::internal("scene command failed"))
            }).map_err(|error| error.message)
        })));
        let cleanup_engine = engine.0.clone();
        ctx.effect(move || cleanup_engine.set_plugin_command_bridge(None));

        let engine_boot = engine.0.clone();
        let paths_boot = paths.data_dir.clone();
        {
            let stored = settings.lock().unwrap();
            if stored.enabled {
                ensure_data_dir(&engine_boot, &paths_boot)?;
                engine_boot.external_mcp_state().set_enabled(true);
            }
        }

        register_commands(
            &ctx,
            engine.0.clone(),
            paths.data_dir.clone(),
            settings_path,
            settings,
        )?;
        Ok(())
    }
}

fn register_commands(
    ctx: &Context,
    engine: Arc<Engine>,
    data_dir: PathBuf,
    settings_path: PathBuf,
    settings: Arc<Mutex<ExternalMcpSettings>>,
) -> Result<(), PluginError> {
    let status_engine = engine.clone();
    ctx.command("external_mcp.status", move |_| {
        let engine = status_engine.clone();
        async move { json(build_status(&engine)) }
    })?;

    #[derive(Deserialize)]
    struct SetEnabledArgs {
        enabled: bool,
    }
    let toggle_engine = engine.clone();
    let toggle_dir = data_dir.clone();
    let toggle_settings_path = settings_path.clone();
    let toggle_settings = settings.clone();
    ctx.command("external_mcp.set_enabled", move |args| {
        let engine = toggle_engine.clone();
        let data_dir = toggle_dir.clone();
        let settings_path = toggle_settings_path.clone();
        let settings = toggle_settings.clone();
        async move {
            let args: SetEnabledArgs = take_args(args)?;
            if args.enabled {
                ensure_data_dir(&engine, &data_dir)?;
            }
            engine
                .external_mcp_state()
                .set_enabled(args.enabled);
            {
                let mut stored = settings.lock().unwrap();
                stored.enabled = args.enabled;
                persist_settings(&settings_path, &stored)?;
            }
            json(build_status(&engine))
        }
    })?;

    let list_engine = engine.clone();
    ctx.command("external_mcp.clients.list", move |_| {
        let engine = list_engine.clone();
        async move {
            let records = match with_registry(&engine, |registry| registry.list()) {
                Some(Ok(records)) => records,
                Some(Err(error)) => {
                    return Err(PluginError::new(registry_error_message(error)));
                }
                None => Vec::new(),
            };
            let now = Utc::now();
            let items = records
                .into_iter()
                .map(|record| client_list_item(record, now))
                .collect::<Vec<_>>();
            json(items)
        }
    })?;

    #[derive(Deserialize)]
    struct CreateArgs {
        name: String,
        #[serde(default)]
        scopes: Vec<String>,
        projects: ProjectsArg,
        ttl: TtlArg,
    }

    let create_engine = engine.clone();
    ctx.command("external_mcp.clients.create", move |args| {
        let engine = create_engine.clone();
        async move {
            let args: CreateArgs = take_args(args)?;
            if !engine.external_mcp_state().is_enabled() {
                return Err(PluginError::new("external MCP is disabled"));
            }
            let scopes = parse_scopes(&args.scopes)?;
            let projects = parse_projects(args.projects)?;
            let ttl = parse_ttl(&args.ttl)?;
            let expires_never = matches!(ttl, TtlChoice::Permanent);
            let (record, token) = create_client(&engine, &args.name, scopes, projects, ttl)?;
            let endpoint = external_endpoint(&engine);
            let config_snippet = config_snippet(endpoint.as_deref());
            let debug = serde_json::to_string(&jval!({ "record": &record })).unwrap_or_default();
            if debug.contains(&token) {
                return Err(PluginError::new("internal error: token leaked into record debug"));
            }
            json(CreateClientResult {
                record,
                token,
                config_snippet,
                expires_never,
            })
        }
    })?;

    #[derive(Deserialize)]
    struct RevokeArgs {
        id: String,
    }
    let revoke_engine = engine.clone();
    ctx.command("external_mcp.clients.revoke", move |args| {
        let engine = revoke_engine.clone();
        async move {
            let args: RevokeArgs = take_args(args)?;
            with_registry(&engine, |registry| registry.revoke(&args.id))
                .ok_or_else(|| PluginError::new("external MCP credential store is unavailable"))?
                .map_err(|error| PluginError::new(registry_error_message(error)))?;
            json(jval!({ "revoked": true, "id": args.id }))
        }
    })?;

    Ok(())
}

fn build_status(engine: &Engine) -> ExternalMcpStatus {
    let state = engine.external_mcp_state();
    let now = Utc::now();
    let (client_count, active_client_count) = state
        .with_registry(|registry| {
            let list = registry.list().unwrap_or_default();
            let active = list
                .iter()
                .filter(|record| client_status(record, now) == "active")
                .count();
            (list.len(), active)
        })
        .unwrap_or((0, 0));
    ExternalMcpStatus {
        enabled: state.is_enabled(),
        endpoint: external_endpoint(engine),
        data_dir_configured: state.data_dir().is_some(),
        client_count,
        active_client_count,
    }
}

fn external_endpoint(engine: &Engine) -> Option<String> {
    engine
        .host_mcp_http_endpoint()
        .map(|url| url.replace("/mcp", "/external-mcp"))
}

fn ensure_data_dir(engine: &Engine, data_dir: &Path) -> Result<(), PluginError> {
    if engine.external_mcp_state().data_dir().is_none() {
        engine
            .external_mcp_state()
            .configure_data_dir(data_dir)
            .map_err(PluginError::new)?;
    }
    Ok(())
}

fn with_registry<T>(
    engine: &Engine,
    f: impl FnOnce(&mut crate::external_mcp::clients::ExternalClientRegistry) -> T,
) -> Option<T> {
    engine.external_mcp_state().with_registry(f)
}

fn parse_scopes(raw: &[String]) -> Result<Vec<ExternalScope>, PluginError> {
    if raw.is_empty() {
        return Ok(vec![
            ExternalScope::Read,
            ExternalScope::Operate,
            ExternalScope::Approve,
        ]);
    }
    let mut scopes = Vec::with_capacity(raw.len());
    for item in raw {
        let scope = match item.as_str() {
            "read" => ExternalScope::Read,
            "operate" => ExternalScope::Operate,
            "approve" => ExternalScope::Approve,
            "admin" => ExternalScope::Admin,
            other => {
                return Err(PluginError::new(format!("unknown scope: {other}")));
            }
        };
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    Ok(scopes)
}

fn parse_projects(projects: ProjectsArg) -> Result<ProjectScope, PluginError> {
    match projects {
        ProjectsArg::All(value) if value == "all" => Ok(ProjectScope::All),
        ProjectsArg::All(other) => Err(PluginError::new(format!(
            "projects must be \"all\" or a path list, got {other}"
        ))),
        ProjectsArg::Paths(paths) => Ok(ProjectScope::Paths(
            paths.into_iter().map(PathBuf::from).collect(),
        )),
    }
}

fn parse_ttl(ttl: &TtlArg) -> Result<TtlChoice, PluginError> {
    match ttl {
        TtlArg::Default => Ok(TtlChoice::Default30Days),
        TtlArg::Days { days } => {
            if *days == 0 {
                return Err(PluginError::new("TTL days must be at least 1"));
            }
            Ok(TtlChoice::Days(*days))
        }
        TtlArg::Permanent => Ok(TtlChoice::Permanent),
    }
}

fn create_client(
    engine: &Engine,
    name: &str,
    scopes: Vec<ExternalScope>,
    projects: ProjectScope,
    ttl: TtlChoice,
) -> Result<(ExternalClientRecord, String), PluginError> {
    with_registry(engine, |registry| registry.create(name, scopes, projects, ttl))
        .ok_or_else(|| PluginError::new("external MCP credential store is unavailable"))?
        .map_err(|error| PluginError::new(registry_error_message(error)))
}

fn config_snippet(endpoint: Option<&str>) -> Value {
    let url = endpoint.unwrap_or("http://127.0.0.1:<port>/external-mcp");
    jval!({
        "mcpServers": {
            "codetwo": {
                "url": url,
                "headers": {
                    "Authorization": "Bearer <paste-your-token-here>"
                }
            }
        }
    })
}

fn client_list_item(record: ExternalClientRecord, now: DateTime<Utc>) -> ClientListItem {
    let status = client_status(&record, now);
    let expires_never = record.expires_at.is_none();
    let projects_label = match &record.projects {
        ProjectScope::All => "all".to_string(),
        ProjectScope::Paths(paths) => paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
    };
    ClientListItem {
        record,
        status,
        expires_never,
        projects_label,
    }
}

fn client_status(record: &ExternalClientRecord, now: DateTime<Utc>) -> &'static str {
    if record.revoked_at.is_some() {
        "revoked"
    } else if record
        .expires_at
        .is_some_and(|expires_at| expires_at <= now)
    {
        "expired"
    } else {
        "active"
    }
}

fn registry_error_message(error: RegistryError) -> String {
    error.to_string()
}

fn load_settings(path: &Path) -> ExternalMcpSettings {
    if !path.exists() {
        return ExternalMcpSettings::default();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn persist_settings(path: &Path, settings: &ExternalMcpSettings) -> Result<(), PluginError> {
    let parent = path
        .parent()
        .ok_or_else(|| PluginError::new("settings path has no parent"))?;
    std::fs::create_dir_all(parent).map_err(PluginError::new)?;
    let json = serde_json::to_vec_pretty(settings).map_err(PluginError::new)?;
    let temp = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("external-mcp-settings.json")
    ));
    std::fs::write(&temp, json).map_err(PluginError::new)?;
    std::fs::rename(&temp, path).map_err(PluginError::new)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_mcp::clients::ExternalClientRegistry;
    use crate::provider::default_registry;
    use crate::skill::SkillLibrary;
    use crate::store::Store;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_engine(dir: &TempDir) -> Arc<Engine> {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let (engine, _rx) = Engine::with_store(
            default_registry(),
            SkillLibrary::default(),
            store,
        );
        engine.set_private_data_dir(dir.path());
        engine.external_mcp_state().configure_data_dir(dir.path()).unwrap();
        engine.external_mcp_state().set_enabled(true);
        Arc::new(engine)
    }

    #[test]
    fn create_returns_token_once_and_list_omits_it() {
        let dir = TempDir::new().unwrap();
        let engine = test_engine(&dir);
        let (record, token) = create_client(
            &engine,
            "cli-bot",
            vec![ExternalScope::Read, ExternalScope::Operate, ExternalScope::Approve],
            ProjectScope::All,
            TtlChoice::Default30Days,
        )
        .unwrap();
        assert!(token.starts_with("ctmcp_"));
        let listed = engine
            .external_mcp_state()
            .with_registry(|registry| registry.list().unwrap())
            .unwrap();
        let json = serde_json::to_string(&listed).unwrap();
        assert!(!json.contains(&token));
        assert!(!json.contains("token_hash"));
        assert_eq!(listed[0].id, record.id);
    }

    #[test]
    fn revoke_marks_client_revoked() {
        let dir = TempDir::new().unwrap();
        let engine = test_engine(&dir);
        let (record, token) = create_client(
            &engine,
            "revoke-me",
            vec![ExternalScope::Read],
            ProjectScope::All,
            TtlChoice::Default30Days,
        )
        .unwrap();
        with_registry(&engine, |registry| registry.revoke(&record.id))
            .unwrap()
            .unwrap();
        let err = engine
            .external_mcp_state()
            .with_registry(|registry| registry.resolve(&token))
            .unwrap()
            .unwrap_err();
        assert!(matches!(
            err,
            crate::external_mcp::clients::ResolveError::Revoked
        ));
    }

    #[test]
    fn permanent_ttl_create_succeeds() {
        let dir = TempDir::new().unwrap();
        let engine = test_engine(&dir);
        let (record, token) = create_client(
            &engine,
            "forever",
            vec![ExternalScope::Read],
            ProjectScope::All,
            TtlChoice::Permanent,
        )
        .unwrap();
        assert!(record.expires_at.is_none());
        assert!(token.starts_with("ctmcp_"));
    }

    #[test]
    fn invalid_name_rejected() {
        let dir = TempDir::new().unwrap();
        let mut registry = ExternalClientRegistry::open(dir.path().join("external-mcp-clients.json")).unwrap();
        let err = registry
            .create("", vec![ExternalScope::Read], ProjectScope::All, TtlChoice::Default30Days)
            .unwrap_err();
        assert!(matches!(err, RegistryError::Validation(_)));
    }
}

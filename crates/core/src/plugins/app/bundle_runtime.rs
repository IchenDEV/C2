use super::{normalize_project_path, protocol::ProtocolPlugin};
use crate::kernel::{
    async_trait, CommandRealm, Context, Injection, Plugin, PluginCategory, PluginEntry,
    PluginError, PluginMetadata, PluginOrigin, PluginRegistry, PluginResult, PluginRole, Service,
};
use crate::plugins::bundle::{
    observation_connector_ids, InstalledPlugin, PluginConnectorContribution, PluginRuntimeCommand,
    PluginRuntimeSpec,
};
use serde::Serialize;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Lifecycle capability owned by the in-process extensions host. Process bundles require this
/// service so disabling or unloading that host tears down every runtime it supervises.
#[derive(Debug, Default)]
pub(crate) struct ExtensionRuntimeHost;

impl Service for ExtensionRuntimeHost {
    const NAME: &'static str = "extensions-runtime";
}

/// One installed process bundle translated into the kernel's runtime factory vocabulary.
#[derive(Clone)]
pub(crate) struct BundleRuntimeDescriptor {
    pub(crate) name: String,
    pub(crate) default_entry: PluginEntry,
    pub(crate) fingerprint: String,
    plugin: BundleRuntimePlugin,
}

impl BundleRuntimeDescriptor {
    pub(crate) fn register_into(&self, registry: &mut PluginRegistry) {
        let plugin = self.plugin.clone();
        registry.register_arc(Box::new(move || Arc::new(plugin.clone())));
    }
}

pub(crate) fn bundle_runtime_descriptor(
    installed: &InstalledPlugin,
    plugins_dir: &Path,
) -> Option<BundleRuntimeDescriptor> {
    let runtime = installed.runtime.clone()?;
    if !is_safe_bundle_id(&installed.id) {
        tracing::warn!(plugin = %installed.id, "ignoring process bundle with an unsafe id");
        return None;
    }

    let name = format!("bundle:{}", installed.id);
    let bundle_dir = plugins_dir.join(&installed.id).join("bundle");
    let data_root = plugins_dir.join(".data").join(&installed.id);
    let default_enabled = installed.enabled && installed.trusted;
    let metadata = PluginMetadata {
        origin: PluginOrigin::ThirdParty,
        role: PluginRole::Extension,
        category: PluginCategory::Integration,
        scope_support: runtime.scope_support.clone(),
        essential: false,
        default_enabled,
    };
    let fingerprint = bundle_fingerprint(installed, &runtime, &name, plugins_dir);
    let plugin = BundleRuntimePlugin {
        name: name.clone(),
        description: installed.description.clone(),
        metadata,
        trusted: installed.trusted,
        runtime,
        runtime_commands: installed.runtime_commands.clone(),
        observation_connectors: observation_connector_ids(&installed.connector_contributions),
        bundle_dir,
        data_root,
    };
    Some(BundleRuntimeDescriptor {
        name,
        default_entry: PluginEntry {
            enabled: default_enabled,
            config: Value::Null,
        },
        fingerprint,
        plugin,
    })
}

fn is_safe_bundle_id(id: &str) -> bool {
    let mut components = Path::new(id).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FingerprintMaterial<'a> {
    name: &'a str,
    display_name: &'a str,
    version: &'a str,
    description: &'a str,
    trusted: bool,
    runtime: &'a PluginRuntimeSpec,
    runtime_commands: &'a [PluginRuntimeCommand],
    connector_contributions: &'a [PluginConnectorContribution],
    plugins_root: String,
}

fn bundle_fingerprint(
    installed: &InstalledPlugin,
    runtime: &PluginRuntimeSpec,
    name: &str,
    plugins_dir: &Path,
) -> String {
    let material = FingerprintMaterial {
        name,
        display_name: &installed.name,
        version: &installed.version,
        description: &installed.description,
        trusted: installed.trusted,
        runtime,
        runtime_commands: &installed.runtime_commands,
        connector_contributions: &installed.connector_contributions,
        // Normalize the existing inventory root once. Canonicalizing the child data directory
        // would make the fingerprint change merely because the first run created that directory
        // (notably `/var` becoming `/private/var` on macOS).
        plugins_root: normalize_project_path(plugins_dir),
    };
    let encoded = serde_json::to_vec(&material)
        .expect("bundle runtime fingerprint material must always serialize");
    blake3::hash(&encoded).to_hex().to_string()
}

#[derive(Clone)]
struct BundleRuntimePlugin {
    name: String,
    description: String,
    metadata: PluginMetadata,
    trusted: bool,
    runtime: PluginRuntimeSpec,
    runtime_commands: Vec<PluginRuntimeCommand>,
    observation_connectors: Vec<String>,
    bundle_dir: PathBuf,
    data_root: PathBuf,
}

impl BundleRuntimePlugin {
    fn context_and_data_dir(&self, ctx: Context) -> (Context, PathBuf) {
        match ctx.command_realm() {
            CommandRealm::Global => (ctx, self.data_root.clone()),
            CommandRealm::Project(project_path) => {
                let project_path = normalize_project_path(project_path);
                let project_key = blake3::hash(project_path.as_bytes()).to_hex().to_string();
                let data_dir = self.data_root.join("projects").join(project_key);
                (
                    ctx.with_command_realm(CommandRealm::project(project_path)),
                    data_dir,
                )
            }
        }
    }
}

#[async_trait]
impl Plugin for BundleRuntimePlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn metadata(&self) -> PluginMetadata {
        self.metadata.clone()
    }

    fn inject(&self) -> Injection {
        let mut required = vec![ExtensionRuntimeHost::NAME.to_string()];
        if !self.observation_connectors.is_empty() {
            required.push("store".into());
        }
        required.extend(
            self.runtime
                .inject
                .iter()
                .filter(|name| name.as_str() != ExtensionRuntimeHost::NAME)
                .cloned(),
        );
        Injection {
            required,
            optional: self.runtime.optional_inject.clone(),
        }
    }

    fn description(&self) -> Option<&str> {
        (!self.description.is_empty()).then_some(self.description.as_str())
    }

    async fn apply(&self, ctx: Context, config: Value) -> PluginResult {
        if !self.trusted {
            return Err(PluginError::new(format!(
                "bundle `{}` is not trusted; approve it before enabling its runtime",
                self.name
            )));
        }

        let (ctx, data_dir) = self.context_and_data_dir(ctx);
        let mut protocol =
            ProtocolPlugin::from_spec(&self.name, &self.runtime, self.bundle_dir.clone(), data_dir);
        protocol = protocol
            .with_declared_commands(self.runtime_commands.clone())
            .with_observation_connectors(self.observation_connectors.clone());
        if !self.description.is_empty() {
            protocol = protocol.with_description(self.description.clone());
        }
        Plugin::apply(&protocol, ctx, config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::{PluginCategory, PluginOrigin, PluginScopeSupport, Status};
    use serde_json::{json, Value};

    fn installed(trusted: bool) -> InstalledPlugin {
        serde_json::from_value(json!({
            "schema_version": 3,
            "id": "fixture",
            "name": "Fixture",
            "version": "1.2.3",
            "author": "C2 Test",
            "description": "A fixture process",
            "source": "local-test",
            "repository": "fixture",
            "standard_version": "1.2.0",
            "enabled": true,
            "trusted": trusted,
            "scope": "user",
            "counts": {
                "skills": 0,
                "subagents": 0,
                "mcp_servers": 0,
                "scaffolds": 0,
                "commands": 0,
                "runtime_commands": 1,
                "hooks": 0,
                "lsp_servers": 0,
                "monitors": 0,
                "apps": 0,
                "ui": 0,
                "connectors": 0,
                "scenes": 0,
                "pipelines": 0,
                "runtime": 1
            },
            "components": [],
            "scaffolds": [],
            "extension_components": [],
            "runtime_commands": [{
                "id": "fixture.ping",
                "title": "Ping fixture"
            }],
            "ui_contributions": [],
            "connector_contributions": [],
            "lsp_servers": [],
            "diagnostics": [],
            "runtime": {
                "protocol": "1.0.0",
                "command": "definitely-not-a-real-binary",
                "scopeSupport": ["user", "project"]
            }
        }))
        .unwrap()
    }

    #[test]
    fn descriptor_is_stable_and_registers_third_party_metadata() {
        let root = tempfile::tempdir().unwrap();
        let descriptor = bundle_runtime_descriptor(&installed(true), root.path()).unwrap();
        let again = bundle_runtime_descriptor(&installed(true), root.path()).unwrap();
        assert_eq!(descriptor.name, "bundle:fixture");
        assert_eq!(descriptor.default_entry, PluginEntry::default());
        assert_eq!(descriptor.fingerprint, again.fingerprint);
        std::fs::create_dir_all(root.path().join(".data/fixture")).unwrap();
        assert_eq!(
            descriptor.fingerprint,
            bundle_runtime_descriptor(&installed(true), root.path())
                .unwrap()
                .fingerprint,
            "creating the runtime data directory must not invalidate its factory"
        );

        let mut changed = installed(true);
        changed
            .runtime
            .as_mut()
            .unwrap()
            .args
            .push("--changed".into());
        assert_ne!(
            descriptor.fingerprint,
            bundle_runtime_descriptor(&changed, root.path())
                .unwrap()
                .fingerprint
        );

        let mut static_commands = installed(true);
        static_commands.runtime_commands = vec![PluginRuntimeCommand {
            id: "fixture.run".into(),
            title: "Run fixture".into(),
            description: "Run the fixture command.".into(),
            args_schema: None,
        }];
        let static_fingerprint = bundle_runtime_descriptor(&static_commands, root.path())
            .unwrap()
            .fingerprint;
        assert_ne!(descriptor.fingerprint, static_fingerprint);
        static_commands.runtime_commands[0].description = "Changed".into();
        assert_ne!(
            static_fingerprint,
            bundle_runtime_descriptor(&static_commands, root.path())
                .unwrap()
                .fingerprint
        );

        let mut registry = PluginRegistry::new();
        descriptor.register_into(&mut registry);
        let factory = registry.get("bundle:fixture").unwrap();
        assert_eq!(factory.description.as_deref(), Some("A fixture process"));
        assert_eq!(factory.metadata.origin, PluginOrigin::ThirdParty);
        assert_eq!(factory.metadata.role, PluginRole::Extension);
        assert_eq!(factory.metadata.category, PluginCategory::Integration);
        assert_eq!(
            factory.metadata.scope_support,
            [PluginScopeSupport::User, PluginScopeSupport::Project]
        );
        assert_eq!(factory.dependencies.required, ["extensions-runtime"]);
        assert_eq!(factory.build().name(), "bundle:fixture");
    }

    #[test]
    fn untrusted_bundles_are_disabled_by_default_and_change_the_fingerprint() {
        let root = tempfile::tempdir().unwrap();
        let trusted = bundle_runtime_descriptor(&installed(true), root.path()).unwrap();
        let untrusted = bundle_runtime_descriptor(&installed(false), root.path()).unwrap();
        assert!(trusted.default_entry.enabled);
        assert!(!untrusted.default_entry.enabled);
        assert_ne!(trusted.fingerprint, untrusted.fingerprint);
    }

    #[tokio::test]
    async fn data_directories_preserve_global_layout_and_hash_normalized_projects() {
        let root = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let descriptor = bundle_runtime_descriptor(&installed(true), root.path()).unwrap();
        let app = crate::kernel::App::new();

        let (_, global_data) = descriptor.plugin.context_and_data_dir(app.ctx());
        assert_eq!(global_data, root.path().join(".data/fixture"));

        let normalized = normalize_project_path(project.path());
        let project_context = app.ctx().with_command_realm(CommandRealm::project(
            project.path().join(".").to_string_lossy(),
        ));
        let (project_context, project_data) =
            descriptor.plugin.context_and_data_dir(project_context);
        let expected_key = blake3::hash(normalized.as_bytes()).to_hex().to_string();
        assert_eq!(
            project_data,
            root.path()
                .join(".data/fixture/projects")
                .join(expected_key)
        );
        assert_eq!(
            project_context.command_realm(),
            &CommandRealm::project(normalized)
        );
    }

    #[tokio::test]
    async fn trust_is_checked_before_attempting_to_start_the_transport() {
        let root = tempfile::tempdir().unwrap();
        let descriptor = bundle_runtime_descriptor(&installed(false), root.path()).unwrap();
        let mut registry = PluginRegistry::new();
        descriptor.register_into(&mut registry);

        let app = crate::kernel::App::new();
        app.ctx().provide(Arc::new(ExtensionRuntimeHost)).unwrap();
        let fork = app
            .ctx()
            .plugin_arc(registry.get("bundle:fixture").unwrap().build(), Value::Null);
        app.flush().await;
        assert_eq!(fork.status(), Status::Failed);
        let error = app
            .runtime()
            .scopes()
            .into_iter()
            .find(|scope| scope.id == fork.id())
            .and_then(|scope| scope.error)
            .unwrap();
        assert!(error.contains("trusted"), "unexpected failure: {error}");
        assert!(
            !error.contains("couldn't start"),
            "transport was attempted: {error}"
        );
    }
}

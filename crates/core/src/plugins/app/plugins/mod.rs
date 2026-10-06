//! Core and built-in runtime modules, and the registry that makes them loadable by name.
//!
//! These modules share the kernel's internal lifecycle. That does not make Core infrastructure a
//! user-installable extension or make its commands part of the public Extension API.

mod assistant;
mod canvas;
mod engine;
mod extensions;
mod foundation;
mod handoff;
mod hub;
mod issues;
mod library;
mod memory;
mod plugin_development;
mod runtime;
mod scene_commands;
mod terminal;
mod utility;
mod workspace;
mod workspace_io;

pub use canvas::{CanvasPlugin, DocumentPlugin};
pub use engine::{EngineBuilder, EngineInputs, EnginePlugin};
pub use extensions::ExtensionsPlugin;
pub use foundation::{BusPlugin, PathsPlugin, ProvidersPlugin, StorePlugin};
pub use handoff::HandoffPlugin;
pub use hub::{HubPlugin, KernelPlugin};
pub use issues::IssuesPlugin;
pub use library::{ScenesPlugin, SkillsPlugin};
pub use memory::MemoryPlugin;
pub use runtime::{CostPlugin, SceneRuntimePlugin};
pub use scene_commands::SceneCommandsPlugin;
pub use terminal::TerminalPlugin;
pub use utility::{UsagePlugin, VoicePlugin};
pub use workspace::{GitPlugin, KeymapPlugin, MarketPlugin};
pub use workspace_io::{ArtifactsPlugin, ProjectsPlugin, WorkspacePlugin, WorkspaceSearchPlugin};

use crate::kernel::{
    PluginCategory, PluginMetadata, PluginOrigin, PluginRegistry, PluginRole, PluginScopeSupport,
};

/// Runtime modules that form the C2 platform rather than an optional extension.
///
/// They still use the kernel's [`Plugin`](crate::kernel::Plugin) lifecycle internally, but their
/// presence is owned by host configuration and cannot be changed through extension policy.
pub const CORE: &[&str] = &[
    "paths",
    "store",
    "bus",
    "providers",
    "plugin-hub",
    "skills",
    "engine",
    "kernel",
    "extensions",
];

/// The names of every built-in, in the order a fresh config enables them.
///
/// Order is documentation, not sequencing — the kernel resolves the real order from each plugin's
/// injections. `engine` listed last would load just the same.
pub const BUILTIN: &[&str] = &[
    "paths",
    "store",
    "bus",
    "providers",
    "plugin-hub",
    "skills",
    "scenes",
    "engine",
    "handoff",
    "assistant",
    "git",
    "memory",
    "market",
    "workspace",
    "projects",
    "artifacts",
    "workspace-search",
    "usage",
    "voice",
    "issues",
    "canvas",
    "document",
    "keymap",
    "scene-runtime",
    "scene-commands",
    "terminal",
    "cost",
    "kernel",
    "extensions",
];

/// Everything C2 ships, ready to be loaded by name from a config file.
pub fn builtin_registry() -> PluginRegistry {
    let mut registry = PluginRegistry::new();
    registry.register(|| PathsPlugin);
    registry.register(|| StorePlugin);
    registry.register(|| BusPlugin);
    registry.register(|| ProvidersPlugin);
    registry.register(|| HubPlugin);
    registry.register(|| SkillsPlugin);
    registry.register(|| ScenesPlugin);
    registry.register(EnginePlugin::new);
    registry.register(|| HandoffPlugin);
    registry.register(|| assistant::AssistantPlugin);
    registry.register(|| GitPlugin);
    registry.register(|| MemoryPlugin);
    registry.register(|| MarketPlugin);
    registry.register(|| WorkspacePlugin);
    registry.register(|| ProjectsPlugin);
    registry.register(|| ArtifactsPlugin);
    registry.register(|| WorkspaceSearchPlugin);
    registry.register(|| UsagePlugin);
    registry.register(|| VoicePlugin);
    registry.register(|| IssuesPlugin);
    registry.register(|| CanvasPlugin);
    registry.register(|| DocumentPlugin);
    registry.register(|| KeymapPlugin);
    registry.register(|| SceneRuntimePlugin);
    registry.register(|| SceneCommandsPlugin);
    registry.register(|| TerminalPlugin);
    registry.register(|| CostPlugin);
    registry.register(|| KernelPlugin);
    registry.register(|| ExtensionsPlugin);

    for (name, category) in [
        ("paths", PluginCategory::Foundation),
        ("store", PluginCategory::Foundation),
        ("bus", PluginCategory::Foundation),
        ("providers", PluginCategory::Foundation),
        ("engine", PluginCategory::Foundation),
        ("handoff", PluginCategory::Integration),
        ("assistant", PluginCategory::Automation),
        ("memory", PluginCategory::Foundation),
        ("kernel", PluginCategory::Foundation),
        ("workspace", PluginCategory::Workspace),
        ("projects", PluginCategory::Workspace),
        ("artifacts", PluginCategory::Workspace),
        ("workspace-search", PluginCategory::Workspace),
        ("issues", PluginCategory::Workspace),
        ("scenes", PluginCategory::Automation),
        ("scene-runtime", PluginCategory::Automation),
        ("scene-commands", PluginCategory::Automation),
        ("git", PluginCategory::DeveloperTools),
        ("terminal", PluginCategory::DeveloperTools),
        ("cost", PluginCategory::DeveloperTools),
        ("plugin-hub", PluginCategory::Interface),
        ("canvas", PluginCategory::Interface),
        ("document", PluginCategory::Interface),
        ("keymap", PluginCategory::Interface),
        ("usage", PluginCategory::Interface),
        ("voice", PluginCategory::Interface),
        ("skills", PluginCategory::Integration),
        ("market", PluginCategory::Integration),
        ("extensions", PluginCategory::Integration),
    ] {
        let scope_support = if matches!(
            name,
            "git" | "workspace" | "artifacts" | "workspace-search" | "issues" | "terminal"
        ) {
            vec![PluginScopeSupport::User, PluginScopeSupport::Project]
        } else {
            vec![PluginScopeSupport::User]
        };

        registry
            .set_metadata(
                name,
                PluginMetadata {
                    origin: PluginOrigin::BuiltIn,
                    role: if CORE.contains(&name) {
                        PluginRole::Core
                    } else {
                        PluginRole::BuiltIn
                    },
                    category,
                    scope_support,
                    essential: name == "kernel",
                    default_enabled: true,
                },
            )
            .expect("built-in metadata must refer to a registered plugin");
    }

    registry
}

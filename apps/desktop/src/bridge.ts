import {
  desktopAppshotSettings,
  desktopCaptureAppshot,
  desktopGetAppshot,
  desktopConfirm,
  desktopOpenDevtools,
  desktopOpenDialog,
  desktopOpenExternal,
  desktopOpenPath,
  desktopShowItemInFolder,
  desktopOpenWorkspace,
  desktopSaveDialog,
  desktopSetSystemBadgeCount,
  desktopSystemProfileAvatar,
  desktopOpenAppshotPrivacySettings,
  desktopRequestAppshotPermissions,
  desktopUpdateAppshotSettings,
  desktopCheckForUpdates,
  desktopUpdateStatus,
  isElectrobun,
  listenDesktop,
  onDesktopAppshotCaptured,
  onDesktopAppshotFailed,
  browserAnnotateLocal,
  browserAnnotationCountLocal,
  browserAnnotationsClearLocal,
  browserAnnotationsLocal,
  browserBoundsLocal,
  browserCloseAllLocal,
  browserCloseLocal,
  browserDevtoolsLocal,
  browserHistoryLocal,
  browserNavigateLocal,
  browserOpenLocal,
  browserRegistryCreateLocal,
  browserRegistrySnapshotLocal,
  browserReloadLocal,
  browserSubscribe,
  browserTakeControlLocal,
  browserVisibleLocal,
  browserZoomLocal,
} from "./container";
import type {
  EmbeddedBrowserTab,
  AppshotCapture,
  AppshotSettings,
  AppUpdateStatus,
  WorkspaceOpenTarget,
} from "./container";
export {
  type AppshotCapture,
  type AppshotDestination,
  type AppshotHotkey,
  type AppshotSettings,
  type AppUpdateStatus,
  type WorkspaceOpenTarget,
} from "./container";
import {
  coreAvailable,
  coreCall,
  coreCallEnvironment,
  environmentRegistry,
  listenCore,
} from "./coreTransport";
import type {
  PluginConnectorContribution,
  PluginRuntimeCommandContribution,
  PluginUiContribution,
} from "./pluginModel";

// Product-facing content bridge. Native shell details stay behind `container.ts`.

export async function getAppUpdateStatus(): Promise<AppUpdateStatus> {
  return await desktopUpdateStatus();
}

export async function checkForAppUpdates(): Promise<AppUpdateStatus> {
  return await desktopCheckForUpdates();
}

export async function setSystemBadgeCount(count: number): Promise<boolean> {
  return inDesktop ? await desktopSetSystemBadgeCount(count) : false;
}

let systemProfileAvatarRequest: Promise<string | null> | null = null;

export async function systemProfileAvatar(): Promise<string | null> {
  if (!inDesktop) return null;
  systemProfileAvatarRequest ??= desktopSystemProfileAvatar();
  return await systemProfileAvatarRequest;
}

const browserAppshotSettings: AppshotSettings = {
  available: false,
  hotkey: "both-command",
  destination: "automatic",
  play_sound: true,
  screen_recording: false,
  accessibility: false,
  hotkey_registered: false,
  unavailable_reason: "Appshots require the C2 macOS desktop app.",
};

export async function getAppshotSettings(): Promise<AppshotSettings> {
  return inDesktop ? await desktopAppshotSettings() : browserAppshotSettings;
}

export async function updateAppshotSettings(
  patch: Partial<Pick<AppshotSettings, "hotkey" | "destination" | "play_sound">>
): Promise<AppshotSettings> {
  return inDesktop
    ? await desktopUpdateAppshotSettings(patch)
    : { ...browserAppshotSettings, ...patch };
}

export async function requestAppshotPermissions(
  kind: "screen-recording" | "accessibility"
): Promise<AppshotSettings> {
  return inDesktop
    ? await desktopRequestAppshotPermissions(kind)
    : browserAppshotSettings;
}

export async function openAppshotPrivacySettings(
  kind: "screen-recording" | "accessibility"
): Promise<boolean> {
  return inDesktop ? await desktopOpenAppshotPrivacySettings(kind) : false;
}

export async function takeAppshot(): Promise<AppshotCapture> {
  if (!inDesktop)
    throw new Error(
      browserAppshotSettings.unavailable_reason ?? "Appshots are unavailable."
    );
  return await desktopCaptureAppshot();
}

/** Reload an app-owned screen capture while its private retention window is still active. */
export async function getAppshot(id: string): Promise<AppshotCapture> {
  if (!inDesktop) throw new Error("Appshots require the C2 macOS desktop app.");
  return await desktopGetAppshot(id);
}

export async function onAppshotCaptured(
  cb: (capture: AppshotCapture) => void
): Promise<() => void> {
  return inDesktop
    ? await onDesktopAppshotCaptured(cb)
    : () => {
        /* empty */
      };
}

export async function onAppshotFailed(
  cb: (failure: { message: string }) => void
): Promise<() => void> {
  return inDesktop
    ? await onDesktopAppshotFailed(cb)
    : () => {
        /* empty */
      };
}

export type DeviceSyncState =
  | "disabled"
  | "ready"
  | "syncing"
  | "unsupported"
  | "signed-out"
  | "restricted"
  | "unavailable"
  | "error";

export interface DeviceSyncStatus {
  transport: string;
  state: DeviceSyncState;
  enabled: boolean;
  available: boolean;
  last_success_at: number | null;
  message: string | null;
  imported: {
    projects: number;
    sessions: number;
    parts: number;
    memories: number;
  } | null;
}

export async function getDeviceSyncStatus(): Promise<DeviceSyncStatus> {
  return inDesktop
    ? await call<DeviceSyncStatus>("device_sync.status")
    : {
        transport: "paired-devices",
        state: "unsupported",
        enabled: false,
        available: false,
        last_success_at: null,
        message: null,
        imported: null,
      };
}

export async function setDeviceSyncEnabled(
  enabled: boolean
): Promise<DeviceSyncStatus> {
  return inDesktop
    ? await call<DeviceSyncStatus>("device_sync.set_enabled", { enabled })
    : await getDeviceSyncStatus();
}

export async function syncDeviceDataNow(): Promise<DeviceSyncStatus> {
  return inDesktop
    ? await call<DeviceSyncStatus>("device_sync.sync_now")
    : await getDeviceSyncStatus();
}

export interface ProviderInfo {
  id: string;
  display_name: string;
  custom: boolean;
  available: boolean;
  enabled: boolean;
  needs_node: boolean;
  /// The core's built-in models for this provider — what the picker offers when the provider
  /// reports none of its own over ACP. Empty only for providers we ship no list for.
  models: ModelChoice[];
  capabilities: ProviderCapability[];
  management: ProviderManagementInfo;
  configuration: ProviderRuntimeConfiguration;
}

export interface ProviderRuntimeOverride {
  display_name: string | null;
  command: string | null;
  args: string[] | null;
  home_path: string | null;
  forwarded_environment: string[];
}

export interface ProviderRuntimeConfiguration extends ProviderRuntimeOverride {
  home_environment: string | null;
  missing_environment: string[];
  effective_command: string;
  effective_args: string[];
}

export interface CustomProviderConfiguration {
  id: string;
  display_name: string;
  command: string;
  args: string[];
  forwarded_environment: string[];
}

export interface ProviderManagementInfo {
  installed: boolean;
  version: string | null;
  latest_version: string | null;
  update_available: boolean | null;
  check_error: string | null;
  install_supported: boolean;
  upgrade_supported: boolean;
  launch_mode: "installed" | "on_demand" | "unavailable";
}

export type ProviderCapabilityId =
  | "image_generation"
  | "computer_use"
  | "chrome_browser"
  | "codetwo_browser"
  | "sites";
export type CapabilityState = "ready" | "unverified" | "unavailable";

export interface ProviderCapability {
  id: ProviderCapabilityId;
  state: CapabilityState;
  version?: string | null;
  experimental: boolean;
  reason?: string | null;
  fix?: string | null;
}

export interface ComputerUseBackendOption {
  id: string;
  display_name: string;
  available: boolean;
  reason: string | null;
  providers: string[];
  exclude_providers: string[];
}

export interface ComputerUseSettings {
  selections: Record<string, string>;
  backends: ComputerUseBackendOption[];
  errors: string[];
}

export type BrowserUseBackendOption = ComputerUseBackendOption;
export interface BrowserUseSettings extends ComputerUseSettings {
  access_enabled: boolean;
}

interface ComputerUseBackendWire {
  id: string;
  display_name?: string;
  displayName?: string;
  available: boolean;
  reason?: string | null;
  providers?: string[];
  exclude_providers?: string[];
  excludeProviders?: string[];
}

interface ComputerUseSettingsWire {
  selections?: Record<string, string>;
  backends?: ComputerUseBackendWire[];
  errors?: string[];
}

interface BrowserUseSettingsWire extends ComputerUseSettingsWire {
  access_enabled?: boolean;
  accessEnabled?: boolean;
}

function normalizeComputerUseSettings(
  settings: ComputerUseSettingsWire
): ComputerUseSettings {
  return {
    selections: settings.selections ?? {},
    backends: (settings.backends ?? []).map((backend) => ({
      id: backend.id,
      display_name: backend.display_name ?? backend.displayName ?? backend.id,
      available: backend.available,
      reason: backend.reason ?? null,
      providers: backend.providers ?? [],
      exclude_providers:
        backend.exclude_providers ?? backend.excludeProviders ?? [],
    })),
    errors: settings.errors ?? [],
  };
}

function normalizeBrowserUseSettings(
  settings: BrowserUseSettingsWire
): BrowserUseSettings {
  return {
    ...normalizeComputerUseSettings(settings),
    access_enabled: settings.access_enabled ?? settings.accessEnabled ?? false,
  };
}

type ProviderInfoWire = Omit<
  ProviderInfo,
  "capabilities" | "custom" | "enabled" | "management" | "configuration"
> & {
  capabilities?: ProviderCapability[] | null;
  custom?: boolean | null;
  enabled?: boolean | null;
  management?: ProviderManagementInfo | null;
  configuration?: ProviderRuntimeConfiguration | null;
};

function defaultProviderConfiguration(
  provider: Pick<ProviderInfo, "id">
): ProviderRuntimeConfiguration {
  return {
    display_name: null,
    command: null,
    args: null,
    home_path: null,
    home_environment:
      provider.id === "codex"
        ? "CODEX_HOME"
        : provider.id === "claude_code"
          ? "CLAUDE_CONFIG_DIR"
          : null,
    forwarded_environment: [],
    missing_environment: [],
    effective_command: "",
    effective_args: [],
  };
}

export function normalizeProviderInfo(
  provider: ProviderInfoWire
): ProviderInfo {
  return {
    ...provider,
    custom: provider.custom ?? false,
    enabled: provider.enabled ?? true,
    capabilities: provider.capabilities ?? [],
    configuration:
      provider.configuration ?? defaultProviderConfiguration(provider),
    management: provider.management ?? {
      installed: provider.available,
      version: null,
      latest_version: null,
      update_available: null,
      check_error: null,
      install_supported: false,
      upgrade_supported: false,
      launch_mode: provider.available ? "installed" : "unavailable",
    },
  };
}

/** One typed macro slot as `list_skills` reports it (core `SlotDef`, Agent Scenes vocabulary). */
export interface MacroSlotInfo {
  id: string;
  label?: string;
  kind?: "text" | "multiline" | "select" | "file" | "artifact";
  options?: string[];
  required?: boolean;
  default?: string;
}

export interface SkillInfo {
  preview?: string | null;
  id: string;
  name: string;
  description: string;
  icon: string | null;
  kind: string;
  /// Harness display name ("Claude Code" …) for skills auto-discovered from a product's skill
  /// directory; null for library skills.
  source: string | null;
  /// Macro payload metadata so the `/` picker can render the R1 slot card without a second
  /// fetch. Absent for every other kind (and for backends predating the fields).
  macro_template?: string | null;
  macro_slots?: MacroSlotInfo[] | null;
}

export type PermissionMode = "ask" | "accept_edits" | "yolo";
export type Sandbox = "read_only" | "workspace_write" | "danger_full_access";

export interface ExecutionPolicy {
  mode: PermissionMode;
  sandbox: Sandbox;
}

export interface Automation {
  id: string;
  name: string;
  prompt: string;
  project_path: string;
  provider: string | { custom: string };
  cron: string;
  timezone: string;
  enabled: boolean;
  use_worktree: boolean;
  permission_mode: PermissionMode;
  sandbox_policy: Sandbox;
  next_run_at: number | null;
  last_run_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface AutomationInput {
  name: string;
  prompt: string;
  projectPath: string;
  provider: string | { custom: string };
  cron: string;
  timezone: string;
  enabled: boolean;
  useWorktree: boolean;
  permissionMode: PermissionMode;
  sandboxPolicy: Sandbox;
}

export type AutomationRunStatus =
  | "starting"
  | "running"
  | "needs_attention"
  | "succeeded"
  | "failed"
  | "interrupted";

export interface AutomationRun {
  id: string;
  automation_id: string;
  session_id: string | null;
  status: AutomationRunStatus;
  scheduled_for: number;
  started_at: number;
  finished_at: number | null;
  error: string | null;
  /** The instruction as recorded when the run started; replayed by `rerunAutomation`. */
  prompt: string;
}

export interface AutomationAlert {
  automation_id: string;
  run_id: string;
  automation_name: string;
  status: AutomationRunStatus;
  error: string | null;
}

export interface SessionInfo {
  id: string;
  title: string;
  title_origin: "default" | "automatic" | "manual";
  /** Pinned sessions stay above the active recency list for their project. */
  pinned: boolean;
  /** App-lifetime sessions used by Quick Chat and Side Chat. They never appear in history/search. */
  transient?: boolean;
  provider: string | { custom: string };
  model: string | null;
  cwd: string;
  worktree_path: string | null;
  /** Original selected project directory; `cwd` may instead point into an isolated worktree. */
  project_path: string | null;
  /** Exact local ref + commit used to create the isolated checkout. */
  worktree_baseline?: ResolvedWorktreeBaseline | null;
  /** Opaque persisted filesystem identity. Its presence distinguishes strongly verified rows. */
  worktree_identity?: Record<string, unknown> | null;
  /** True once the user discarded this session's isolated checkout. Old rows read as false. */
  worktree_discarded?: boolean;
  permission_mode: PermissionMode;
  sandbox_policy: Sandbox;
  acp_session_id: string | null;
  memory_read: MemoryAccess;
  memory_write: MemoryAccess;
  created_at: number;
  /** Last accepted prompt or explicit unarchive; absent on older Core versions. */
  last_active_at?: number;
  /** Core-owned, revisioned run/input state; survives renderer and remote reconnects. */
  activity?: SessionActivity;
  /** Set on sessions owned by a paired remote C2 server; absent for this machine's own. */
  environment_id?: string | null;
}

export type PendingInputKind = "permission" | "elicitation";

/** What a client renders for one elicitation field (core: `ElicitationFieldKind`). */
export type ElicitationFieldKind =
  | "text"
  | "number"
  | "integer"
  | "boolean"
  | "select"
  | "multi_select";

export interface ElicitationOption {
  /** What travels back to the agent. */
  value: string;
  label: string;
  description?: string | null;
  /** Longer content (mockups, snippets) shown while the option is focused. */
  preview?: string | null;
}

export interface ElicitationField {
  key: string;
  kind: ElicitationFieldKind;
  title?: string | null;
  description?: string | null;
  required: boolean;
  options?: ElicitationOption[];
  /** Set when this is the free-text "Other" box belonging to the named field. */
  custom_answer_for?: string | null;
}

/**
 * A structured question from the agent (ACP `elicitation/create`) — Claude Code's
 * `AskUserQuestion`, or an MCP form elicitation. The core normalizes the JSON Schema so every
 * frontend renders the same shape.
 */
export interface ElicitationForm {
  message: string;
  tool_call_id?: string | null;
  fields: ElicitationField[];
}

/** Content values the core accepts back; anything else is dropped when the answer is sanitized. */
export type ElicitationContent = Record<
  string,
  string | string[] | number | boolean
>;

export type ElicitationAnswer =
  | { action: "accept"; content: ElicitationContent }
  | { action: "decline" }
  | { action: "cancel" };

export type PermissionContextKind =
  | "acp"
  | "mcp_elicitation"
  | "website_access"
  | "sensitive_web_action"
  | "computer_use_application"
  | "sites_mutation"
  | "sites_production";

export interface PermissionContext {
  kind: PermissionContextKind;
  server?: string | null;
  tool?: string | null;
  origin?: string | null;
  risk?: string | null;
  application?: string | null;
}

export interface PendingInput {
  input_id: string;
  kind: PendingInputKind;
  title: string;
  options: [string, string][];
  sequence: number;
  context?: PermissionContext;
  /** Present on `elicitation` inputs: the question to render instead of an approval prompt. */
  form?: ElicitationForm | null;
}

export type RunFailureReason = "provider_error" | "interrupted";

export type SessionRunState =
  | { kind: "idle" }
  | { kind: "running"; turn_id: string; prompt_request_id?: string | null }
  | {
      kind: "awaiting_input";
      turn_id: string;
      prompt_request_id?: string | null;
      pending: PendingInput[];
    }
  | {
      kind: "failed";
      turn_id?: string | null;
      reason: RunFailureReason;
      message: string;
    };

export interface SessionActivity {
  revision: number;
  state: SessionRunState;
}

export type WorktreeBaselineKind = "current" | "origin_default";
export type ProjectWorktreeMode = "local" | WorktreeBaselineKind;

export interface ResolvedWorktreeBaseline {
  kind: WorktreeBaselineKind;
  ref: string;
  sha: string;
  display: string;
}

export interface WorktreeBaselineOption {
  kind: WorktreeBaselineKind;
  resolved: ResolvedWorktreeBaseline | null;
  unavailable_reason: string | null;
}

export interface WorktreeSettings {
  root?: string;
  fetch_upstream: boolean;
  auto_delete: boolean;
  auto_delete_limit: number;
}

/** What a discard actually removed. A repeat discard is a no-op success with both fields empty. */
export interface DiscardedWorktree {
  removed_checkout: boolean;
  deleted_branch?: string;
}

/// session = a checkout some session record still claims; orphan = git still registers a codetwo/
/// branch checkout no session claims; stale = leftover directory git no longer registers at all.
export type WorktreeEntryKind = "session" | "orphan" | "stale";

export interface WorktreeStatusEntry {
  path: string;
  branch?: string;
  kind: WorktreeEntryKind;
  registered: boolean;
  checkout_present: boolean;
  session_id?: string;
  session_title?: string;
  session_archived: boolean;
  worktree_discarded: boolean;
}

export type MemoryAccess = "inherit" | "allow" | "deny";

/// A workspace the user works in. The path is the identity — one directory is one project.
export interface Project {
  path: string;
  name: string;
  last_opened_at: number;
  /** Null follows the current draft/session; local is an explicit no-worktree default. */
  default_worktree_mode: ProjectWorktreeMode | null;
  /** Icons are fetched separately so refreshing the project list never serializes image bytes. */
  has_icon?: boolean;
  icon_updated_at?: number;
  /** Null keeps the current provider and lets it choose its own model. */
  default_provider?: string | null;
  default_model?: string | null;
  /** Applied after a newly created session reports its provider-owned effort selector. */
  default_reasoning_effort?: string | null;
}

export interface ProjectIconData {
  mime_type: "image/png" | "image/jpeg" | "image/webp";
  bytes: Uint8Array;
}

export interface MemorySettings {
  enabled: boolean;
  capture: boolean;
  inject: boolean;
  include_external_context: boolean;
}

export type MemoryPolicyValue = "inherit" | "allow" | "deny";

export interface MemoryProjectPolicy {
  project_path: string;
  capture: MemoryPolicyValue;
  inject: MemoryPolicyValue;
  include_external_context: MemoryPolicyValue;
}

export interface MemorySourceRef {
  session_id: string;
  part_seq: number;
}

export interface MemoryRecord {
  id: string;
  project_path: string;
  session_id: string | null;
  layer: "L0" | "L1" | "L2" | "L3";
  category: string;
  content: string;
  confidence: number;
  sources: MemorySourceRef[];
  pinned: boolean;
  active: boolean;
  created_at: number;
  updated_at: number;
  accessed_at: number | null;
  access_count: number;
  origin: "manual" | "automatic" | "user_correction" | "profile";
  forgotten_at: number | null;
  supersedes_id: string | null;
  conflict_with_id: string | null;
  conflict_reason: string | null;
  relevance: number | null;
  editable: boolean;
}

export interface MemoryStats {
  l0: number;
  l1: number;
  l2: number;
  l3: number;
  pending: number;
  active: number;
  pinned: number;
  recent: number;
  forgotten: number;
  conflicts: number;
}

export interface MemoryEvidence {
  session_id: string;
  session_title: string;
  part_seq: number;
  created_at: number;
  excerpt: string;
  available: boolean;
  redacted: boolean;
}

export interface MemoryUsage {
  session_id: string;
  session_title: string;
  user_part_seq: number;
  created_at: number;
}

export interface MemoryReceiptItem {
  id: string;
  layer: "L0" | "L1" | "L2" | "L3";
  category: string;
  content: string;
  source: MemorySourceRef | null;
  relevance: number | null;
}

export interface MemoryReceipt {
  session_id: string;
  user_part_seq: number;
  estimated_tokens: number;
  items: MemoryReceiptItem[];
  created_at: number;
}

/// One model a session can run on. ACP's model API is UNSTABLE and most adapters don't report a
/// list, so these usually come from the core's built-in list for the provider instead.
export interface ModelChoice {
  id: string;
  name: string;
  description: string | null;
}

/// One session config selector the agent reports (ACP config options — the newer surface where
/// current adapters put the model picker and the reasoning/thought level).
export interface ConfigOptionInfo {
  id: string;
  name: string;
  /// Semantic hint from the spec: "model" | "mode" | "thought_level" | anything else.
  category: string | null;
  current: string;
  choices: ModelChoice[];
}

export interface SessionInteractionCapabilities {
  steering: boolean;
  /** Set only after the live ACP session advertises its native `/compact` command. */
  compact_context: boolean;
}

/// Neutral document shape the editor serializes into; matches core `DocBlock` serde.
export type DocBlock =
  | { type: "text"; text: string }
  | { type: "skill"; skill_id: string; params: Record<string, string> }
  | { type: "file"; path: string }
  | { type: "image"; path: string }
  | { type: "appshot"; id: string; title?: string }
  | { type: "attachment"; id: string; name?: string }
  | {
      type: "canvas";
      id: string;
      frozen_revision: number;
      pixel_policy?: CanvasPixelPolicy;
    }
  | { type: "session"; session_id: string; through_seq?: number }
  // R12: a referenced issue-tracker item with its snapshot embedded at insert time; mirrors core
  // `DocBlock::Issue`, which re-renders `issues::Issue::to_context` from exactly these fields.
  | {
      type: "issue";
      source: string;
      id: string;
      title: string;
      url: string;
      body: string;
    };

/// One-line description of a doc block, used for summaries and browser-mode previews.
export function describeBlock(b: DocBlock): string {
  switch (b.type) {
    case "text": {
      return b.text;
    }
    case "skill": {
      return `[skill:${b.skill_id}]`;
    }
    case "file": {
      return `[@${b.path}]`;
    }
    case "image": {
      return `[img:${b.path}]`;
    }
    case "appshot": {
      return `[appshot:${b.title ?? b.id}]`;
    }
    case "attachment": {
      return `[image:${b.name ?? b.id}]`;
    }
    case "canvas": {
      return `[canvas:${b.id}@${b.frozen_revision}]`;
    }
    case "session": {
      return `[chat:${b.session_id.slice(0, 8)}]`;
    }
    case "issue": {
      return `[issue:${b.source}#${b.id}]`;
    }
  }
}

/// Mirrors core `Event` (tagged by `event`, snake_case).
export type CoreEvent =
  | {
      event: "task_snapshot_changed";
      session: null;
      task_id: string;
      revision: number;
    }
  | {
      event: "session_created";
      session: string;
      cwd?: string;
      project_path?: string | null;
      worktree_path?: string | null;
      worktree_baseline?: ResolvedWorktreeBaseline | null;
      request_id?: string | null;
    }
  | { event: "memory_context"; session: string; receipt: MemoryReceipt }
  | { event: "session_title_changed"; session: string; title: string }
  | {
      event: "provider_changed";
      session: string;
      provider: string | { custom: string };
      model?: string | null;
    }
  | {
      event: "worktree_discarded";
      session: string;
      worktree_path: string;
      removed_checkout: boolean;
      deleted_branch?: string;
    }
  | {
      event: "session_activity_changed";
      session: string;
      activity: SessionActivity;
    }
  | {
      event: "turn_started";
      session: string;
      request_id?: string | null;
      transcript_seq?: number | null;
    }
  | {
      event: "agent_text";
      session: string;
      message_id: string;
      text: string;
      transcript_seq?: number | null;
    }
  | {
      event: "agent_thought";
      session: string;
      text: string;
      transcript_seq?: number | null;
    }
  | {
      event: "tool_call";
      session: string;
      id: string;
      title: string;
      status: string;
      kind?: string | null;
      agent_input?: unknown;
      outputs?: ToolOutput[];
      transcript_seq?: number | null;
    }
  | {
      event: "permission_request";
      session: string;
      request_id: string;
      title: string;
      options: [string, string][];
      context?: PermissionContext;
    }
  | {
      event: "elicitation_request";
      session: string;
      request_id: string;
      form: ElicitationForm;
    }
  | {
      event: "usage";
      session: string;
      input_tokens: number;
      output_tokens: number;
    }
  | {
      event: "context_window";
      session: string;
      used_tokens: number;
      context_window: number;
      cost_usd?: number | null;
      breakdown?: { id: string; tokens: number }[] | null;
    }
  | {
      event: "models";
      session: string;
      available: ModelChoice[];
      current: string;
    }
  | { event: "config_options"; session: string; options: ConfigOptionInfo[] }
  | {
      event: "session_capabilities";
      session: string;
      steering: boolean;
      compact_context?: boolean;
    }
  | {
      event: "prompt_queued";
      session: string;
      request_id?: string | null;
      position: number;
    }
  | {
      event: "steer_accepted";
      session: string;
      request_id?: string | null;
      transcript_seq?: number | null;
      outcome: "injected" | "startedNewTurn";
    }
  | {
      event: "execution_policy_changed";
      session: string;
      policy: ExecutionPolicy;
      request_id?: string | null;
    }
  | { event: "turn_ended"; session: string; stop_reason: string }
  | {
      event: "error";
      session: string | null;
      message: string;
      terminal: boolean;
      request_id?: string | null;
    }
  | {
      event: "test_signal";
      session: string;
      tool_call_id: string;
      command: string;
      passed: boolean;
      exit_code?: number | null;
    }
  | {
      event: "artifact_produced";
      session: string;
      scene_ref: string;
      artifact_key: string;
      kind: string;
      version: number;
      record_id: number;
    }
  | {
      event: "exit_criteria_met";
      session: string;
      scene_ref: string;
      satisfied: string[];
      unverified: string[];
      state_key: string;
    }
  | {
      event: "hook_suggestion";
      session: string;
      scene_ref: string;
      on: string;
      kind: string;
      target_scene?: string | null;
      carry?: string[];
      message?: string | null;
      pipeline_instance?: string | null;
      to_stage?: string | null;
      state_key: string;
    }
  | {
      event: "hook_turn_started";
      session: string;
      scene_ref: string;
      macro_id: string;
    }
  | {
      event: "session_cost";
      session: string;
      input_tokens: number;
      output_tokens: number;
      cost_usd?: number | null;
      burn_rate_usd_per_hour?: number | null;
      priced: boolean;
    };

export interface PtyOutput {
  id: string;
  data: string;
  project_path: string | null;
}

/** The title the child set (OSC 0/2), or its working directory (OSC 7). */
export interface PtyTitle {
  id: string;
  title: string;
  project_path: string | null;
}

export interface PtyExit {
  id: string;
  project_path: string | null;
}

export interface PtyAttach {
  /** False when we re-attached to a terminal that was already running. */
  created: boolean;
  /** VT sequences reproducing the terminal's scrollback, screen, and cursor. */
  restore: string;
}

export type Part =
  | { kind: "text"; text: string }
  | { kind: "prompt"; text: string; display: string }
  | { kind: "reasoning"; text: string }
  | {
      kind: "tool_call";
      id: string;
      title: string;
      status: string;
      tool_kind?: string | null;
      agent_input?: unknown;
      outputs?: ToolOutput[];
    }
  | { kind: "plan"; entries: unknown[] };

export interface ArtifactRef {
  id: string;
  mime_type: string;
  bytes: number;
  width: number;
  height: number;
  display_name: string;
}

export type ToolOutput =
  | { type: "text"; text: string }
  | { type: "image"; artifact: ArtifactRef }
  | {
      type: "resource_link";
      name: string;
      uri: string;
      mime_type?: string | null;
    };

/** One durable transcript row. `seq` is stable within a session and orders snapshot/live merge. */
export interface TranscriptEntry {
  seq: number;
  role: "user" | "agent";
  part: Part;
  created_at?: number;
  started_at?: number;
}

/** A page never begins in the middle of a user turn. `next_before` is exclusive. */
export interface TranscriptPage {
  entries: TranscriptEntry[];
  next_before: number | null;
  snapshot_through: number | null;
}

export type SkillPayload =
  | { kind: "fragment"; text: string }
  // Core's untagged slot deserializer accepts bare legacy ids and full slot objects (R2).
  | { kind: "macro"; template: string; slots: (string | MacroSlotInfo)[] };

export interface Skill {
  id: string;
  name: string;
  description: string;
  icon: string | null;
  source?: string | null;
  payload: SkillPayload;
}

// True only inside the Electrobun webview; Vite preview still runs in a plain browser.
const inDesktop = isElectrobun;

/** False in the plain Vite renderer (`bun run dev:renderer`), where native commands do not exist. */
export const isDesktop = inDesktop;

let callProjectPath: string | null = null;

/** Route subsequent extension calls through the active project's command realm. */
export function setCallProjectPath(path: string | null): void {
  callProjectPath = path;
}

const BROWSER_DOCKER_CONTAINERS = [
  {
    id: "8f4c2e9133ef",
    name: "api",
    image: "ghcr.io/codetwo/api:latest",
    command: '"bun run start"',
    createdAt: "2026-08-25 19:21:04 +0800 SGT",
    runningFor: "22 hours ago",
    ports: "0.0.0.0:8080->8080/tcp",
    state: "running",
    status: "Up 22 hours",
    size: "12.4kB (virtual 286MB)",
    labels: null,
    localVolumes: 0,
    mounts: "",
    networks: "bridge",
  },
  {
    id: "b497581dd054",
    name: "worker",
    image: "ghcr.io/codetwo/worker:latest",
    command: '"bun run worker"',
    createdAt: "2026-08-24 08:12:01 +0800 SGT",
    runningFor: "2 days ago",
    ports: "",
    state: "exited",
    status: "Exited (0) 18 hours ago",
    size: "8.2kB (virtual 284MB)",
    labels: null,
    localVolumes: 0,
    mounts: "",
    networks: "bridge",
  },
  {
    id: "24c2830d260a",
    name: "postgres",
    image: "postgres:16-alpine",
    command: '"docker-entrypoint.s…"',
    createdAt: "2026-08-24 08:11:40 +0800 SGT",
    runningFor: "2 days ago",
    ports: "0.0.0.0:5432->5432/tcp",
    state: "exited",
    status: "Exited (0) 18 hours ago",
    size: "63B (virtual 247MB)",
    labels: null,
    localVolumes: 1,
    mounts: "codetwo_pgdata",
    networks: "bridge",
  },
];

const BROWSER_DOCKER_IMAGES = [
  {
    id: "sha256:19a3a8c9d0d8",
    repository: "ghcr.io/codetwo/api",
    tag: "latest",
    digest: "sha256:6ec4ca4b0d1e",
    createdAt: "2026-08-25 18:42:11 +0800 SGT",
    createdSince: "22 hours ago",
    size: "286MB",
    sharedSize: "0B",
    uniqueSize: "286MB",
    containers: 1,
  },
  {
    id: "sha256:f7eb244d02c9",
    repository: "ghcr.io/codetwo/worker",
    tag: "latest",
    digest: "sha256:f79edcb4a1ef",
    createdAt: "2026-08-24 07:55:03 +0800 SGT",
    createdSince: "2 days ago",
    size: "284MB",
    sharedSize: "0B",
    uniqueSize: "284MB",
    containers: 1,
  },
  {
    id: "sha256:34f2dfe3bb89",
    repository: "postgres",
    tag: "16-alpine",
    digest: "sha256:3e89afe3d2c2",
    createdAt: "2026-08-18 05:10:02 +0800 SGT",
    createdSince: "8 days ago",
    size: "247MB",
    sharedSize: "0B",
    uniqueSize: "247MB",
    containers: 1,
  },
];

function browserDockerCall<T>(name: string, rawArgs: unknown): T {
  const args =
    rawArgs != null && typeof rawArgs === "object" && !Array.isArray(rawArgs)
      ? (rawArgs as Record<string, unknown>)
      : {};
  const container =
    typeof args.container === "string" ? args.container : "container";
  const image = typeof args.image === "string" ? args.image : "image";
  switch (name) {
    case "docker.status": {
      return {
        available: true,
        clientVersion: "29.7.2",
        serverVersion: "29.7.2",
        context: "desktop-linux",
        engine: {
          name: "docker-desktop",
          operatingSystem: "Docker Desktop",
          architecture: "aarch64",
          cpus: 10,
          memoryBytes: 8_589_934_592,
          dockerRootDir: "/var/lib/docker",
        },
        containers: { total: 21, running: 1, paused: 0, stopped: 20 },
        images: 12,
        message:
          "Docker 29.7.2 is running · 1 running · 20 stopped · 12 images",
      } as T;
    }
    case "docker.containers": {
      return {
        containers: BROWSER_DOCKER_CONTAINERS,
        count: BROWSER_DOCKER_CONTAINERS.length,
        truncated: false,
        message: `${BROWSER_DOCKER_CONTAINERS.length} Docker containers.`,
      } as T;
    }
    case "docker.images": {
      return {
        images: BROWSER_DOCKER_IMAGES,
        count: BROWSER_DOCKER_IMAGES.length,
        truncated: false,
        message: `${BROWSER_DOCKER_IMAGES.length} Docker images.`,
      } as T;
    }
    case "docker.inspect": {
      return {
        container,
        details: {
          Id:
            BROWSER_DOCKER_CONTAINERS.find((item) => item.name === container)
              ?.id ?? container,
          Name: `/${container}`,
          State: { Status: container === "api" ? "running" : "exited" },
          Config: {
            Image: BROWSER_DOCKER_CONTAINERS.find(
              (item) => item.name === container
            )?.image,
          },
        },
        message: `Inspected ${container}.`,
      } as T;
    }
    case "docker.logs": {
      return {
        container,
        stdout: `[2026-08-26T10:31:04Z] ${container} ready\n[2026-08-26T10:31:08Z] GET /health 200`,
        stderr: "",
        message: `Read logs from ${container}.`,
      } as T;
    }
    case "docker.start":
    case "docker.stop":
    case "docker.restart": {
      return {
        container,
        action: name.slice("docker.".length),
        output: container,
        message: `${name} ${container}`,
      } as T;
    }
    case "docker.pull": {
      return {
        image,
        output: `Downloaded newer image for ${image}`,
        message: `Pulled ${image}.`,
      } as T;
    }
    case "docker.remove_image": {
      return {
        image,
        output: `Untagged: ${image}`,
        message: `Removed ${image}.`,
      } as T;
    }
    default: {
      throw new Error(
        `plugin command "${name}" is unavailable outside the desktop app`
      );
    }
  }
}

// ---- the plugin graph -------------------------------------------------------------------------

/**
 * Call a command through the selected Core transport — `call("git.status", { cwd })`.
 *
 * The desktop adapter carries the full trusted host surface. Paired Web mode keeps the same typed
 * product projection but the remote host enforces a smaller allowlist. A runtime module that
 * registers `foo.bar` remains callable without adding a command-specific renderer RPC method.
 */
export async function call<T = unknown>(
  name: string,
  args?: unknown,
  projectPath: string | null = callProjectPath
): Promise<T> {
  if (!coreAvailable) return browserDockerCall<T>(name, args);
  return await coreCall<T>(name, args ?? null, projectPath);
}

/** Lifecycle state of one plugin instance, as the kernel reports it. */
export type PluginStatus =
  | "pending"
  | "loading"
  | "active"
  | "failed"
  | "disposed";

/** One plugin instance in the running graph. */
export interface PluginScope {
  id: number;
  parent: number | null;
  plugin: string;
  status: PluginStatus;
  error: string | null;
  inject: { required: string[]; optional: string[] };
  /** Injected services that are missing — why a `pending` plugin is pending. */
  missing: string[];
  services: string[];
  commands: string[];
  config: unknown;
}

export interface PluginCommand {
  name: string;
  plugin: string;
  scope: number;
  description: string | null;
}

/** Everything currently loaded, why it is in that state, and what it contributed. */
export async function kernelScopes(): Promise<PluginScope[]> {
  return inDesktop ? await call<PluginScope[]>("kernel.scopes") : [];
}

/** Every command the running graph offers, with the plugin that owns it. */
export async function kernelCommands(): Promise<PluginCommand[]> {
  return inDesktop ? await call<PluginCommand[]>("kernel.commands") : [];
}

/** One installable plugin: whether it runs, its config, and the schema to render a form from. */
export interface PluginEntry {
  name: string;
  description: string | null;
  enabled: boolean;
  running: boolean;
  status: PluginStatus | null;
  config: unknown;
  schema: unknown;
  /** Registered but absent from the config — installable, not installed. */
  available: boolean;
}

export async function listCorePlugins(): Promise<PluginEntry[]> {
  return inDesktop ? await call<PluginEntry[]>("kernel.plugins") : [];
}

// ---- unified plugin management ---------------------------------------------------------------

/** Configuration scope exposed to renderer code. Rust receives the same tagged enum in snake_case. */
export type ManagedPluginScope =
  | { kind: "user" }
  | { kind: "project"; projectPath: string };

export type ManagedPluginOverride = "inherit" | "enabled" | "disabled";
export type ManagedPluginOrigin = "built_in" | "host" | "third_party";
export type ManagedPluginRole = "core" | "built_in" | "extension";
export type ManagedPluginCategory =
  | "foundation"
  | "workspace"
  | "automation"
  | "developer_tools"
  | "interface"
  | "integration"
  | "other";
export type ManagedPluginScopeSupport = "user" | "project";

export interface ManagedPluginMetadata {
  origin: ManagedPluginOrigin;
  role: ManagedPluginRole;
  category: ManagedPluginCategory;
  scope_support: ManagedPluginScopeSupport[];
  essential: boolean;
  default_enabled: boolean;
}

export interface ManagedPluginDependencies {
  required: string[];
  optional: string[];
}

export interface EffectivePluginState {
  effective_enabled: boolean;
  state: ManagedPluginOverride;
  status: PluginStatus | "disabled";
  missing: string[];
  error: string | null;
  reason: string | null;
}

export type PluginProcessObservation =
  | { phase: "dormant" | "starting" | "running" }
  | { phase: "failed"; error: string };

export interface ManagedPluginCatalogEntry {
  process?: PluginProcessObservation | null;
  effective_state?: EffectivePluginState;
  effective_components?: Record<string, boolean>;
  id: string;
  description: string | null;
  metadata: ManagedPluginMetadata;
  dependencies: ManagedPluginDependencies;
  /** Current scope policy. Older hosts omit this, so adapters must tolerate undefined. */
  state?: ManagedPluginOverride;
  enabled: boolean;
  running: boolean;
  status: PluginStatus | null;
  missing: string[];
  error: string | null;
  config: unknown;
  schema: unknown;
  available: boolean;
  components: Record<string, ManagedPluginOverride>;
  commands?: string[];
  services?: string[];
}

export type ManagedPluginRecovery =
  | { kind: "normal" }
  | { kind: "restored_last_good"; error: string }
  | { kind: "safe_mode"; error: string };

export interface ManagedPluginCatalog {
  bundle_states?: Record<string, EffectivePluginState>;
  graph_revision: number;
  config_revision: number;
  recovery: ManagedPluginRecovery;
  plugins: ManagedPluginCatalogEntry[];
}

export interface ManagedPluginChangeRequest {
  plugin: string;
  scope: ManagedPluginScope;
  state?: ManagedPluginOverride;
  config?: unknown;
  component?: string;
}

export interface ManagedPluginActiveResource {
  plugin: string;
  kind: string;
  id: string;
  label: string;
}

export interface ManagedPluginChangePlan {
  id: string;
  graph_revision: number;
  config_revision: number;
  request: ManagedPluginChangeRequest;
  affected: string[];
  active_resources: ManagedPluginActiveResource[];
  requires_confirmation: boolean;
}

export interface ManagedPluginChangeResult {
  graph_revision: number;
  config_revision: number;
  affected: string[];
}

type ManagedPluginScopeWire =
  | { kind: "user" }
  | { kind: "project"; project_path: string };

type ManagedPluginChangePlanWire = Omit<ManagedPluginChangePlan, "request"> & {
  request: Omit<ManagedPluginChangeRequest, "scope"> & {
    scope: ManagedPluginScopeWire;
  };
};

/** Kept public for adapter/contract tests and to make the camelCase-to-serde boundary explicit. */
export function managedPluginScopeToWire(
  scope: ManagedPluginScope
): ManagedPluginScopeWire {
  return scope.kind === "user"
    ? { kind: "user" }
    : { kind: "project", project_path: scope.projectPath };
}

function managedPluginScopeFromWire(
  scope: ManagedPluginScopeWire
): ManagedPluginScope {
  return scope.kind === "user"
    ? { kind: "user" }
    : { kind: "project", projectPath: scope.project_path };
}

function managedPluginPlanFromWire(
  plan: ManagedPluginChangePlanWire
): ManagedPluginChangePlan {
  return {
    ...plan,
    request: {
      ...plan.request,
      scope: managedPluginScopeFromWire(plan.request.scope),
    },
  };
}

const EMPTY_MANAGED_PLUGIN_CATALOG: ManagedPluginCatalog = {
  graph_revision: 0,
  config_revision: 0,
  recovery: { kind: "normal" },
  plugins: [],
};

/** Read the effective catalog for one user or project scope. */
export async function pluginCatalog(
  scope: ManagedPluginScope
): Promise<ManagedPluginCatalog> {
  if (!inDesktop) return EMPTY_MANAGED_PLUGIN_CATALOG;
  return await call<ManagedPluginCatalog>(
    "plugins.catalog",
    { scope: managedPluginScopeToWire(scope) },
    null
  );
}

export interface PluginSnapshot {
  bundles: PluginInfo[];
  catalogs: { scope: ManagedPluginScope; catalog: ManagedPluginCatalog }[];
}

/** One inventory-consistent snapshot; the host always includes the user catalog. */
export async function pluginSnapshot(
  scopes: ManagedPluginScope[]
): Promise<PluginSnapshot> {
  if (!inDesktop) {
    return {
      bundles: await listPlugins(),
      catalogs: [
        { kind: "user" } as ManagedPluginScope,
        ...scopes.filter((scope) => scope.kind !== "user"),
      ].map((scope) => ({ scope, catalog: EMPTY_MANAGED_PLUGIN_CATALOG })),
    };
  }
  const wire = await call<{
    bundles: PluginInfo[];
    catalogs: {
      scope: ManagedPluginScopeWire;
      catalog: ManagedPluginCatalog;
    }[];
  }>(
    "plugins.snapshot",
    { scopes: scopes.map(managedPluginScopeToWire) },
    null
  );
  return {
    bundles: wire.bundles,
    catalogs: wire.catalogs.map(({ scope, catalog }) => ({
      scope: managedPluginScopeFromWire(scope),
      catalog,
    })),
  };
}

/** Stage a revision-bound change. The returned id is the only value accepted by apply. */
export async function planPluginChange(
  request: ManagedPluginChangeRequest
): Promise<ManagedPluginChangePlan> {
  const wire = await call<ManagedPluginChangePlanWire>(
    "plugins.plan_change",
    { ...request, scope: managedPluginScopeToWire(request.scope) },
    null
  );
  return managedPluginPlanFromWire(wire);
}

/** Apply exactly one previously planned change and wait for the graph to settle. */
export async function applyPluginChange(
  id: string
): Promise<ManagedPluginChangeResult> {
  return await call<ManagedPluginChangeResult>(
    "plugins.apply_change",
    { id },
    null
  );
}

/** Clear a plugin's state, configuration, and component overrides in one scope. */
export async function resetManagedPlugin(
  plugin: string,
  scope: ManagedPluginScope
): Promise<ManagedPluginChangeResult> {
  return await call<ManagedPluginChangeResult>(
    "plugins.reset",
    { plugin, scope: managedPluginScopeToWire(scope) },
    null
  );
}

/**
 * Installed bundles that ship a process: which have a ready host adapter, and which are installed
 * but waiting for trust. A ready lazy adapter does not imply that its child process is resident.
 */
export async function listExtensions(): Promise<{
  ready: string[];
  untrusted: string[];
}> {
  if (!inDesktop) return { ready: [], untrusted: [] };
  return await call("extensions.list");
}

export interface PluginReloadRecord {
  at: number;
  plugins: string[];
  success: boolean;
  error?: string | null;
}

export interface PluginDeveloperStatus {
  enabled: boolean;
  watching: boolean;
  plugins_dir: string;
  last_reload: PluginReloadRecord | null;
}

const FALLBACK_PLUGIN_DEVELOPER_STATUS: PluginDeveloperStatus = {
  enabled: false,
  watching: false,
  plugins_dir: "",
  last_reload: null,
};

export async function getPluginDeveloperStatus(): Promise<PluginDeveloperStatus> {
  return inDesktop
    ? await call<PluginDeveloperStatus>(
        "plugins.developer_status",
        undefined,
        null
      )
    : { ...FALLBACK_PLUGIN_DEVELOPER_STATUS };
}

export async function setPluginDeveloperMode(
  enabled: boolean
): Promise<PluginDeveloperStatus> {
  if (!inDesktop)
    throw new Error("Plugin development requires the C2 desktop app.");
  return await call<PluginDeveloperStatus>(
    "plugins.set_developer_mode",
    { enabled },
    null
  );
}

export async function reloadDevelopmentPlugins(): Promise<PluginDeveloperStatus> {
  if (!inDesktop) throw new Error("Plugin reload requires the C2 desktop app.");
  return await call<PluginDeveloperStatus>(
    "plugins.reload_development",
    undefined,
    null
  );
}

const FALLBACK_SKILLS: SkillInfo[] = [
  {
    id: "reviewer",
    name: "Code Reviewer",
    description: "Meticulous reviewer",
    icon: null,
    kind: "fragment",
    source: null,
  },
  {
    id: "test-writer",
    name: "Test Writer",
    description: "Thorough tests",
    icon: null,
    kind: "fragment",
    source: null,
  },
  {
    id: "security-audit",
    name: "Security Audit",
    description: "Find vulns",
    icon: null,
    kind: "fragment",
    source: null,
  },
  {
    id: "commit-macro",
    name: "Commit Message",
    description: "Commit macro",
    icon: null,
    kind: "macro",
    source: null,
    macro_template:
      "Write a {{style}} commit message for changes to {{scope}}.",
    macro_slots: [
      {
        id: "style",
        label: "Style",
        kind: "select",
        options: ["conventional", "descriptive"],
        required: true,
      },
      { id: "scope", label: "Scope", kind: "text" },
    ],
  },
  {
    id: "demo:skill:review",
    name: "Release Review",
    description: "Review a release against its acceptance criteria",
    icon: null,
    kind: "agent_skill",
    source: "Plugin · Developer Toolkit",
  },
  {
    id: "demo:agent:research",
    name: "Researcher",
    description: "Collect primary evidence before implementation",
    icon: null,
    kind: "subagent",
    source: "Plugin · Developer Toolkit",
  },
  {
    id: "demo:mcp:docs",
    name: "docs-search",
    description: "MCP server from Developer Toolkit",
    icon: null,
    kind: "mcp",
    source: "Plugin · Developer Toolkit",
  },
];

export async function listProviders(
  checkUpdates = false
): Promise<ProviderInfo[]> {
  const providers = coreAvailable
    ? await call<ProviderInfoWire[]>("providers.list", {
        check_updates: checkUpdates,
      })
    : [];
  return providers.map(normalizeProviderInfo);
}

export async function setProviderEnabled(
  provider: string,
  enabled: boolean
): Promise<ProviderInfo[]> {
  if (!inDesktop)
    throw new Error("Provider management is available in the C2 desktop app");
  const providers = await call<ProviderInfoWire[]>("providers.set_enabled", {
    provider,
    enabled,
  });
  return providers.map(normalizeProviderInfo);
}

export async function configureProvider(
  provider: string,
  configuration: ProviderRuntimeOverride
): Promise<ProviderInfo[]> {
  if (!inDesktop)
    throw new Error(
      "Provider configuration is available in the C2 desktop app"
    );
  const providers = await call<ProviderInfoWire[]>("providers.configure", {
    provider,
    configuration,
  });
  return providers.map(normalizeProviderInfo);
}

export async function registerCustomProvider(
  configuration: CustomProviderConfiguration
): Promise<void> {
  if (!inDesktop)
    throw new Error(
      "Custom provider registration is available in the C2 desktop app"
    );
  await call<boolean>("providers.register", { configuration });
}

export async function removeCustomProvider(provider: string): Promise<void> {
  if (!inDesktop)
    throw new Error(
      "Custom provider removal is available in the C2 desktop app"
    );
  await call<boolean>("providers.remove", { provider });
}

export async function installProvider(
  provider: string
): Promise<ProviderInfo[]> {
  if (!inDesktop)
    throw new Error("Provider installation is available in the C2 desktop app");
  const providers = await call<ProviderInfoWire[]>("providers.install", {
    provider,
  });
  return providers.map(normalizeProviderInfo);
}

export async function upgradeProvider(
  provider: string
): Promise<ProviderInfo[]> {
  if (!inDesktop)
    throw new Error("Provider upgrades are available in the C2 desktop app");
  const providers = await call<ProviderInfoWire[]>("providers.upgrade", {
    provider,
  });
  return providers.map(normalizeProviderInfo);
}

export async function getComputerUseSettings(): Promise<ComputerUseSettings> {
  if (!inDesktop) {
    return {
      selections: {},
      backends: [
        {
          id: "cua",
          display_name: "Cua Driver",
          available: false,
          reason: "Computer Use backends are discovered by the desktop host.",
          providers: [],
          exclude_providers: [],
        },
      ],
      errors: [],
    };
  }
  return normalizeComputerUseSettings(
    await call<ComputerUseSettingsWire>("computer_use.settings")
  );
}

export async function selectComputerUseBackend(
  backend: string
): Promise<ComputerUseSettings> {
  if (!inDesktop) return await getComputerUseSettings();
  return normalizeComputerUseSettings(
    await call<ComputerUseSettingsWire>("computer_use.select", { backend })
  );
}

export async function getBrowserUseSettings(): Promise<BrowserUseSettings> {
  if (!inDesktop) {
    return {
      access_enabled: false,
      selections: {},
      backends: [
        {
          id: "openai-browser",
          display_name: "OpenAI Browser / Chrome",
          available: false,
          reason: "Browser Use backends are discovered by the desktop host.",
          providers: ["codex"],
          exclude_providers: [],
        },
      ],
      errors: [],
    };
  }
  return normalizeBrowserUseSettings(
    await call<BrowserUseSettingsWire>("browser_use.settings")
  );
}

export async function selectBrowserUseBackend(
  backend: string
): Promise<BrowserUseSettings> {
  if (!inDesktop) return await getBrowserUseSettings();
  return normalizeBrowserUseSettings(
    await call<BrowserUseSettingsWire>("browser_use.select", { backend })
  );
}

export async function setAgentBrowserAccess(
  enabled: boolean
): Promise<BrowserUseSettings> {
  if (!inDesktop) return await getBrowserUseSettings();
  return normalizeBrowserUseSettings(
    await call<BrowserUseSettingsWire>("browser_use.set_access", { enabled })
  );
}

/// Passing a cwd makes the core rescan that workspace's harness skill directories
/// (.claude/skills, .codex/skills, …) before answering.
export async function listSkills(cwd?: string): Promise<SkillInfo[]> {
  return inDesktop
    ? await call<SkillInfo[]>("skills.list", { cwd: cwd ?? null })
    : FALLBACK_SKILLS;
}

export async function listSessions(): Promise<SessionInfo[]> {
  return coreAvailable ? await call<SessionInfo[]>("sessions.list") : [];
}

export interface ImportedSessionSummary {
  id: string;
  title: string;
  source: string;
  messages: number;
  imported: boolean;
}

export interface SessionImportResult {
  files: number;
  imported: number;
  skipped: number;
  failed: number;
  messages: number;
  sessions: ImportedSessionSummary[];
  errors: { path: string; message: string }[];
}

/** Choose provider-owned transcripts or message databases and let Core parse them read-only. */
export async function importSessionFiles(
  fallbackCwd: string
): Promise<SessionImportResult | null> {
  if (!inDesktop) return null;
  const paths = await desktopOpenDialog({
    directory: false,
    multiple: true,
    title: "Import conversations",
    filters: [
      {
        name: "Codex, Claude Code, Cursor, or T3 Code",
        extensions: ["jsonl", "vscdb", "sqlite", "db"],
      },
    ],
  });
  if (paths.length === 0) return null;
  return await call<SessionImportResult>("sessions.import", {
    paths,
    fallback_cwd: fallbackCwd,
  });
}

export async function getMemorySettings(): Promise<MemorySettings> {
  return inDesktop
    ? await call<MemorySettings>("memory.settings")
    : {
        enabled: true,
        capture: true,
        inject: true,
        include_external_context: true,
      };
}

export async function saveMemorySettings(
  settings: MemorySettings
): Promise<void> {
  if (inDesktop) await call("memory.set_settings", { settings });
}

export async function getMemoryProjectPolicy(
  projectPath: string
): Promise<MemoryProjectPolicy> {
  return inDesktop
    ? await call<MemoryProjectPolicy>("memory.project_policy", {
        project_path: projectPath,
      })
    : {
        project_path: projectPath,
        capture: "inherit",
        inject: "inherit",
        include_external_context: "inherit",
      };
}

export async function saveMemoryProjectPolicy(
  projectPath: string,
  policy: MemoryProjectPolicy
): Promise<void> {
  if (inDesktop)
    await call("memory.set_project_policy", {
      project_path: projectPath,
      policy,
    });
}

export async function listMemories(
  projectPath: string,
  limit = 100
): Promise<MemoryRecord[]> {
  return inDesktop
    ? await call<MemoryRecord[]>("memory.list", {
        project_path: projectPath,
        limit,
      })
    : [];
}

export async function listManagedMemories(
  projectPath: string,
  limit = 500
): Promise<MemoryRecord[]> {
  return inDesktop
    ? await call<MemoryRecord[]>("memory.manage_list", {
        project_path: projectPath,
        limit,
      })
    : [];
}

export async function searchMemories(
  projectPath: string,
  query: string,
  limit = 50
): Promise<MemoryRecord[]> {
  return inDesktop
    ? await call<MemoryRecord[]>("memory.search", {
        project_path: projectPath,
        query,
        limit,
      })
    : [];
}

export async function getMemoryStats(
  projectPath: string
): Promise<MemoryStats> {
  return inDesktop
    ? await call<MemoryStats>("memory.stats", { project_path: projectPath })
    : {
        l0: 0,
        l1: 0,
        l2: 0,
        l3: 0,
        pending: 0,
        active: 0,
        pinned: 0,
        recent: 0,
        forgotten: 0,
        conflicts: 0,
      };
}

export async function addMemory(
  projectPath: string,
  category: string,
  content: string,
  pinned = true
): Promise<MemoryRecord> {
  return await call<MemoryRecord>("memory.add", {
    project_path: projectPath,
    category,
    content,
    pinned,
  });
}

export async function setMemoryPinned(
  id: string,
  pinned: boolean
): Promise<void> {
  if (inDesktop) await call("memory.set_pinned", { id, value: pinned });
}

export async function setMemoryActive(
  id: string,
  active: boolean
): Promise<void> {
  if (inDesktop) await call("memory.set_active", { id, value: active });
}

export async function updateMemory(
  id: string,
  category: string,
  content: string
): Promise<MemoryRecord> {
  return await call<MemoryRecord>("memory.update", { id, category, content });
}

export async function setMemoryCategory(
  id: string,
  category: string
): Promise<MemoryRecord> {
  return await call<MemoryRecord>("memory.set_category", { id, category });
}

export async function correctMemory(
  id: string,
  category: string,
  content: string
): Promise<MemoryRecord> {
  return await call<MemoryRecord>("memory.correct", { id, category, content });
}

export async function deleteMemory(id: string): Promise<void> {
  if (inDesktop) await call("memory.delete", { id });
}

export async function getMemoryEvidence(
  id: string,
  reveal = false
): Promise<MemoryEvidence[]> {
  return inDesktop
    ? await call<MemoryEvidence[]>("memory.evidence", { id, reveal })
    : [];
}

export async function getMemoryUsages(id: string): Promise<MemoryUsage[]> {
  return inDesktop ? await call<MemoryUsage[]>("memory.usages", { id }) : [];
}

export async function setSessionMemoryPolicy(
  session: string,
  read: MemoryAccess,
  write: MemoryAccess
): Promise<void> {
  if (inDesktop)
    await call("memory.set_session_policy", { session, read, write });
}

export async function listMemoryReceipts(
  session: string
): Promise<MemoryReceipt[]> {
  return inDesktop
    ? await call<MemoryReceipt[]>("memory.receipts", { session })
    : [];
}

export async function newSession(
  provider: string,
  cwd: string,
  worktreeBase: WorktreeBaselineKind | null,
  requestId: string,
  worktreeBaseSha?: string | null,
  initialPolicy?: ExecutionPolicy | null,
  initialModel?: string | null,
  transient = false,
  initialReasoningEffort?: string | null,
  parallelTask?: { taskId: string; goal: string } | null
): Promise<void> {
  const environmentId = environmentRegistry.activeId();
  if (coreAvailable && environmentId != null) {
    // The session lives on the remote machine, so a local folder or checkout means nothing there.
    if (parallelTask) {
      throw new Error(
        "Parallel tasks run on this machine. Switch the environment to Local."
      );
    }
    const environment = environmentRegistry.get(environmentId);
    const remoteCwd =
      environment?.workspace ??
      (await coreCallEnvironment<string>(
        environmentId,
        "workspace.default_cwd",
        null
      ));
    await call("engine.new_session", {
      provider,
      cwd: remoteCwd,
      use_worktree: false,
      worktree_base: null,
      worktree_base_sha: null,
      request_id: requestId,
      initial_policy: initialPolicy ?? null,
      model: initialModel ?? null,
      transient,
      reasoning_effort: initialReasoningEffort ?? null,
      environment_id: environmentId,
    });
    return;
  }
  if (coreAvailable) {
    if (parallelTask) {
      if (worktreeBase === null) {
        throw new Error("Parallel tasks require an isolated worktree");
      }
      await call("engine.new_parallel_task", {
        provider,
        cwd,
        worktree_base: worktreeBase,
        worktree_base_sha: worktreeBaseSha ?? null,
        request_id: requestId,
        initial_policy: initialPolicy ?? null,
        model: initialModel ?? null,
        reasoning_effort: initialReasoningEffort ?? null,
        task_id: parallelTask.taskId,
        goal: parallelTask.goal,
      });
      return;
    }
    await call("engine.new_session", {
      provider,
      cwd,
      use_worktree: worktreeBase !== null,
      worktree_base: worktreeBase,
      worktree_base_sha: worktreeBaseSha ?? null,
      request_id: requestId,
      initial_policy: initialPolicy ?? null,
      model: initialModel ?? null,
      transient,
      reasoning_effort: initialReasoningEffort ?? null,
    });
  }
}

/** Stop and forget one app-lifetime side-chat session. Durable sessions are rejected. */
export async function closeTransientSession(session: string): Promise<boolean> {
  return coreAvailable
    ? await call<boolean>("engine.close_transient_session", { session })
    : true;
}

/** Resolve selectable baselines from local refs only. This command never fetches. */
export async function listWorktreeBaselines(
  cwd: string
): Promise<WorktreeBaselineOption[]> {
  return inDesktop
    ? await call<WorktreeBaselineOption[]>("worktrees.baselines", { cwd })
    : [];
}

/** Remove a session's isolated checkout and its codetwo/ branch. Repeating is a no-op success. */
export async function discardSessionWorktree(
  session: string
): Promise<DiscardedWorktree> {
  return inDesktop
    ? await call<DiscardedWorktree>("worktrees.discard_session", { session })
    : { removed_checkout: false };
}

/** Every checkout under a project's worktree container — session-claimed, orphan, or stale. */
export async function listProjectWorktrees(
  projectPath: string
): Promise<WorktreeStatusEntry[]> {
  return inDesktop
    ? await call<WorktreeStatusEntry[]>("worktrees.list", {
        project_path: projectPath,
      })
    : [];
}

const browserWorktreeSettings: WorktreeSettings = {
  fetch_upstream: false,
  auto_delete: false,
  auto_delete_limit: 15,
};

export async function getWorktreeSettings(): Promise<WorktreeSettings> {
  return inDesktop
    ? await call<WorktreeSettings>("worktrees.settings", {})
    : { ...browserWorktreeSettings };
}

export async function updateWorktreeSettings(
  settings: WorktreeSettings
): Promise<WorktreeSettings> {
  return inDesktop
    ? await call<WorktreeSettings>("worktrees.set_settings", { settings })
    : { ...settings };
}

/** Remove an unclaimed checkout by path. The core rejects paths a session still claims. */
export async function discardOrphanWorktree(
  projectPath: string,
  worktreePath: string
): Promise<DiscardedWorktree> {
  return inDesktop
    ? await call<DiscardedWorktree>("worktrees.discard_orphan", {
        project_path: projectPath,
        worktree_path: worktreePath,
      })
    : { removed_checkout: false };
}

export async function submitPrompt(
  session: string,
  doc: DocBlock[],
  requestId: string
): Promise<void> {
  if (coreAvailable)
    await call("engine.prompt", { session, doc, request_id: requestId });
}

/** Connect a durable session early so provider-native modes are available before its next turn. */
export async function prepareSession(session: string): Promise<void> {
  if (coreAvailable) await call("engine.prepare_session", { session });
}

export async function queuePrompt(
  session: string,
  doc: DocBlock[],
  requestId: string
): Promise<{ position: number }> {
  return coreAvailable
    ? await call<{ position: number }>("engine.queue", {
        session,
        doc,
        request_id: requestId,
      })
    : { position: 0 };
}

export async function steerPrompt(
  session: string,
  doc: DocBlock[],
  requestId: string
): Promise<{ outcome: "injected" | "startedNewTurn" }> {
  return await call("engine.steer", { session, doc, request_id: requestId });
}

export async function listAutomations(): Promise<Automation[]> {
  return inDesktop ? await call<Automation[]>("automation.list") : [];
}

export async function createAutomation(
  input: AutomationInput
): Promise<Automation> {
  return await call<Automation>("automation.create", { input });
}

export async function updateAutomation(
  id: string,
  input: AutomationInput
): Promise<Automation> {
  return await call<Automation>("automation.update", { id, input });
}

export async function setAutomationEnabled(
  id: string,
  enabled: boolean
): Promise<Automation> {
  return await call<Automation>("automation.set_enabled", { id, enabled });
}

export async function deleteAutomation(id: string): Promise<boolean> {
  return await call<boolean>("automation.delete", { id });
}

export async function listAutomationRuns(
  automationId?: string | null,
  limit = 50
): Promise<AutomationRun[]> {
  return inDesktop
    ? await call<AutomationRun[]>("automation.runs", {
        automation_id: automationId ?? null,
        limit,
      })
    : [];
}

export async function runAutomationNow(id: string): Promise<AutomationRun> {
  return await call<AutomationRun>("automation.run_now", { id });
}

export async function rerunAutomation(runId: string): Promise<AutomationRun> {
  return await call<AutomationRun>("automation.rerun", { run_id: runId });
}

export async function onAutomationAlert(
  cb: (alert: AutomationAlert) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return listenDesktop<AutomationAlert>("automation-alert", cb);
}

export async function onAutomationChanged(
  cb: (automationId: string) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return listenDesktop<string>("automation-changed", cb);
}

export async function onDeviceSyncChanged(
  cb: (imported: NonNullable<DeviceSyncStatus["imported"]>) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return listenDesktop<NonNullable<DeviceSyncStatus["imported"]>>(
    "device-sync-changed",
    cb
  );
}

export async function answerPermission(
  session: string,
  requestId: string,
  optionId: string | null
): Promise<boolean> {
  if (coreAvailable) {
    return await call<boolean>("engine.answer_permission", {
      session,
      request_id: requestId,
      option_id: optionId,
    });
  }
  return false;
}

export async function answerElicitation(
  session: string,
  requestId: string,
  answer: ElicitationAnswer
): Promise<boolean> {
  if (coreAvailable) {
    return await call<boolean>("engine.answer_elicitation", {
      session,
      request_id: requestId,
      answer,
    });
  }
  return false;
}

export async function setPermissionMode(
  session: string,
  mode: string
): Promise<void> {
  if (coreAvailable)
    await call("engine.set_permission_mode", { session, mode });
}

export async function setExecutionPolicy(
  session: string,
  mode: PermissionMode,
  sandbox: Sandbox,
  requestId: string
): Promise<void> {
  if (coreAvailable) {
    await call("engine.set_execution_policy", {
      session,
      mode,
      sandbox,
      request_id: requestId,
    });
  }
}

/// One entry in a directory listing, for the file tree in the side dock.
export interface DirEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

/** One directory level. The tree expands lazily, so nothing is capped or silently truncated. */
export async function listDir(cwd: string, path: string): Promise<DirEntry[]> {
  return inDesktop
    ? await call<DirEntry[]>("workspace.list_dir", { cwd, path })
    : [];
}

/** Create an empty file. Rejects paths that already exist rather than overwriting. */
export async function createFile(cwd: string, path: string): Promise<void> {
  if (inDesktop) await call("workspace.create_file", { cwd, path });
}

export async function createDir(cwd: string, path: string): Promise<void> {
  if (inDesktop) await call("workspace.create_dir", { cwd, path });
}

/** Read a file for the viewer. Rejects binaries and oversized files rather than showing mojibake. */
export async function readText(cwd: string, path: string): Promise<string> {
  return inDesktop
    ? await call<string>("workspace.read_text", { cwd, path })
    : "";
}

/**
 * Raw bytes for the image preview. JSON host results arrive as a number array; the ArrayBuffer
 * branch keeps the wrapper compatible with direct binary transports.
 */
export async function readBinary(
  cwd: string,
  path: string
): Promise<Uint8Array> {
  if (!inDesktop) return new Uint8Array();
  const res = await call<ArrayBuffer | number[]>("workspace.read_binary", {
    cwd,
    path,
  });
  return res instanceof ArrayBuffer ? new Uint8Array(res) : new Uint8Array(res);
}

export async function getArtifact(id: string): Promise<Uint8Array> {
  if (!inDesktop) return new Uint8Array();
  const res = await call<ArrayBuffer | number[]>("artifacts.get", { id });
  return res instanceof ArrayBuffer ? new Uint8Array(res) : new Uint8Array(res);
}

export async function saveArtifactAs(
  id: string,
  displayName: string
): Promise<boolean> {
  if (!inDesktop) return false;
  const destination = await desktopSaveDialog({ defaultPath: displayName });
  if (destination == null || destination === "") return false;
  await call("artifacts.save_as", { id, destination });
  return true;
}

export async function revealArtifact(id: string): Promise<void> {
  if (inDesktop) await call("artifacts.reveal", { id });
}

/** The artifacts a session produced, newest first. */
export async function listArtifacts(session: string): Promise<ArtifactRef[]> {
  if (!inDesktop) return [];
  return await call<ArtifactRef[]>("artifacts.list", { session });
}

/** A bounded UTF-8 view of a text artifact for preview; rejects binary and oversized bodies. */
export async function readArtifactText(
  id: string,
  maxBytes?: number
): Promise<string> {
  if (!inDesktop) return "";
  return await call<string>("artifacts.read_text", {
    id,
    max_bytes: maxBytes ?? null,
  });
}

/** Load a bounded local visualize fragment; the host validates its canonical root and file type. */
export async function readVisualization(path: string): Promise<string> {
  if (
    !inDesktop &&
    import.meta.env.DEV &&
    path === "/__codetwo__/rich-transcript-preview.html"
  ) {
    return String.raw`<section class="card" aria-labelledby="release-confidence">
  <div class="viz-row" style="justify-content:space-between">
    <div><h2 id="release-confidence">Release confidence</h2><p class="text-small text-muted">Latest verification run</p></div>
    <span class="viz-badge">Healthy</span>
  </div>
  <div class="viz-grid" style="margin-top:12px">
    <div><p class="text-small text-muted">Checks passed</p><p class="viz-stat-value">511</p></div>
    <div><p class="text-small text-muted">New design violations</p><p class="viz-stat-value">0</p></div>
    <div><p class="text-small text-muted">Renderer build</p><p class="viz-stat-value">Passed</p></div>
  </div>
  <div class="viz-row" style="margin-top:14px">
    <button class="btn btn-primary" onclick="window.openai.sendFollowUpMessage({prompt:'Show the failed checks only',title:'Filter verification results'})">Filter results</button>
    <span class="text-small text-muted">Updated just now</span>
  </div>
</section>`;
  }
  return await call<string>("artifacts.read_visualization", { path });
}

export async function writeText(
  cwd: string,
  path: string,
  content: string
): Promise<void> {
  if (inDesktop) await call("workspace.write_text", { cwd, path, content });
}

function splitNativePath(path: string): { cwd: string; name: string } {
  const separator = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (separator < 0) return { cwd: ".", name: path };
  const windowsRoot = separator === 2 && path[1] === ":";
  const cwd = windowsRoot
    ? path.slice(0, separator + 1)
    : path.slice(0, separator) || path.slice(0, 1);
  return { cwd, name: path.slice(separator + 1) };
}

/** Native theme picker. `undefined` means the web build should use its hidden file input. */
export async function pickAppearanceThemeDocument(): Promise<
  string | null | undefined
> {
  if (!inDesktop) return undefined;
  const [selected] = await desktopOpenDialog({
    multiple: false,
    directory: false,
    title: "Import C2 theme",
    filters: [{ name: "C2 theme", extensions: ["json"] }],
  });
  if (!selected) return null;
  const { cwd, name } = splitNativePath(selected);
  return await call<string>("workspace.read_text", { cwd, path: name });
}

export type AppearanceThemeSaveResult = "saved" | "cancelled" | "unsupported";

/** Saves through the OS dialog on desktop; the web build falls back to a normal JSON download. */
export async function saveAppearanceThemeDocument(
  suggestedName: string,
  content: string
): Promise<AppearanceThemeSaveResult> {
  if (!inDesktop) return "unsupported";
  const selected = await desktopSaveDialog({
    title: "Export C2 theme",
    defaultPath: suggestedName,
    filters: [{ name: "C2 theme", extensions: ["json"] }],
  });
  if (typeof selected !== "string") return "cancelled";
  const { cwd, name } = splitNativePath(selected);
  try {
    await call("workspace.create_file", { cwd, path: name });
  } catch {
    // The save panel explicitly confirms replacement. An existing file is therefore expected.
  }
  await call("workspace.write_text", { cwd, path: name, content });
  return "saved";
}

export type DiagnosticsExportResult = "saved" | "cancelled" | "unsupported";

/** Save the Core's default-redacted support snapshot through an explicit native save panel. */
export async function exportRedactedDiagnostics(): Promise<DiagnosticsExportResult> {
  if (!inDesktop) return "unsupported";
  const date = new Date().toISOString().slice(0, 10);
  const selected = await desktopSaveDialog({
    title: "Export C2 diagnostics",
    defaultPath: `c2-diagnostics-${date}.json`,
    filters: [{ name: "C2 diagnostics", extensions: ["json"] }],
  });
  if (typeof selected !== "string") return "cancelled";
  const report = await call("diagnostics.redacted_snapshot");
  const { cwd, name } = splitNativePath(selected);
  try {
    await call("workspace.create_file", { cwd, path: name });
  } catch {
    // The native save panel already asked the user to confirm replacement.
  }
  await call("workspace.write_text", {
    cwd,
    path: name,
    content: `${JSON.stringify(report, null, 2)}\n`,
  });
  return "saved";
}

/** Rename *and* move — a `to` with a different parent moves it. */
export async function renamePath(
  cwd: string,
  from: string,
  to: string
): Promise<void> {
  if (inDesktop) await call("workspace.rename", { cwd, from, to });
}

export async function copyPath(
  cwd: string,
  from: string,
  to: string
): Promise<void> {
  if (inDesktop) await call("workspace.copy", { cwd, from, to });
}

export async function deletePath(cwd: string, path: string): Promise<void> {
  if (inDesktop) await call("workspace.delete", { cwd, path });
}

/** Open the webview inspector on the app's own UI. */
export async function openDevtools(): Promise<void> {
  if (!inDesktop)
    throw new Error("WebView DevTools require the C2 desktop app.");
  await desktopOpenDevtools();
}

// ---- built-in browser --------------------------------------------------------------------------
// Each browser tab is a native child webview of the main window, not an iframe: the sites people
// actually open (github.com, google.com, anything with `X-Frame-Options: DENY`) refuse to be framed
// and render blank. The panel measures a placeholder in the DOM and keeps the native view pinned
// over it — see `browser/Browser.tsx` and `browser/electrobun.ts`.

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Create the tab's webview if needed, place it, and load `url`. Safe to call repeatedly. */
export async function browserOpen(
  label: string,
  url: string,
  _r: Rect
): Promise<void> {
  if (inDesktop) browserOpenLocal(label, url);
}

export async function browserBounds(label: string, _r: Rect): Promise<void> {
  if (inDesktop) browserBoundsLocal(label);
}

export async function browserNavigate(
  label: string,
  url: string
): Promise<void> {
  if (inDesktop) browserNavigateLocal(label, url);
}

/** −1 is back, 1 is forward — the page's own history, not one we keep for it. */
export async function browserHistory(
  label: string,
  delta: number
): Promise<void> {
  if (inDesktop) browserHistoryLocal(label, delta);
}

export async function browserReload(label: string): Promise<void> {
  if (inDesktop) browserReloadLocal(label);
}

/** Hide, not close: the page keeps its state, which is the point of tabs. */
export async function browserVisible(
  label: string,
  visible: boolean
): Promise<void> {
  if (inDesktop) browserVisibleLocal(label, visible);
}

export async function browserZoom(
  label: string,
  factor: number
): Promise<void> {
  if (inDesktop) browserZoomLocal(label, factor);
}

export async function browserDevtools(label: string): Promise<void> {
  if (inDesktop) browserDevtoolsLocal(label);
}

export async function browserClose(label: string): Promise<void> {
  if (inDesktop) browserCloseLocal(label);
}

export async function browserCloseAll(): Promise<void> {
  if (inDesktop) browserCloseAllLocal();
}

export interface BrowserNav {
  label: string;
  url: string;
}

export type BrowserTab = EmbeddedBrowserTab;

export async function browserRegistrySnapshot(): Promise<BrowserTab[]> {
  return inDesktop
    ? browserRegistrySnapshotLocal()
    : [
        {
          id: "browser-1",
          url: "about:blank",
          title: "",
          active: true,
          agent_active: false,
        },
      ];
}

export async function browserRegistryCreate(url: string): Promise<BrowserTab> {
  return inDesktop
    ? browserRegistryCreateLocal(url)
    : {
        id: `browser-${Date.now()}`,
        url,
        title: "",
        active: true,
        agent_active: false,
      };
}

export async function browserTakeControl(label: string): Promise<void> {
  if (inDesktop) browserTakeControlLocal(label);
}

export async function browserPermissions(): Promise<string[]> {
  return [];
}

export async function browserRevokePermission(origin: string): Promise<void> {
  void origin;
}

export async function onBrowserRegistry(
  cb: (tabs: BrowserTab[]) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return browserSubscribe("browser-registry", cb);
}

export async function onBrowserAgentActivity(
  cb: (payload: { tabId: string }) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return browserSubscribe("browser-agent-activity", cb);
}

export async function onBrowserDownloadBlocked(
  cb: (payload: { label: string }) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return browserSubscribe("browser-download-blocked", cb);
}

// ---- in-page annotator -------------------------------------------------------------------------
// Element picking, the note card and the live style edits all live inside the page (`annotate.js`),
// because the page is a native webview and nothing in our DOM can be drawn on top of it. The app
// arms it and pulls the results back out; the page can never call in.

/** Arm or disarm element picking on the active page. */
export async function browserAnnotate(
  label: string,
  on: boolean
): Promise<void> {
  if (inDesktop) browserAnnotateLocal(label, on);
}

export async function browserAnnotations(
  label: string,
  url: string
): Promise<Annotation[]> {
  return inDesktop ? await browserAnnotationsLocal(label, url) : [];
}

export async function browserAnnotationCount(label: string): Promise<number> {
  return inDesktop ? await browserAnnotationCountLocal(label) : 0;
}

/** Drop the notes and undo the live style edits they described. */
export async function browserAnnotationsClear(label: string): Promise<void> {
  if (inDesktop) browserAnnotationsClearLocal(label);
}

/** A document finished loading — the point at which a fresh annotator exists to re-arm. */
export async function onBrowserLoad(
  cb: (p: BrowserNav) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return browserSubscribe("browser-load", cb);
}

/** The page navigated itself — a link, a redirect, a form post. The address bar follows this. */
export async function onBrowserNav(
  cb: (p: BrowserNav) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return browserSubscribe("browser-nav", cb);
}

export async function onBrowserTitle(
  cb: (p: { label: string; title: string }) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return browserSubscribe("browser-title", cb);
}

/** `target="_blank"` / `window.open`, denied natively and reopened here as a tab. */
export async function onBrowserPopup(
  cb: (p: BrowserNav) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return browserSubscribe("browser-popup", cb);
}

/**
 * Native yes/no dialog. `window.confirm` is a silent always-true stub in wry's WKWebView (no
 * JS-confirm delegate), which turns every "are you sure?" into "yes" — so anything destructive
 * must come through here instead.
 */
export async function confirmNative(
  message: string,
  title?: string
): Promise<boolean> {
  if (!inDesktop) return window.confirm(message);
  try {
    return await desktopConfirm(message, title);
  } catch (error) {
    // A missing capability must fail closed: "no" loses nothing, "yes" can destroy work.
    console.error("confirmNative:", error);
    return false;
  }
}

/** Hand a URL to the system's default browser. Outside desktop, open a plain new tab. */
export async function openExternal(url: string): Promise<void> {
  if (inDesktop) {
    await desktopOpenExternal(url);
  } else {
    window.open(url, "_blank", "noopener");
  }
}

/** Open a local path with the operating system (a directory opens in Finder on macOS). */
export async function openNativePath(path: string): Promise<boolean> {
  if (!inDesktop) return false;
  return await desktopOpenPath(path);
}

/** Reveal a local path in the operating system's file manager. */
export async function revealNativePath(path: string): Promise<boolean> {
  if (!inDesktop) return false;
  return await desktopShowItemInFolder(path);
}

/** Open a workspace in one of the desktop destinations offered by the session header. */
export async function openWorkspace(
  path: string,
  target: WorkspaceOpenTarget
): Promise<boolean> {
  if (!inDesktop) return false;
  return await desktopOpenWorkspace(path, target);
}

// ---- LSP bridge --------------------------------------------------------------------------------
// The Rust side spawns real language servers (rust-analyzer, pyright, gopls, …) as children and
// frames their stdio JSON-RPC; the frontend LSP client in src/lsp speaks the protocol. One server
// per (binary, project) pair — the key names that pair.

/** Spawn (or reuse) a language server for `lang` rooted at `cwd`. Null when none is installed. */
export async function lspStart(
  cwd: string,
  lang: string
): Promise<string | null> {
  return inDesktop
    ? await call<string | null>("lsp.start", { cwd, lang })
    : null;
}

/** Send one raw JSON-RPC message (already serialized) to the server behind `key`. */
export async function lspSend(key: string, payload: string): Promise<void> {
  if (inDesktop) await call("lsp.send", { key, payload });
}

/** Suspend or resume language servers in one explicit project realm without unloading the plugin. */
export async function lspSetRuntimeEnabled(
  enabled: boolean,
  projectPath: string | null
): Promise<void> {
  if (inDesktop)
    await call("lsp.set_runtime_enabled", { enabled }, projectPath);
}

export interface LspMessage {
  key: string;
  payload: string;
}

export async function onLspMessage(
  cb: (p: LspMessage) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return listenDesktop<LspMessage>("lsp-message", cb);
}

/** The server process died or closed its pipe — the client for `key` is gone. */
export async function onLspExit(
  cb: (key: string) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return listenDesktop<string>("lsp-exit", cb);
}

/** Newest text per session id, for the rail's preview line. */
export async function sessionPreviews(): Promise<Record<string, string>> {
  if (!coreAvailable) return {};
  const rows = await call<[string, string][]>("sessions.previews");
  return Object.fromEntries(rows);
}

export interface SessionSearchHit {
  session_id: string;
  title: string;
  cwd: string;
  archived: boolean;
  role: "user" | "agent";
  snippet: string;
  seq: number;
}

/** Bounded full-text search over canonical user prompts and agent output. */
export async function searchSessions(
  query: string,
  limit = 12
): Promise<SessionSearchHit[]> {
  return inDesktop
    ? await call<SessionSearchHit[]>("sessions.search", { query, limit })
    : [];
}

// ---- projects ----------------------------------------------------------------------------------

export async function listProjects(): Promise<Project[]> {
  return coreAvailable ? await call<Project[]>("projects.list") : [];
}

/**
 * Native folder chooser. Returns null when the user cancels — which is a normal outcome, not an
 * error. Outside desktop there's no picker, so it resolves to null and the caller does nothing.
 */
export async function pickDirectory(): Promise<string | null> {
  if (!inDesktop) return null;
  const [picked] = await desktopOpenDialog({
    directory: true,
    multiple: false,
    title: "Choose a project folder",
  });
  return picked ?? null;
}

/** Returns the resolved absolute path, which is the project's identity. */
export async function addProject(path: string, name?: string): Promise<string> {
  return inDesktop
    ? await call<string>("projects.add", { path, name: name ?? null })
    : path;
}

export async function openProject(path: string): Promise<void> {
  if (inDesktop) await call("projects.open", { path });
}

export async function renameProject(path: string, name: string): Promise<void> {
  if (inDesktop) await call("projects.rename", { path, name });
}

export async function setProjectAgentDefaults(
  path: string,
  provider: string | null,
  model: string | null,
  reasoningEffort: string | null
): Promise<void> {
  if (inDesktop) {
    await call("projects.set_agent_defaults", {
      path,
      provider,
      model,
      reasoning_effort: reasoningEffort,
    });
  }
}

export async function pickProjectIcon(): Promise<string | null> {
  if (!inDesktop) return null;
  const [picked] = await desktopOpenDialog({
    directory: false,
    multiple: false,
    title: "Choose a project icon",
    filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
  });
  return picked ?? null;
}

export async function setProjectIcon(
  path: string,
  source: string | null
): Promise<number> {
  return inDesktop
    ? await call<number>("projects.set_icon", { path, source })
    : Date.now();
}

export async function getProjectIcon(
  path: string
): Promise<ProjectIconData | null> {
  if (!inDesktop) return null;
  const icon = await call<{
    mime_type: ProjectIconData["mime_type"];
    bytes: ArrayBuffer | number[];
  } | null>("projects.icon", { path });
  if (!icon) return null;
  return {
    mime_type: icon.mime_type,
    bytes:
      icon.bytes instanceof ArrayBuffer
        ? new Uint8Array(icon.bytes)
        : new Uint8Array(icon.bytes),
  };
}

export async function setProjectWorktreeMode(
  path: string,
  mode: ProjectWorktreeMode | null
): Promise<void> {
  if (inDesktop) await call("projects.set_worktree_mode", { path, mode });
}

export async function removeProject(path: string): Promise<void> {
  if (inDesktop) await call("projects.remove", { path });
}

/** Where a new session should start. Resolved by the core, never `"."` — see `default_cwd`. */
export async function defaultCwd(): Promise<string> {
  return coreAvailable ? await call<string>("workspace.default_cwd") : ".";
}

export async function setModel(session: string, model: string): Promise<void> {
  if (coreAvailable) await call("engine.set_model", { session, model });
}

/** Atomically replace an idle Session's provider while retaining its transcript and workspace. */
export async function switchProvider(
  session: string,
  provider: string,
  model: string | null = null
): Promise<SessionInfo> {
  if (inDesktop) {
    return await call<SessionInfo>("engine.switch_provider", {
      session,
      provider,
      model,
    });
  }
  throw new Error("Provider switching requires the desktop app.");
}

/** Set an agent-reported config option (model, reasoning effort, …) by its id. */
export async function setConfigOption(
  session: string,
  configId: string,
  value: string
): Promise<void> {
  if (coreAvailable)
    await call("engine.set_config_option", {
      session,
      config_id: configId,
      value,
    });
}

export async function cancelTurn(session: string): Promise<void> {
  if (coreAvailable) await call("engine.cancel", { session });
}

/**
 * Attach to the terminal `id`, creating it if it doesn't exist. Terminals live in the core, so a
 * re-attach returns `restore`: a VT dump to replay into a fresh renderer so the screen and
 * scrollback come back exactly as they were.
 */
export async function ptySpawn(
  id: string,
  cwd: string | null,
  rows: number,
  cols: number,
  opts?: { tmuxSession?: string | null; scrollback?: number }
): Promise<PtyAttach> {
  if (!inDesktop) return { created: true, restore: "" };
  return await call<PtyAttach>("terminal.spawn", {
    id,
    cwd,
    rows,
    cols,
    scrollback: opts?.scrollback ?? null,
    tmux_session: opts?.tmuxSession ?? null,
  });
}

export async function tmuxAvailable(): Promise<boolean> {
  return inDesktop ? await call<boolean>("terminal.tmux_available") : false;
}

export async function ptyWrite(id: string, data: string): Promise<void> {
  if (inDesktop) await call("terminal.write", { id, data });
}

export async function ptyResize(
  id: string,
  rows: number,
  cols: number
): Promise<void> {
  if (inDesktop) await call("terminal.resize", { id, rows, cols });
}

/** Terminal contents as plain text — `all` includes scrollback, otherwise just the visible screen. */
export async function ptyDump(id: string, all = true): Promise<string> {
  return inDesktop ? await call<string>("terminal.dump", { id, all }) : "";
}

/** Close a terminal for good, killing its child process. Detaching a renderer does not do this. */
export async function ptyKill(id: string): Promise<void> {
  if (inDesktop) await call("terminal.kill", { id });
}

export async function getTranscriptPage(
  session: string,
  before: number | null = null,
  limit = 20
): Promise<TranscriptPage> {
  return coreAvailable
    ? await call<TranscriptPage>("sessions.transcript", {
        session,
        before,
        limit,
      })
    : { entries: [], next_before: null, snapshot_through: null };
}

// ---- git (F1) --------------------------------------------------------------------------------

export interface GitFile {
  path: string;
  original_path: string | null;
  staged: boolean;
  unstaged: boolean;
  state: string;
  staged_state: string | null;
  unstaged_state: string | null;
}
export interface GitStatus {
  is_repo: boolean;
  branch: string;
  ahead: number;
  behind: number;
  files: GitFile[];
}

export type SourceControlProviderKind =
  | "github"
  | "gitlab"
  | "azure-devops"
  | "bitbucket"
  | "unknown";

export interface SourceControlInfo {
  remote_name: string;
  provider: SourceControlProviderKind;
  provider_name: string;
  host: string;
  web_url: string | null;
  change_request_label: "PR" | "MR" | "change request";
  create_change_request_supported: boolean;
  required_cli: string | null;
  required_cli_available: boolean;
}

export async function gitSourceControlInfo(
  cwd: string
): Promise<SourceControlInfo | null> {
  return inDesktop
    ? await call<SourceControlInfo | null>("workspace.source_control", { cwd })
    : null;
}

export interface GitHubPullRequestCheck {
  name: string;
  status: string | null;
  conclusion: string | null;
  details_url: string | null;
  workflow_name: string | null;
}

export interface GitHubPullRequest {
  number: number;
  title: string;
  url: string;
  state: string;
  is_draft: boolean;
  head_ref: string;
  base_ref: string;
  additions: number;
  deletions: number;
  changed_files: number;
  body: string;
  review_decision: string | null;
  mergeable: string;
  merge_state_status: string;
  author: string;
  comments_count: number;
  reviews_count: number;
  checks: GitHubPullRequestCheck[];
  created_at: string;
  updated_at: string;
}

export interface GitHubPullRequestDiff {
  text: string;
  truncated: boolean;
}

export type GitHubReviewAction = "approve" | "comment" | "request_changes";
export type GitHubMergeStrategy = "merge" | "squash" | "rebase";

export async function githubCurrentPullRequest(
  cwd: string
): Promise<GitHubPullRequest | null> {
  return inDesktop
    ? await call<GitHubPullRequest | null>("github.current_pr", { cwd })
    : null;
}

export async function githubPullRequestDiff(
  cwd: string,
  number: number
): Promise<GitHubPullRequestDiff> {
  return inDesktop
    ? await call<GitHubPullRequestDiff>("github.pr_diff", { cwd, number })
    : { text: "", truncated: false };
}

export async function githubReviewPullRequest(
  cwd: string,
  number: number,
  action: GitHubReviewAction,
  body: string
): Promise<void> {
  if (inDesktop) await call("github.review_pr", { cwd, number, action, body });
}

export async function githubMergePullRequest(
  cwd: string,
  number: number,
  strategy: GitHubMergeStrategy
): Promise<void> {
  if (inDesktop) await call("github.merge_pr", { cwd, number, strategy });
}

export async function gitStatus(cwd: string): Promise<GitStatus> {
  return inDesktop
    ? await call<GitStatus>("git.status", { cwd })
    : { is_repo: false, branch: "", ahead: 0, behind: 0, files: [] };
}

export interface Checkpoint {
  id: string;
  refname: string;
  commit: string;
  message: string;
}

export async function gitCheckpoint(
  cwd: string,
  message: string
): Promise<Checkpoint | null> {
  return inDesktop
    ? await call<Checkpoint>("git.checkpoint", { cwd, message })
    : null;
}
export async function gitCheckpoints(cwd: string): Promise<Checkpoint[]> {
  return inDesktop ? await call<Checkpoint[]>("git.checkpoints", { cwd }) : [];
}

export type GitDiffScope = "all" | "staged" | "unstaged";

export type GitDiffLineKind = "context" | "added" | "removed";

export interface GitDiffLine {
  kind: GitDiffLineKind;
  old_line: number | null;
  new_line: number | null;
  text: string;
}

export interface GitDiffHunk {
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  lines: GitDiffLine[];
}

export interface GitFileDiff {
  path: string;
  old_path: string | null;
  additions: number;
  deletions: number;
  hunks: GitDiffHunk[];
}

export interface GitDiffResult {
  text: string;
  truncated: boolean;
  truncation_reason: string | null;
  returned_bytes: number;
  files: number;
  file_diffs: GitFileDiff[];
}

export interface GitDiffStat {
  added: number;
  deleted: number;
  files: number;
  truncated: boolean;
  truncation_reason: string | null;
}

const EMPTY_DIFF: GitDiffResult = {
  text: "",
  truncated: false,
  truncation_reason: null,
  returned_bytes: 0,
  files: 0,
  file_diffs: [],
};

export async function gitDiff(
  cwd: string,
  path: string | null,
  scope: GitDiffScope = "all"
): Promise<GitDiffResult> {
  return inDesktop
    ? await call<GitDiffResult>("git.diff", { cwd, path, scope })
    : EMPTY_DIFF;
}
export async function gitDiffSince(
  cwd: string,
  commit: string
): Promise<GitDiffResult> {
  return inDesktop
    ? await call<GitDiffResult>("git.diff_since", { cwd, commit })
    : EMPTY_DIFF;
}
export async function gitDiffStat(cwd: string): Promise<GitDiffStat> {
  return inDesktop
    ? await call<GitDiffStat>("git.diff_stat", { cwd })
    : {
        added: 0,
        deleted: 0,
        files: 0,
        truncated: false,
        truncation_reason: null,
      };
}
export async function gitStagePaths(
  cwd: string,
  paths: string[]
): Promise<void> {
  if (inDesktop) await call("git.stage", { cwd, paths });
}
export async function gitUnstagePaths(
  cwd: string,
  paths: string[]
): Promise<void> {
  if (inDesktop) await call("git.unstage", { cwd, paths });
}
export async function gitRevert(cwd: string, commit: string): Promise<void> {
  if (inDesktop) await call("git.revert", { cwd, commit });
}
export async function gitCommit(cwd: string, message: string): Promise<string> {
  return inDesktop ? await call<string>("git.commit", { cwd, message }) : "";
}
export async function gitPush(cwd: string): Promise<string> {
  return inDesktop ? await call<string>("git.push", { cwd }) : "";
}

// ---- keybindings (F2) ------------------------------------------------------------------------

export type KeymapEntry = [action: string, key: string, label: string];

// Mirrors `Action::default_key()` in crates/core/src/keymap.rs — keep the two in step. This is only
// the browser-preview fallback; inside desktop the real keymap (with user overrides) comes from core.
export const DEFAULT_KEYMAP: KeymapEntry[] = [
  ["run", "Mod+Enter", "Run prompt"],
  ["new_session", "Mod+N", "New task"],
  ["cancel", "Mod+.", "Cancel turn"],
  ["toggle_terminal", "Mod+J", "Toggle terminal"],
  ["toggle_browser", "Mod+B", "Toggle browser"],
  ["toggle_git", "Mod+Shift+B", "Toggle git panel"],
  ["close_panel", "Escape", "Close side panel"],
  ["split_pane_right", "Mod+Alt+R", "Split pane right"],
  ["split_pane_down", "Mod+Alt+D", "Split pane down"],
  ["toggle_side_panel", "Mod+Alt+P", "Toggle side panel"],
  ["open_skill_picker", "Mod+/", "Open skill picker"],
  ["focus_editor", "Mod+E", "Focus editor"],
  ["toggle_doc_mode", "Mod+Shift+E", "Expand document to full height"],
  ["open_command_palette", "Mod+K", "Command palette"],
  ["open_source_control", "Mod+Shift+G", "Source control"],
  ["open_market", "Mod+Shift+M", "Open Plugin Hub"],
  ["open_files", "Mod+P", "Browse workspace files"],
  ["open_finder", "Mod+O", "Open in file manager"],
  ["search_workspace", "Mod+Shift+F", "Search workspace contents"],
  ["open_issues", "Mod+Shift+I", "Open issues"],
  ["open_usage", "Mod+Shift+U", "Open usage"],
  ["open_settings", "Mod+,", "Open settings"],
  ["cycle_permission_mode", "Mod+Shift+P", "Cycle permission mode"],
  ["refresh_git", "Mod+G", "Refresh git status"],
  ["prev_session", "Mod+Alt+ArrowUp", "Previous session"],
  ["next_session", "Mod+Alt+ArrowDown", "Next session"],
  ["open_mission_control", "Mod+Shift+O", "Open mission control"],
];

export async function getKeymap(): Promise<KeymapEntry[]> {
  return inDesktop ? await call<KeymapEntry[]>("keymap.get") : DEFAULT_KEYMAP;
}

export async function setKeymap(action: string, key: string): Promise<void> {
  if (inDesktop) await call("keymap.set", { action, key });
}

// ---- browser annotate (F3/F4) ----------------------------------------------------------------

/** One property the in-page annotator adjusted, and what it went from and to. */
export interface StyleChange {
  property: string;
  from: string;
  to: string;
}

export interface Annotation {
  url: string;
  note: string;
  selector: string | null;
  selected_text: string | null;
  styles: StyleChange[];
}

// ---- Plugin Hub + component market (F5) -------------------------------------------------------

export interface MarketItem {
  id: string;
  name: string;
  description: string;
  author: string;
  tags: string[];
  icon: string | null;
  kind: string;
  installed: boolean;
}

const FALLBACK_MARKET: MarketItem[] = [
  {
    id: "architect",
    name: "System Architect",
    description: "Design before coding.",
    author: "codetwo",
    tags: ["design"],
    icon: null,
    kind: "fragment",
    installed: false,
  },
  {
    id: "test-suite",
    name: "Test Suite Author",
    description: "Thorough deterministic tests.",
    author: "codetwo",
    tags: ["testing"],
    icon: null,
    kind: "fragment",
    installed: false,
  },
  {
    id: "browser-tool",
    name: "Browser Tool (MCP)",
    description: "Give the agent a browser.",
    author: "codetwo",
    tags: ["mcp", "browser"],
    icon: null,
    kind: "mcp",
    installed: false,
  },
];

export async function marketCatalog(): Promise<MarketItem[]> {
  return inDesktop
    ? await call<MarketItem[]>("market.catalog")
    : FALLBACK_MARKET;
}

export async function marketInstall(id: string): Promise<void> {
  if (inDesktop) await call("market.install", { id });
}

export interface PluginCounts {
  skills: number;
  subagents: number;
  mcp_servers: number;
  scaffolds: number;
  commands: number;
  hooks: number;
  lsp_servers: number;
  monitors: number;
  apps: number;
  /** Safe declarative actions rendered into C2-owned UI slots. */
  ui: number;
  /** Host-rendered external-system connectors backed by an owned runtime command. */
  connectors: number;
  /** Agent Scenes components (R14); serde-defaulted server-side, so always present here. */
  scenes: number;
  pipelines: number;
  /** Present for hosts that support a C2 JSON-RPC process runtime contribution. */
  runtime: number;
  /** Statically declared commands implemented by the process runtime. */
  runtime_commands: number;
}

export type PluginInstallScope = "user" | "project" | "local" | "managed";

export interface PluginDiagnostic {
  level: "warning" | "error";
  code: string;
  message: string;
  component?: string;
}

export interface PluginExtensionComponent {
  kind: string;
  name: string;
  path: string;
  status: "ready" | "requires_trust" | "requires_auth" | "unsupported";
}

export interface PluginScaffoldInfo {
  id: string;
  name: string;
  description: string;
  files: number;
}

export { PLUGIN_UI_SLOT_IDS, pluginUiComponentId } from "./pluginModel";
export type {
  PluginRuntimeCommandContribution,
  PluginConnectorCapability,
  PluginConnectorContribution,
  PluginUiContribution,
  PluginUiSlotId,
} from "./pluginModel";

export interface PluginLanguageServer {
  id: string;
  languages: string[];
  command: string;
  args: string[];
  env: Record<string, string>;
}

export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  source: string;
  repository: string;
  standard_version: string;
  enabled: boolean;
  trusted: boolean;
  scope: PluginInstallScope;
  counts: PluginCounts;
  scaffolds: PluginScaffoldInfo[];
  extension_components: PluginExtensionComponent[];
  runtime_commands: PluginRuntimeCommandContribution[];
  ui_contributions: PluginUiContribution[];
  connector_contributions: PluginConnectorContribution[];
  lsp_servers: PluginLanguageServer[];
  diagnostics: PluginDiagnostic[];
}

export interface GitHubImportResult {
  plugin: PluginInfo;
}

export type MarketplacePluginSource =
  | { kind: "local"; path: string }
  | {
      kind: "github";
      repository: string;
      reference: string | null;
      sha: string | null;
    }
  | {
      kind: "git";
      url: string;
      path: string | null;
      reference: string | null;
      sha: string | null;
    }
  | {
      kind: "npm";
      package: string;
      version: string | null;
      registry: string | null;
    }
  | { kind: "archive"; url: string; sha256: string | null };

export interface MarketplacePlugin {
  name: string;
  display_name: string;
  description: string;
  version: string;
  category: string;
  installation_policy: string;
  authentication_policy: string;
  default_enabled: boolean;
  source: MarketplacePluginSource;
  installable: boolean;
  diagnostic: string | null;
}

export interface MarketplaceDiagnostic {
  code: string;
  message: string;
  entry: number | null;
}

export interface PluginMarketplace {
  name: string;
  display_name: string;
  description: string;
  manifest_path: string;
  root: string;
  plugins: MarketplacePlugin[];
  diagnostics: MarketplaceDiagnostic[];
}

const BROWSER_PLUGIN_MARKETPLACE: PluginMarketplace = {
  name: "code2-demo-marketplace",
  display_name: "C2 Marketplace Preview",
  description:
    "A browser-only preview of C2 marketplace source support and diagnostics.",
  manifest_path: "/demo/marketplace.json",
  root: "/demo",
  plugins: [
    {
      name: "local-review-suite",
      display_name: "Local Review Suite",
      description:
        "A local plugin bundle that can be installed by the desktop runtime.",
      version: "2.1.0",
      category: "development",
      installation_policy: "allowed",
      authentication_policy: "none",
      default_enabled: true,
      source: { kind: "local", path: "./plugins/local-review-suite" },
      installable: true,
      diagnostic: null,
    },
    {
      name: "npm-observability-demo",
      display_name: "NPM Observability Demo",
      description:
        "Catalog preview for an npm source that is not installable in this release.",
      version: "0.8.0",
      category: "monitoring",
      installation_policy: "allowed",
      authentication_policy: "none",
      default_enabled: false,
      source: {
        kind: "npm",
        package: "@example/observability-plugin",
        version: "0.8.0",
        registry: null,
      },
      installable: false,
      diagnostic:
        "npm marketplace sources are recognized but not installable in this release",
    },
  ],
  diagnostics: [],
};

export interface ScaffoldInstallResult {
  plugin: string;
  scaffold: string;
  destination: string;
  files: number;
}

export async function listPlugins(): Promise<PluginInfo[]> {
  return inDesktop
    ? await call<PluginInfo[]>("plugins.list", undefined, null)
    : [
        {
          id: "docker-tools-preview",
          name: "docker-tools",
          version: "0.1.0",
          description: "Inspect Docker and manage containers and images.",
          author: "C2",
          source: "Built-in renderer preview",
          repository: "https://github.com/IchenDEV/codeTwo",
          standard_version: "1.2.0",
          enabled: true,
          trusted: true,
          scope: "user",
          counts: {
            skills: 0,
            subagents: 0,
            mcp_servers: 0,
            scaffolds: 0,
            commands: 0,
            runtime_commands: 10,
            hooks: 0,
            lsp_servers: 0,
            monitors: 0,
            apps: 0,
            ui: 0,
            connectors: 0,
            scenes: 0,
            pipelines: 0,
            runtime: 1,
          },
          extension_components: [
            {
              kind: "runtime",
              name: "docker-tools",
              path: "plugin.js",
              status: "ready",
            },
          ],
          runtime_commands: [
            ["docker.status", "Docker status"],
            ["docker.containers", "List containers"],
            ["docker.images", "List images"],
            ["docker.inspect", "Inspect container"],
            ["docker.logs", "Read container logs"],
            ["docker.start", "Start container"],
            ["docker.stop", "Stop container"],
            ["docker.restart", "Restart container"],
            ["docker.pull", "Pull image"],
            ["docker.remove_image", "Remove image"],
          ].map(([id, title]) => ({
            id,
            title,
            description: "",
            argsSchema: null,
          })),
          ui_contributions: [],
          connector_contributions: [],
          lsp_servers: [],
          diagnostics: [],
          scaffolds: [],
        },
        {
          id: "developer-toolkit-demo",
          name: "Developer Toolkit",
          version: "1.4.0",
          description:
            "A complete development workflow with review, research, tools, and starter projects.",
          author: "C2 Community",
          source: "GitHub · example/developer-toolkit",
          repository: "https://github.com/example/developer-toolkit",
          standard_version: "1.2.0",
          enabled: true,
          trusted: false,
          scope: "user",
          counts: {
            skills: 1,
            subagents: 1,
            mcp_servers: 1,
            scaffolds: 1,
            commands: 1,
            hooks: 1,
            lsp_servers: 1,
            monitors: 0,
            apps: 0,
            ui: 0,
            connectors: 0,
            scenes: 1,
            pipelines: 1,
            runtime: 0,
            runtime_commands: 0,
          },
          extension_components: [
            {
              kind: "lsp",
              name: "rust",
              path: "plugin.json#extensions.dev.codetwo.languageServers",
              status: "requires_trust",
            },
            {
              kind: "hook",
              name: "session",
              path: "hooks/hooks.json",
              status: "unsupported",
            },
          ],
          diagnostics: [],
          runtime_commands: [],
          ui_contributions: [],
          connector_contributions: [],
          lsp_servers: [],
          scaffolds: [
            {
              id: "vite-react-demo",
              name: "Vite React app",
              description:
                "TypeScript, tests, and a production-ready project structure",
              files: 12,
            },
          ],
        },
        {
          id: "ui-lsp-demo",
          name: "UI & LSP Demo",
          version: "1.0.0",
          description:
            "Renderer-only preview of declarative plugin slots and language-server contributions.",
          author: "C2",
          source: "Built-in preview",
          repository: "",
          standard_version: "1.2.0",
          enabled: true,
          trusted: true,
          scope: "user",
          counts: {
            skills: 0,
            subagents: 0,
            mcp_servers: 0,
            scaffolds: 0,
            commands: 1,
            hooks: 0,
            lsp_servers: 1,
            monitors: 0,
            apps: 0,
            ui: 5,
            connectors: 0,
            scenes: 0,
            pipelines: 0,
            runtime: 1,
            runtime_commands: 1,
          },
          extension_components: [
            {
              kind: "ui",
              name: "Review tools",
              path: "rail.features",
              status: "ready",
            },
            {
              kind: "ui",
              name: "Review workspace",
              path: "session.header",
              status: "ready",
            },
            {
              kind: "ui",
              name: "Summarize thread",
              path: "transcript.before",
              status: "ready",
            },
            {
              kind: "ui",
              name: "Project health",
              path: "composer.above",
              status: "ready",
            },
            {
              kind: "ui",
              name: "Insert context",
              path: "composer.toolbar",
              status: "ready",
            },
            { kind: "lsp", name: "zig", path: "zig", status: "ready" },
          ],
          ui_contributions: [
            {
              id: "review-tools",
              slot: "rail.features",
              label: "Review tools",
              description: "Open the plugin's review workflow.",
              command: "demo.review",
              input: { mode: "tools" },
              order: 0,
            },
            {
              id: "review-workspace",
              slot: "session.header",
              label: "Review workspace",
              description: "Run the plugin's project review command.",
              command: "demo.review",
              input: null,
              order: 0,
            },
            {
              id: "summarize-thread",
              slot: "transcript.before",
              label: "Summarize thread",
              description: "Summarize the current conversation.",
              command: "demo.review",
              input: { mode: "summary" },
              order: 0,
            },
            {
              id: "project-health",
              slot: "composer.above",
              label: "Project health",
              description:
                "Ask the plugin to inspect this project before starting the next turn.",
              command: "demo.review",
              input: { mode: "health" },
              order: 0,
            },
            {
              id: "insert-context",
              slot: "composer.toolbar",
              label: "Insert context",
              description: "Insert plugin-provided context into this draft.",
              command: "demo.review",
              input: { mode: "context" },
              order: 0,
            },
          ],
          runtime_commands: [
            {
              id: "demo.review",
              title: "Review",
              description: "Run the demo review action.",
              argsSchema: null,
            },
          ],
          connector_contributions: [],
          lsp_servers: [
            {
              id: "zig",
              languages: ["zig"],
              command: "zls",
              args: [],
              env: {},
            },
          ],
          diagnostics: [],
          scaffolds: [],
        },
      ];
}

/** Refresh bundle descriptors after install, trust, lifecycle, or on-disk inventory changes. */
export async function onPluginsChanged(cb: () => void): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return listenDesktop<null>("plugins-changed", cb);
}

export interface DesktopRevealSessionEvent {
  session: string;
}

/** Receive a session-navigation request from an authenticated desktop host capability. */
export async function onDesktopRevealSession(
  cb: (event: DesktopRevealSessionEvent) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return listenDesktop<DesktopRevealSessionEvent>("desktop-reveal-session", cb);
}

export interface PluginConnectorEventEnvelope {
  plugin_id: string;
  event: unknown;
}

/** Receive one event from a connector Runtime after the host authenticates its owning bundle. */
export async function onPluginConnectorEvent(
  cb: (event: PluginConnectorEventEnvelope) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return listenDesktop<PluginConnectorEventEnvelope>(
    "plugin-connector-event",
    cb
  );
}

/** Invoke a manifest-declared UI action after the host verifies contribution and command ownership. */
export async function invokePluginUi(
  pluginId: string,
  contributionId: string,
  context: Record<string, unknown>,
  projectPath: string | null
): Promise<unknown> {
  if (!inDesktop)
    throw new Error("Plugin UI actions require the C2 desktop app.");
  return await call(
    "plugins.invoke_ui",
    {
      plugin_id: pluginId,
      contribution_id: contributionId,
      context,
    },
    projectPath
  );
}

/** Invoke one manifest-declared connector operation after runtime ownership checks. */
export async function invokePluginConnector<T = unknown>(
  pluginId: string,
  contributionId: string,
  operation: string,
  input: unknown,
  projectPath: string | null
): Promise<T> {
  if (!inDesktop)
    throw new Error("Plugin connectors require the C2 desktop app.");
  return await call<T>(
    "plugins.invoke_connector",
    {
      plugin_id: pluginId,
      contribution_id: contributionId,
      operation,
      input,
    },
    projectPath
  );
}

/** Install a complete plugin from a GitHub repository or a selected /tree/ path. */
export async function githubImportPlugin(
  repository: string
): Promise<GitHubImportResult> {
  if (!inDesktop)
    throw new Error("Plugin installation requires the C2 desktop app.");
  return await call<GitHubImportResult>(
    "plugins.import_github",
    { repository },
    null
  );
}

export async function pickPluginMarketplace(): Promise<PluginMarketplace | null> {
  if (!inDesktop) return BROWSER_PLUGIN_MARKETPLACE;
  const [selected] = await desktopOpenDialog({
    multiple: false,
    directory: false,
    filters: [{ name: "Plugin marketplace", extensions: ["json"] }],
  });
  if (!selected) return null;
  return await call<PluginMarketplace>(
    "plugins.read_marketplace",
    {
      path: selected,
    },
    null
  );
}

export async function installMarketplacePlugin(
  marketplacePath: string,
  pluginName: string
): Promise<GitHubImportResult> {
  if (!inDesktop)
    throw new Error("Marketplace installation requires the C2 desktop app.");
  return await call<GitHubImportResult>(
    "plugins.install_marketplace",
    {
      marketplace_path: marketplacePath,
      plugin_name: pluginName,
    },
    null
  );
}

export async function uninstallPlugin(
  id: string,
  keepData = false
): Promise<void> {
  if (inDesktop)
    await call("plugins.uninstall", { id, keep_data: keepData }, null);
}

export async function setPluginEnabled(
  id: string,
  enabled: boolean
): Promise<PluginInfo> {
  if (!inDesktop)
    throw new Error("Plugin state changes require the C2 desktop app.");
  return await call<PluginInfo>(
    "plugins.set_enabled",
    { id, value: enabled },
    null
  );
}

export async function setPluginTrusted(
  id: string,
  trusted: boolean
): Promise<PluginInfo> {
  if (!inDesktop)
    throw new Error("Plugin trust changes require the C2 desktop app.");
  return await call<PluginInfo>(
    "plugins.set_trusted",
    { id, value: trusted },
    null
  );
}

export async function applyPluginScaffold(
  pluginId: string,
  scaffoldId: string,
  cwd: string
): Promise<ScaffoldInstallResult> {
  if (!inDesktop)
    throw new Error("Scaffold installation requires the C2 desktop app.");
  return await call<ScaffoldInstallResult>(
    "plugins.apply_scaffold",
    {
      plugin_id: pluginId,
      scaffold_id: scaffoldId,
      cwd,
    },
    null
  );
}

// ---- remote control (F10) --------------------------------------------------------------------

export interface RemoteEndpoint {
  id: string;
  label: string;
  url: string;
  /** Loopback remains copyable for this Mac, but must never be encoded for another device. */
  qr_shareable: boolean;
}

export interface RemoteStatus {
  port: number;
  endpoints: RemoteEndpoint[];
  /** Older Rust hosts omit this field and support the original T3/legacy protocols. */
  protocols?: RemoteClientProtocol[];
}

export interface RemotePairingLink {
  endpoint_id: string;
  url: string;
  token: string;
  expires_in: number;
  qr_svg: string;
}

export interface RemoteDevice {
  id: string;
  name: string;
  created_at: number;
  last_seen: number;
  direction?: "incoming" | "outgoing";
  protocol?: RemoteClientProtocol;
}

/** Turn on network access: serve the live engine on all interfaces (idempotent). */
export async function startRemote(port?: number): Promise<RemoteStatus | null> {
  return inDesktop
    ? await call<RemoteStatus>("remote.start", { port: port ?? null })
    : null;
}

/** Turn off network access. Paired devices persist and reconnect next time. */
export async function stopRemote(): Promise<void> {
  if (inDesktop) await call("remote.stop");
}

export async function remoteStatus(): Promise<RemoteStatus | null> {
  return inDesktop ? await call<RemoteStatus | null>("remote.status") : null;
}

/** The wire protocol expected by the client consuming a pairing link. */
export type RemoteClientProtocol = "c2" | "t3" | "legacy";

/** Mint a fresh one-time pairing link for an advertised endpoint (URL + optional QR SVG). */
export async function remotePairingLink(
  endpointId?: string,
  clientProtocol: RemoteClientProtocol = "c2",
  ttlSecs?: number
): Promise<RemotePairingLink | null> {
  return inDesktop
    ? await call<RemotePairingLink>("remote.pairing_link", {
        endpoint_id: endpointId ?? null,
        client_protocol: clientProtocol,
        ttl_secs: ttlSecs ?? null,
      })
    : null;
}

export async function remoteDevices(): Promise<RemoteDevice[]> {
  return inDesktop ? await call<RemoteDevice[]>("remote.devices") : [];
}

export async function pairRemoteDevice(
  url: string,
  deviceName?: string
): Promise<{ device: RemoteDevice; sync: DeviceSyncStatus }> {
  if (!inDesktop)
    throw new Error("Device pairing requires the C2 desktop app.");
  return await call<{ device: RemoteDevice; sync: DeviceSyncStatus }>(
    "remote.pair_device",
    {
      url,
      device_name: deviceName ?? null,
    }
  );
}

export async function remoteRevokeDevice(id: string): Promise<boolean> {
  return inDesktop
    ? await call<boolean>("remote.revoke_device", { id })
    : false;
}

// ---- external MCP credentials -------------------------------------------------------------

export interface ExternalMcpStatus {
  enabled: boolean;
  endpoint: string | null;
  data_dir_configured: boolean;
  client_count: number;
  active_client_count: number;
}

export type ExternalMcpScope = "read" | "operate" | "approve" | "admin";

export type ExternalMcpTtlKind = "default" | "days90" | "days365" | "permanent";

export interface ExternalMcpClient {
  id: string;
  name: string;
  scopes: ExternalMcpScope[];
  created_at: string;
  expires_at?: string | null;
  last_used_at?: string | null;
  revoked_at?: string | null;
  status: string;
  expires_never: boolean;
  projects_label: string;
}

export interface ExternalMcpCreateInput {
  name: string;
  scopes: ExternalMcpScope[];
  projects: "all" | string[];
  ttl:
    | { kind: "default" }
    | { kind: "permanent" }
    | { kind: "days"; days: number };
}

export interface ExternalMcpCreateResult {
  record: ExternalMcpClient;
  token: string;
  config_snippet: Record<string, unknown>;
  expires_never: boolean;
}

export async function externalMcpStatus(): Promise<ExternalMcpStatus | null> {
  return inDesktop
    ? await call<ExternalMcpStatus>("external_mcp.status")
    : null;
}

export async function externalMcpSetEnabled(
  enabled: boolean
): Promise<ExternalMcpStatus | null> {
  return inDesktop
    ? await call<ExternalMcpStatus>("external_mcp.set_enabled", { enabled })
    : null;
}

export async function externalMcpClientsList(): Promise<ExternalMcpClient[]> {
  return inDesktop
    ? await call<ExternalMcpClient[]>("external_mcp.clients.list")
    : [];
}

export async function createExternalMcpClient(
  input: ExternalMcpCreateInput
): Promise<ExternalMcpCreateResult> {
  if (!inDesktop) {
    throw new Error(
      "External MCP credentials are only available in the desktop app"
    );
  }
  return await call<ExternalMcpCreateResult>(
    "external_mcp.clients.create",
    input
  );
}

export async function revokeExternalMcpClient(id: string): Promise<void> {
  if (inDesktop) await call("external_mcp.clients.revoke", { id });
}

export interface TaskHandoffResult {
  session: string;
  handoff: string;
  epoch: number;
  destination: string;
  state: "transferred";
}

export async function transferTaskToDevice(
  session: string,
  pairingUrl: string,
  destination: string
): Promise<TaskHandoffResult> {
  if (!inDesktop)
    throw new Error("Task transfer is only available in the desktop app");
  return await call<TaskHandoffResult>("handoff.transfer_pairing", {
    session,
    pairing_url: pairingUrl,
    destination,
  });
}

// ---- issues (F14) ----------------------------------------------------------------------------

export interface Issue {
  id: string;
  title: string;
  state: string;
  url: string;
  body: string;
  source: string;
}

export async function ghAvailable(): Promise<boolean> {
  return inDesktop ? await call<boolean>("issues.github_available") : false;
}
export async function listGithubIssues(
  cwd: string,
  limit = 30
): Promise<Issue[]> {
  return inDesktop
    ? await call<Issue[]>("issues.list_github", { cwd, limit })
    : [];
}
export async function listLinearIssues(
  token: string,
  limit = 30
): Promise<Issue[]> {
  return inDesktop
    ? await call<Issue[]>("issues.list_linear", { token, limit })
    : [];
}
export async function issueContext(issue: Issue): Promise<string> {
  if (inDesktop) return await call<string>("issues.context", { issue });
  return `**${issue.source} #${issue.id}** — ${issue.title} (${issue.state})\n${issue.url}`;
}

// ---- compiled-prompt preview (F13) -----------------------------------------------------------

export type CanvasPixelPolicy = "required" | "structure_only";
export type CanvasTheme = "light" | "dark";
export type CanvasObjectKind =
  | "pen"
  | "text"
  | "rectangle"
  | "ellipse"
  | "line"
  | "arrow"
  | "image";

export interface CanvasFeatureState {
  feature: string;
  enabled: boolean;
  status: "not production-enabled";
}

export interface CanvasPoint {
  x: number;
  y: number;
}

export interface CanvasRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CanvasObject {
  id: string;
  kind: CanvasObjectKind;
  originalText?: string;
  bounds: CanvasRect;
  layer: number;
  arrowStart?: CanvasPoint | null;
  arrowEnd?: CanvasPoint | null;
  assetId?: string | null;
}

export interface CanvasAssetRef {
  id: string;
  mimeType: "image/png" | "image/webp";
  width: number;
  height: number;
  sourceName?: string | null;
}

export interface CanvasStaticAsset extends CanvasAssetRef {
  bytes: number[];
}

export interface CanvasSceneEnvelope {
  engine: string;
  engineVersion: string;
  schemaVersion: number;
  revision: number;
  theme: CanvasTheme;
  assets: CanvasAssetRef[];
  /** Exact opaque Excalidraw scene retained by the core; no active refs are accepted on write. */
  scene: Record<string, unknown>;
}

export interface CanvasManifest {
  objects: CanvasObject[];
}

export type CanvasExportKind = "overview" | "detail";
export interface CanvasExport {
  id: string;
  kind: CanvasExportKind;
  index?: number | null;
  mimeType: "image/png";
  width: number;
  height: number;
  bytes: number[];
}

export interface CanvasDraftUpdate {
  title: string;
  theme: CanvasTheme;
  envelope: CanvasSceneEnvelope;
  manifest: CanvasManifest;
  assets?: CanvasStaticAsset[];
}

export interface CanvasFreezeInput extends CanvasDraftUpdate {
  exports?: CanvasExport[];
}

export interface CanvasDraft {
  id: string;
  owner: string;
  revision: number;
  title: string;
  theme: CanvasTheme;
  envelope: CanvasSceneEnvelope;
  manifest: CanvasManifest;
  assets: CanvasStaticAsset[];
  createdAt: number;
  updatedAt: number;
  tombstonedAt?: number | null;
}

/** Immutable history wire shape. Mutable owner/head timestamps are intentionally absent. */
export interface CanvasSnapshot {
  id: string;
  revision: number;
  title: string;
  theme: CanvasTheme;
  envelope: CanvasSceneEnvelope;
  manifest: CanvasManifest;
  assets: CanvasStaticAsset[];
  createdAt: number;
  frozenAt: number;
  objectCount: number;
  summary: string;
  exports: CanvasExport[];
}

export async function canvasFeatureState(): Promise<CanvasFeatureState> {
  return inDesktop
    ? await call<CanvasFeatureState>("canvas.feature_state")
    : await Promise.resolve({
        feature: "CODETWO_CANVAS_INPUT_V1",
        enabled: false,
        status: "not production-enabled",
      });
}

function canvasAssetToCore(asset: CanvasStaticAsset): Record<string, unknown> {
  return {
    id: asset.id,
    mime_type: asset.mimeType,
    width: asset.width,
    height: asset.height,
    bytes: asset.bytes,
  };
}

function canvasEnvelopeToCore(
  envelope: CanvasSceneEnvelope
): Record<string, unknown> {
  return {
    engine: envelope.engine,
    engine_version: envelope.engineVersion,
    schema_version: envelope.schemaVersion,
    revision: envelope.revision,
    theme: envelope.theme,
    assets: envelope.assets.map((asset) => ({
      id: asset.id,
      mime_type: asset.mimeType,
      width: asset.width,
      height: asset.height,
      source_name: asset.sourceName ?? null,
    })),
    scene: envelope.scene,
  };
}

function canvasManifestToCore(
  manifest: CanvasManifest
): Record<string, unknown> {
  return {
    objects: manifest.objects.map((object) => ({
      id: object.id,
      kind: object.kind,
      original_text: object.originalText ?? "",
      bounds: object.bounds,
      layer: object.layer,
      arrow_start: object.arrowStart ?? null,
      arrow_end: object.arrowEnd ?? null,
      asset_id: object.assetId ?? null,
    })),
  };
}

function canvasExportToCore(exportItem: CanvasExport): Record<string, unknown> {
  return {
    id: exportItem.id,
    kind: exportItem.kind,
    index: exportItem.index ?? null,
    mime_type: exportItem.mimeType,
    width: exportItem.width,
    height: exportItem.height,
    bytes: exportItem.bytes,
  };
}

function canvasUpdateToCore(
  update: CanvasDraftUpdate
): Record<string, unknown> {
  return {
    title: update.title,
    theme: update.theme,
    envelope: canvasEnvelopeToCore(update.envelope),
    manifest: canvasManifestToCore(update.manifest),
    assets: (update.assets ?? []).map(canvasAssetToCore),
  };
}

function canvasFreezeToCore(input: CanvasFreezeInput): Record<string, unknown> {
  return {
    ...canvasUpdateToCore(input),
    exports: (input.exports ?? []).map(canvasExportToCore),
  };
}

export async function canvasCreateDraft(title: string): Promise<CanvasDraft> {
  return await call<CanvasDraft>("canvas.create_draft", { title });
}

export async function canvasGetDraft(id: string): Promise<CanvasDraft | null> {
  return await call<CanvasDraft | null>("canvas.get_draft", { id });
}

export async function canvasUpdateDraft(
  id: string,
  expectedRevision: number,
  update: CanvasDraftUpdate
): Promise<CanvasDraft> {
  return await call<CanvasDraft>("canvas.update_draft", {
    id,
    expected_revision: expectedRevision,
    update: canvasUpdateToCore(update),
  });
}

export async function canvasNormalizeMedia(
  bytes: Uint8Array | number[],
  declaredMime?: string | null
): Promise<CanvasStaticAsset> {
  return await call<CanvasStaticAsset>("canvas.normalize_media", {
    bytes: [...bytes],
    declared_mime: declaredMime ?? null,
  });
}

/** Normalize and store a pasted or selected prompt image in app-private storage. */
export async function importPromptImage(
  bytes: Uint8Array | number[],
  declaredMime: string | null,
  name: string
): Promise<AppshotCapture> {
  return await call<AppshotCapture>("attachments.import", {
    bytes: [...bytes],
    declared_mime: declaredMime,
    name,
  });
}

/** Reload a private prompt image for durable transcript rendering. */
export async function getPromptImage(id: string): Promise<AppshotCapture> {
  if (!inDesktop) throw new Error("Prompt images require the desktop app");
  return await call<AppshotCapture>("attachments.get", { id });
}

export async function canvasFreeze(
  id: string,
  expectedRevision: number,
  input: CanvasFreezeInput
): Promise<CanvasSnapshot> {
  return await call<CanvasSnapshot>("canvas.freeze", {
    id,
    expected_revision: expectedRevision,
    input: canvasFreezeToCore(input),
  });
}

export async function canvasGetSnapshot(
  id: string,
  revision: number
): Promise<CanvasSnapshot | null> {
  return await call<CanvasSnapshot | null>("canvas.get_snapshot", {
    id,
    revision,
  });
}

export async function canvasGetAsset(
  id: string,
  revision: number,
  assetId: string
): Promise<CanvasStaticAsset | null> {
  return await call<CanvasStaticAsset | null>("canvas.get_asset", {
    id,
    revision,
    asset_id: assetId,
  });
}

export async function canvasGetExport(
  id: string,
  revision: number,
  exportId: string
): Promise<CanvasExport | null> {
  return await call<CanvasExport | null>("canvas.get_export", {
    id,
    revision,
    export_id: exportId,
  });
}

export async function canvasDuplicate(
  id: string,
  revision: number
): Promise<CanvasDraft> {
  return await call<CanvasDraft>("canvas.duplicate", { id, revision });
}

export async function canvasTombstone(id: string): Promise<void> {
  return await call("canvas.tombstone", { id });
}

export async function canvasRestore(id: string): Promise<void> {
  return await call("canvas.restore", { id });
}

export async function canvasPurge(id: string): Promise<boolean> {
  return await call<boolean>("canvas.purge", { id });
}

export interface CompiledPreview {
  prompt: string;
  mcp_servers: string[];
  agent_skills: string[];
  subagents: string[];
  files: string[];
  images: string[];
  sessions: string[];
  unresolved: string[];
  canvases: CompiledCanvasPreview[];
}

export interface CompiledCanvasPreview {
  id: string;
  frozenRevision: number;
  title: string;
  summary: string;
  /** Ordered overview first, then detail tiles as returned by core validation. */
  exports: CanvasExport[];
}

export async function compileDoc(
  doc: DocBlock[],
  cwd?: string | null
): Promise<CompiledPreview> {
  if (inDesktop)
    return await call<CompiledPreview>("document.compile", {
      doc,
      cwd: cwd ?? null,
    });
  return {
    prompt: doc.map(describeBlock).join("\n\n"),
    mcp_servers: [],
    agent_skills: [],
    subagents: [],
    files: doc.flatMap((b) => (b.type === "file" ? [b.path] : [])),
    images: doc.flatMap((b) => (b.type === "image" ? [b.path] : [])),
    sessions: doc.flatMap((b) => (b.type === "session" ? [b.session_id] : [])),
    unresolved: [],
    canvases: [],
  };
}

// ---- sandbox + project scripts (G7/G8) ---------------------------------------------------------

export async function setSandbox(
  session: string,
  sandbox: Sandbox
): Promise<void> {
  if (coreAvailable) await call("engine.set_sandbox", { session, sandbox });
}

export interface ProjectScript {
  id: string;
  name: string;
  kind: "command" | "prompt";
  command: string;
  prompt: string;
  keybinding: string;
  preview_url: string;
  run_on_worktree_create: boolean;
  open_preview: boolean;
}

export async function listProjectScripts(
  cwd: string
): Promise<ProjectScript[]> {
  return inDesktop
    ? await call<ProjectScript[]>("workspace.scripts", { cwd })
    : [];
}

export async function saveProjectScript(
  cwd: string,
  script: ProjectScript
): Promise<ProjectScript> {
  return inDesktop
    ? await call<ProjectScript>("workspace.save_script", { cwd, ...script })
    : script;
}

export async function runProjectScript(
  cwd: string,
  id: string
): Promise<string> {
  return inDesktop
    ? await call<string>("workspace.run_script", { cwd, id })
    : "";
}

// ---- voice input (G11) -------------------------------------------------------------------------

/// Whether the core has a configured local transcriber or a platform speech recognizer. The UI
/// prefers the webview's own speech recognition when present.
export async function voiceAvailable(): Promise<boolean> {
  return inDesktop ? await call<boolean>("voice.available") : false;
}

export async function transcribeAudio(
  bytes: Uint8Array,
  ext = "webm"
): Promise<string> {
  if (!inDesktop) return "";
  return await call<string>("voice.transcribe", { bytes: [...bytes], ext });
}

// ---- usage tracking (G12) ----------------------------------------------------------------------

export type ProviderQuotaStatus = "available" | "unavailable" | "unsupported";
export type ProviderQuotaReason =
  | "cli_not_found"
  | "query_failed"
  | "unsupported_provider";

export interface ProviderQuotaWindow {
  used_percent: number;
  window_minutes: number | null;
  /** Unix seconds, as reported by the provider. */
  resets_at: number | null;
}

export interface ProviderQuotaCredits {
  has_credits: boolean;
  unlimited: boolean;
  balance: string | null;
}

export interface ProviderQuotaReport {
  provider: string;
  status: ProviderQuotaStatus;
  reason: ProviderQuotaReason | null;
  source: string | null;
  plan: string | null;
  limit_name: string | null;
  windows: ProviderQuotaWindow[];
  credits: ProviderQuotaCredits | null;
  fetched_at_ms: number;
}

export async function providerQuota(
  provider: string
): Promise<ProviderQuotaReport> {
  if (inDesktop)
    return await call<ProviderQuotaReport>("usage.provider_quota", {
      provider,
    });
  return {
    provider,
    status: "unsupported",
    reason: "unsupported_provider",
    source: null,
    plan: null,
    limit_name: null,
    windows: [],
    credits: null,
    fetched_at_ms: Date.now(),
  };
}

export interface UsageWindow {
  label: string;
  window_secs: number;
  input_tokens: number;
  /// Cache reads — reported but excluded from `total_tokens`.
  cached_tokens: number;
  output_tokens: number;
  total_tokens: number;
  limit: number | null;
  fraction: number | null;
  resets_in_secs: number;
}

export interface UsageReport {
  windows: UsageWindow[];
  by_source: [string, number][];
  transcripts: number;
}

const EMPTY_USAGE: UsageReport = {
  windows: [
    {
      label: "5h session",
      window_secs: 18_000,
      input_tokens: 0,
      cached_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
      limit: null,
      fraction: null,
      resets_in_secs: 0,
    },
    {
      label: "week",
      window_secs: 604_800,
      input_tokens: 0,
      cached_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
      limit: null,
      fraction: null,
      resets_in_secs: 0,
    },
    {
      label: "month",
      window_secs: 2_592_000,
      input_tokens: 0,
      cached_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
      limit: null,
      fraction: null,
      resets_in_secs: 0,
    },
  ],
  by_source: [],
  transcripts: 0,
};

export async function usageReport(): Promise<UsageReport> {
  return inDesktop ? await call<UsageReport>("usage.report") : EMPTY_USAGE;
}

/** One provider's token totals per time bucket, oldest bucket first (cache reads excluded). */
export interface UsageSeries {
  source: string;
  totals: number[];
}

/** Time-bucketed usage for the trend chart; the last bucket is the partial one containing now. */
export interface UsageHistory {
  bucket_secs: number;
  bucket_count: number;
  /// Start of the first bucket (unix ms).
  start_ms: number;
  series: UsageSeries[];
}

/** Per-provider totals over the charted range, with a best-effort cost estimate. */
export interface SourceUsage {
  source: string;
  input_tokens: number;
  cached_tokens: number;
  output_tokens: number;
  total_tokens: number;
  /// Estimated spend over priceable records; null when nothing in this source was priceable.
  estimated_cost_usd: number | null;
  /// Tokens from records whose model is unknown/unpriced — not covered by the estimate.
  unpriced_tokens: number;
}

export interface UsageHistoryReport {
  history: UsageHistory;
  by_source: SourceUsage[];
}

const EMPTY_USAGE_HISTORY: UsageHistoryReport = {
  history: { bucket_secs: 86_400, bucket_count: 0, start_ms: 0, series: [] },
  by_source: [],
};

/** Bucketed usage history: `days <= 7` buckets hourly, otherwise daily. */
export async function usageHistory(days: number): Promise<UsageHistoryReport> {
  return inDesktop
    ? await call<UsageHistoryReport>("usage.history", { days })
    : EMPTY_USAGE_HISTORY;
}

// ---- workspace files & rules (G1/G2) ---------------------------------------------------------

const FALLBACK_FILES = ["src/main.rs", "src/lib.rs", "README.md"];

export async function listFiles(
  cwd: string,
  query: string,
  limit = 50
): Promise<string[]> {
  if (!inDesktop) return FALLBACK_FILES.filter((f) => f.includes(query));
  return await call<string[]>("workspace.list_files", { cwd, query, limit });
}

export interface WorkspaceSearchOptions {
  regex: boolean;
  case_sensitive: boolean;
  whole_word: boolean;
}

export interface WorkspaceContentMatch {
  path: string;
  line: number;
  column: number;
  preview: string;
}

export interface WorkspaceSearchResult {
  matches: WorkspaceContentMatch[];
  truncated: boolean;
  truncation_reason: string | null;
}

export async function searchWorkspaceContents(
  cwd: string,
  query: string,
  options: WorkspaceSearchOptions,
  requestId: string,
  limit = 200
): Promise<WorkspaceSearchResult> {
  if (!inDesktop)
    return { matches: [], truncated: false, truncation_reason: null };
  return await call<WorkspaceSearchResult>("workspace.search", {
    cwd,
    query,
    options,
    limit,
    request_id: requestId,
  });
}

export async function cancelWorkspaceContentSearch(
  requestId: string
): Promise<boolean> {
  return inDesktop
    ? await call<boolean>("workspace.cancel_search", { request_id: requestId })
    : false;
}

export async function listRules(cwd: string): Promise<string[]> {
  return inDesktop ? await call<string[]>("workspace.rules", { cwd }) : [];
}

// ---- session management (G5) -----------------------------------------------------------------

export async function renameSession(
  session: string,
  title: string
): Promise<void> {
  if (coreAvailable) await call("sessions.rename", { session, title });
}
export async function archiveSession(
  session: string,
  archived: boolean
): Promise<void> {
  if (coreAvailable)
    await call("sessions.set_archived", { session, value: archived });
}
export async function pinSession(
  session: string,
  pinned: boolean
): Promise<void> {
  if (coreAvailable)
    await call("sessions.set_pinned", { session, value: pinned });
}
export async function listArchivedSessions(): Promise<SessionInfo[]> {
  return coreAvailable ? await call<SessionInfo[]>("sessions.archived") : [];
}

// ---- PR + commit message (G6) ------------------------------------------------------------------

export async function gitCreatePr(
  cwd: string,
  title: string,
  body: string
): Promise<string> {
  return inDesktop
    ? await call<string>("git.create_pr", { cwd, title, body })
    : "";
}
export async function gitSuggestCommit(cwd: string): Promise<string> {
  return inDesktop
    ? await call<string>("git.suggest_message", { cwd })
    : "chore: update";
}

// ---- GitHub pull requests --------------------------------------------------------------------

export interface GitHubPullRequestSummary {
  id: string;
  number: number;
  title: string;
  url: string;
  repository: { name: string; nameWithOwner: string };
  author: { login: string };
  isDraft: boolean;
  updatedAt: string;
  createdAt: string;
  labels: { name: string; color: string }[];
  commentsCount: number;
  authored: boolean;
  reviewRequested: boolean;
  reviewed: boolean;
}

export interface GitHubPullRequestDetail extends GitHubPullRequestSummary {
  body: string;
  additions: number;
  deletions: number;
  changedFiles: number;
  baseRefName: string;
  headRefName: string;
  state: string;
  mergeStateStatus: string;
  mergeable: string;
  reviewDecision: string;
  reviewers: { login: string; state: string }[];
  checks: {
    name: string;
    status: string;
    conclusion: string;
    detailsUrl: string | null;
  }[];
  files: {
    path: string;
    additions: number;
    deletions: number;
    changeType: string;
  }[];
}

export async function listGitHubPullRequests(): Promise<
  GitHubPullRequestSummary[]
> {
  return inDesktop
    ? await call<GitHubPullRequestSummary[]>("github.pull_requests")
    : [];
}

export async function getGitHubPullRequest(
  summary: GitHubPullRequestSummary
): Promise<GitHubPullRequestDetail> {
  if (!inDesktop)
    throw new Error("GitHub pull requests require the desktop host");
  return await call<GitHubPullRequestDetail>("github.pull_request", {
    url: summary.url,
    summary,
  });
}

export async function browserContext(annotation: Annotation): Promise<string> {
  // Mirrors core::browser::Annotation::to_context without requiring a native browser plugin.
  let s = `**Browser context** — ${annotation.url}`;
  if (annotation.selected_text != null && annotation.selected_text !== "")
    s += `\n- selected: “${annotation.selected_text}”`;
  if (annotation.note) s += `\n- note: ${annotation.note}`;
  return s;
}

export async function saveSkill(skill: Skill): Promise<void> {
  if (inDesktop) await call("skills.save", { skill });
}

export async function deleteSkill(id: string): Promise<void> {
  if (inDesktop) await call("skills.delete", { id });
}

export async function onEngineEvent(
  cb: (ev: CoreEvent) => void
): Promise<() => void> {
  if (!coreAvailable)
    return () => {
      /* empty */
    };
  return listenCore<CoreEvent>("engine-event", cb);
}

export async function onPtyOutput(
  cb: (p: PtyOutput) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return listenCore<PtyOutput>("pty-output", cb);
}

export async function onPtyTitle(
  cb: (p: PtyTitle) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return listenCore<PtyTitle>("pty-title", cb);
}

/** Fires when a terminal's child process exits. */
export async function onPtyExit(
  cb: (event: PtyExit) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return listenCore<PtyExit>("pty-exit", cb);
}

export function providerLabel(p: string | { custom: string }): string {
  return typeof p === "string" ? p : p.custom;
}

// ---- scenes (Agent Scenes 1.0.0; see docs/reference/scenes.md) ---------------------------------------

import type { SceneDocument, SceneInfo, SceneSlotDef } from "./session/scene";

export type SceneSaveScope = "user" | "project";

export interface SceneEscalation {
  from: string;
  to: string;
}

export interface SceneApplyOutcome {
  applied: string[];
  pending: string[];
  escalation: SceneEscalation | null;
  suppress_unpinned: boolean;
  pinned_skills: string[];
}

export interface SceneSessionParams {
  provider: string | null;
  model: string | null;
  reasoning_effort: string | null;
  use_worktree: boolean | null;
  worktree_base: WorktreeBaselineKind | null;
  initial_policy: { mode: PermissionMode; sandbox: Sandbox } | null;
}

export interface SceneSessionPlanOutcome {
  params: SceneSessionParams | null;
  escalation: SceneEscalation | null;
}

export interface SessionSceneState {
  reference: string;
  customized: boolean;
  resolved: boolean;
}

export interface AutoSceneChanged {
  session: string;
  reference: string;
  title: string;
  reason: string;
  pending: string[];
  memoryRead: MemoryAccess;
  memoryWrite: MemoryAccess;
}

export async function onAutoSceneChanged(
  cb: (event: AutoSceneChanged) => void
): Promise<() => void> {
  if (!inDesktop)
    return () => {
      /* empty */
    };
  return listenDesktop<AutoSceneChanged>("auto-scene-changed", cb);
}

/// Every scene call degrades on a missing backend command (feature-detect: catch → fallback),
/// so a frontend running against an older core hides the affordance instead of breaking.
/** Browser-preview stand-ins (same convention as FALLBACK_SKILLS): the five builtin scenes. */
const FALLBACK_SCENES: SceneInfo[] = (
  [
    [
      "research",
      "Research",
      "调研",
      "read_only",
      "Survey the problem space read-only and produce a cited research report.",
    ],
    [
      "develop",
      "Develop",
      "开发",
      "auto_edit",
      "Implementation in an isolated worktree.",
    ],
    [
      "test",
      "Test",
      "测试",
      "auto_edit",
      "Exercise the change against its acceptance criteria.",
    ],
    [
      "fix",
      "Fix",
      "修复",
      "auto_edit",
      "Resolve reported failures one by one.",
    ],
    [
      "acceptance",
      "Acceptance",
      "验收",
      "read_only",
      "Read-only verification against the original acceptance criteria.",
    ],
  ] as const
).map(([name, title, zh, mode, description]) => ({
  reference: `builtin:${name}`,
  name,
  title,
  description,
  icon: null,
  source: "builtin" as const,
  keywords: [],
  has_brief: true,
  localizations: { "zh-CN": { title: zh } },
  execution: { session_mode: mode } as SceneInfo["execution"],
  artifacts: [],
}));

export async function listScenes(cwd?: string): Promise<SceneInfo[]> {
  if (!inDesktop) return FALLBACK_SCENES;
  return await call<SceneInfo[]>("scenes.list", { cwd: cwd ?? null }).catch(
    () => []
  );
}

export async function getScene(
  reference: string
): Promise<{ reference: string; source: string; scene: SceneDocument } | null> {
  if (!inDesktop) return null;
  return await call<{
    reference: string;
    source: string;
    scene: SceneDocument;
  }>("scenes.get", {
    reference,
  }).catch(() => null);
}

export async function saveScene(
  scope: SceneSaveScope,
  cwd: string | null,
  previousName: string | null,
  scene: SceneDocument
): Promise<SceneInfo> {
  if (!inDesktop) {
    return {
      reference: `${scope}:${scene.name}`,
      name: scene.name,
      title: scene.title,
      description: scene.description ?? "",
      icon: scene.icon ?? null,
      source: scope,
      plugin_id: null,
      keywords: scene.keywords ?? [],
      has_brief: Boolean(scene.brief),
      localizations: scene.localizations ?? {},
      execution: scene.execution ?? null,
      brief: scene.brief ?? null,
      artifacts: scene.artifacts ?? [],
      skills: scene.skills ?? null,
      exit: scene.exit ?? null,
    };
  }
  return await call<SceneInfo>("scenes.save", {
    scope,
    cwd,
    previous_name: previousName,
    scene,
  });
}

export async function deleteScene(
  scope: SceneSaveScope,
  cwd: string | null,
  name: string
): Promise<void> {
  if (!inDesktop) return;
  await call("scenes.delete", { scope, cwd, name });
}

export async function applySceneToSession(
  session: string,
  reference: string,
  confirmEscalation: boolean
): Promise<SceneApplyOutcome | null> {
  if (!inDesktop) return null;
  return await call<SceneApplyOutcome>("scenes.apply", {
    session,
    reference,
    confirm_escalation: confirmEscalation,
  }).catch(() => null);
}

export async function sceneSessionPlan(
  reference: string,
  confirmEscalation: boolean
): Promise<SceneSessionPlanOutcome | null> {
  if (!inDesktop) return null;
  return await call<SceneSessionPlanOutcome>("scenes.session_plan", {
    reference,
    confirm_escalation: confirmEscalation,
  }).catch(() => null);
}

export async function setSessionScene(
  session: string,
  reference: string | null,
  customized: boolean
): Promise<void> {
  if (!inDesktop) return;
  await call("scenes.set_session", { session, reference, customized }).catch(
    () => {
      /* empty */
    }
  );
}

export async function getSessionScene(
  session: string
): Promise<SessionSceneState | null> {
  if (!inDesktop) return null;
  return await call<SessionSceneState | null>("scenes.session", {
    session,
  }).catch(() => null);
}

export async function setSessionAutoScene(
  session: string,
  enabled: boolean
): Promise<void> {
  if (!inDesktop) return;
  await call("scenes.set_auto", { session, enabled });
}

export async function getSessionAutoScene(session: string): Promise<boolean> {
  if (!inDesktop) return false;
  return await call<boolean>("scenes.auto", { session }).catch(() => false);
}

/** Diff stat of a session's own checkout, shaped for display. Null when unknown or not a repo. */
export interface SessionDiffStat {
  files: number;
  additions: number;
  deletions: number;
}

export async function sessionDiffStat(
  session: string
): Promise<SessionDiffStat | null> {
  if (!inDesktop) return null;
  return await call<GitDiffStat | null>("sessions.diff_stat", { session })
    .then((stat) =>
      stat
        ? { files: stat.files, additions: stat.added, deletions: stat.deleted }
        : null
    )
    .catch(() => null);
}

/** Per-session token/cost totals. The core command lands in a later wave — until then this
 * resolves null everywhere and the statusline's cost segment stays hidden (feature detect). */
export interface SessionUsage {
  input_tokens: number;
  output_tokens: number;
  cost_usd: number | null;
  burn_rate_usd_per_hour: number | null;
  priced: boolean;
}

export async function usageBySession(
  session: string
): Promise<SessionUsage | null> {
  if (!inDesktop) return null;
  return await call<SessionUsage | null>("cost.session", { session }).catch(
    () => null
  );
}

// ---- scene artifacts (R4) -------------------------------------------------------------------

/** One captured version of one scene artifact, as `list_scene_artifacts` reports it. */
export interface SceneArtifactRecord {
  id: number;
  scene_ref: string;
  artifact_key: string;
  kind: string;
  title: string;
  session_id: string;
  pipeline_instance_id: string | null;
  stage_id: string | null;
  artifact: ArtifactRef;
  version: number;
  pinned: boolean;
  created_at: number;
}

/// Same degradation contract as the scene calls above: against an older core the commands are
/// missing and every call quietly reports "no artifacts" instead of breaking the surface.
export async function listSceneArtifacts(
  session: string
): Promise<SceneArtifactRecord[]> {
  if (!inDesktop) return [];
  return await call<SceneArtifactRecord[]>("scene_artifacts.list", {
    session,
  }).catch(() => []);
}

export async function sceneArtifactContent(
  recordId: number
): Promise<string | null> {
  if (!inDesktop) return null;
  return await call<string>("scene_artifacts.content", {
    record_id: recordId,
  }).catch(() => null);
}

export async function recordSceneArtifact(
  session: string,
  artifactKey: string,
  content: string
): Promise<SceneArtifactRecord | null> {
  if (!inDesktop) return null;
  return await call<SceneArtifactRecord>("scene_artifacts.record", {
    session,
    artifact_key: artifactKey,
    content,
  }).catch(() => null);
}

export async function pinSceneArtifact(
  session: string,
  artifactKey: string,
  version: number | null
): Promise<void> {
  if (!inDesktop) return;
  await call("scene_artifacts.pin", {
    session,
    artifact_key: artifactKey,
    version,
  }).catch(() => {
    /* empty */
  });
}

// ---- issue write path (R12) -----------------------------------------------------------------

/**
 * Post a delegation comment on a GitHub ("github") or Linear ("linear") issue; resolves to the
 * comment URL. Linear needs the caller-held API token (same source as `listLinearIssues`).
 * Null when unavailable: browser preview, older core, or the comment failed to post.
 */
export async function commentIssue(
  cwd: string,
  source: string,
  id: string,
  body: string,
  token?: string
): Promise<string | null> {
  if (!inDesktop) return null;
  return await call<string>("issues.comment", {
    cwd,
    source,
    id,
    body,
    token: token ?? null,
  }).catch(() => null);
}

// ---- voice → structured brief (R11) --------------------------------------------------------

/**
 * Heuristically distribute a finished dictation across a scene brief's slots (core-side keyword
 * sectioning; no model call). Null on browser preview, an older core, or any failure — the caller
 * must fall back to inserting the raw transcript so it is never lost.
 */
export async function structureBrief(
  transcript: string,
  slots: SceneSlotDef[]
): Promise<Record<string, string> | null> {
  if (!inDesktop) return null;
  return await call<Record<string, string>>("issues.structure_brief", {
    transcript,
    slots,
  }).catch(() => null);
}

// ---- template-from-history (R2) --------------------------------------------------------------

/** Heuristic `{{slot-N}}` proposal over a past prompt, as `propose_macro_slots` reports it. */
export interface ProposedMacro {
  template: string;
  slots: SceneSlotDef[];
}

/**
 * Null on browser preview or an older core (missing command) — the template dialog then opens as
 * a plain manual editor over the raw prompt instead of breaking.
 */
export async function proposeMacroSlots(
  text: string
): Promise<ProposedMacro | null> {
  if (!inDesktop) return null;
  return await call<ProposedMacro>("skills.propose_macro", { text }).catch(
    () => null
  );
}

// ---- scene hooks (R8) -----------------------------------------------------------------------

/// Remember a completion-banner dismissal so the same exit state never re-fires this session.
export async function dismissSceneBanner(
  session: string,
  stateKey: string
): Promise<void> {
  if (!inDesktop) return;
  await call("scenes.dismiss_banner", { session, state_key: stateKey }).catch(
    () => {
      /* empty */
    }
  );
}

/// Enable/disable scene `schedule` hooks for one project (off by default).
export async function setProjectScheduling(
  path: string,
  enabled: boolean
): Promise<void> {
  if (!inDesktop) return;
  await call("scenes.set_scheduling", { path, enabled }).catch(() => {
    /* empty */
  });
}

// ---- pipeline instances (R9) ----------------------------------------------------------------

/** One resolved pipeline definition, as `list_pipelines` reports it. */
export interface PipelineInfo {
  reference: string;
  name: string;
  title: string;
  description: string;
  icon: string | null;
  source: string;
  stage_count: number;
}

/** One running (or finished) pipeline instance. */
export interface PipelineInstance {
  id: string;
  pipeline_ref: string;
  project_path: string;
  current_stage: string;
  status: string;
  created_at: number;
  updated_at: number;
}

export interface PipelineTransitionRecord {
  instance_id: string;
  seq: number;
  from_stage: string | null;
  to_stage: string;
  trigger: string;
  gate: string;
  session_id: string | null;
  created_at: number;
}

/** One stage on the horizontal stage track. */
export interface PipelineStageStatus {
  id: string;
  scene_ref: string;
  title: string;
  state: "done" | "current" | "pending";
  gate: string;
  loop_count: number;
  sessions: string[];
  artifacts: SceneArtifactRecord[];
}

export interface PipelineInstanceDetail {
  instance: PipelineInstance;
  transitions: PipelineTransitionRecord[];
  stages: PipelineStageStatus[];
}

export interface PipelineStartOutcome {
  detail: PipelineInstanceDetail;
  applied_scene: SceneApplyOutcome | null;
}

export interface PipelineAdvanceOutcome {
  instance: PipelineInstance;
  applied_scene: SceneApplyOutcome | null;
  session_plan: SceneSessionParams | null;
  escalation: SceneEscalation | null;
  carried: string[];
}

/// Same degradation contract as the other scene calls: on an older core every call quietly
/// reports "no pipelines" instead of breaking the surface.
export async function listPipelines(): Promise<PipelineInfo[]> {
  if (!inDesktop) return [];
  return await call<PipelineInfo[]>("pipelines.list", {}).catch(() => []);
}

export async function startPipeline(
  reference: string,
  projectPath: string,
  session: string | null
): Promise<PipelineStartOutcome | null> {
  if (!inDesktop) return null;
  return await call<PipelineStartOutcome>("pipelines.start", {
    reference,
    project_path: projectPath,
    session,
  }).catch(() => null);
}

export async function advancePipeline(
  instanceId: string,
  toStage: string,
  session: string | null,
  confirm: boolean
): Promise<PipelineAdvanceOutcome | null> {
  if (!inDesktop) return null;
  return await call<PipelineAdvanceOutcome>("pipelines.advance", {
    instance_id: instanceId,
    to_stage: toStage,
    session,
    confirm,
  }).catch(() => null);
}

export async function bindPipelineSession(
  instanceId: string,
  stageId: string,
  session: string
): Promise<void> {
  if (!inDesktop) return;
  await call("pipelines.bind_session", {
    instance_id: instanceId,
    stage_id: stageId,
    session,
  }).catch(() => {
    /* empty */
  });
}

export async function getPipelineInstance(
  instanceId: string
): Promise<PipelineInstanceDetail | null> {
  if (!inDesktop) return null;
  return await call<PipelineInstanceDetail>("pipelines.instance", {
    instance_id: instanceId,
  }).catch(() => null);
}

export async function listPipelineInstances(
  projectPath: string
): Promise<PipelineInstance[]> {
  if (!inDesktop) return [];
  return await call<PipelineInstance[]>("pipelines.instances", {
    project_path: projectPath,
  }).catch(() => []);
}

/** The active session's pipeline binding — the stage track renders only when this is set. */
export async function sessionPipeline(
  session: string
): Promise<{ instance_id: string; stage_id: string } | null> {
  if (!inDesktop) return null;
  return await call<{ instance_id: string; stage_id: string } | null>(
    "pipelines.session",
    {
      session,
    }
  ).catch(() => null);
}

// ---- issue delegation trail (R12 cleanup) ----------------------------------------------------

/** One "delegated to scene" event on an issue's accountability trail. */
export interface IssueDelegation {
  id: number;
  source: string;
  issue_id: string;
  issue_title: string;
  scene_ref: string;
  scene_title: string;
  session_id: string | null;
  comment_url: string | null;
  created_at: number;
}

export async function recordIssueDelegation(
  source: string,
  issueId: string,
  issueTitle: string,
  sceneRef: string,
  sceneTitle: string
): Promise<number | null> {
  if (!inDesktop) return null;
  return await call<number>("issues.record_delegation", {
    source,
    issue_id: issueId,
    issue_title: issueTitle,
    scene_ref: sceneRef,
    scene_title: sceneTitle,
  }).catch(() => null);
}

export async function setIssueDelegationSession(
  id: number,
  session: string
): Promise<void> {
  if (!inDesktop) return;
  await call("issues.set_delegation_session", { id, session }).catch(() => {
    /* empty */
  });
}

export async function setIssueDelegationComment(
  id: number,
  url: string
): Promise<void> {
  if (!inDesktop) return;
  await call("issues.set_delegation_comment", { id, url }).catch(() => {
    /* empty */
  });
}

export async function listIssueDelegations(
  source: string,
  issueId: string
): Promise<IssueDelegation[]> {
  if (!inDesktop) return [];
  return await call<IssueDelegation[]>("issues.delegations", {
    source,
    issue_id: issueId,
  }).catch(() => []);
}

/** Whether scene `schedule` hooks are enabled for this project (off by default). */
export async function getProjectScheduling(path: string): Promise<boolean> {
  if (!inDesktop) return false;
  return await call<boolean>("scenes.scheduling", { path }).catch(() => false);
}

/** Lossy SKILL.md export of a scene (docs/reference/scenes.md §Interop); null when it cannot resolve. */
export async function exportSceneSkillMd(
  reference: string
): Promise<string | null> {
  if (!inDesktop) return null;
  return await call<string>("scenes.export_skill_md", { reference }).catch(
    () => null
  );
}

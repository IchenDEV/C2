import { useEffect, useState } from "react";

import { NavigationRow } from "@/components/business/navigation-row";
import { Button } from "@/components/ui/button";
import {
  ArrowLeft,
  BrainCircuit,
  ChartNoAxesColumn,
  Download,
  Folder,
  GitBranch,
  Globe,
  Keyboard,
  MousePointer2,
  Package,
  Palette,
  PawPrint,
  RefreshCw,
  RotateCcw,
  ScanText,
  Server,
  SlidersHorizontal,
  UserRound,
  Wrench,
} from "@/components/ui/icons";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cssVars } from "@/lib/cssVars";

import { resetVisualAppearanceSettings } from "../appearance";
import {
  checkForAppUpdates,
  confirmNative,
  discardOrphanWorktree,
  discardSessionWorktree,
  getAppUpdateStatus,
  getWorktreeSettings,
  listProjectWorktrees,
  updateWorktreeSettings,
} from "../bridge";
import type {
  AppshotSettings,
  BrowserUseSettings,
  ComputerUseSettings,
  CustomProviderConfiguration,
  AppUpdateStatus,
  DeviceSyncStatus,
  DiagnosticsExportResult,
  KeymapEntry,
  Project,
  ProjectWorktreeMode,
  ProviderInfo,
  ProviderRuntimeOverride,
  SessionImportResult,
  WorktreeSettings,
  PluginDeveloperStatus,
} from "../bridge";
import { remoteEnvironmentsAvailable } from "../coreTransport";
import { useLanguage, useT } from "../i18n";
import type { StringKey } from "../i18n/strings";
import { useTheme } from "../theme";
import { UsagePanel } from "../usage/Usage";
import { AppearanceSettings } from "./AppearanceSettings";
import { AppshotsSettingsPage } from "./AppshotsSettings";
import { MemorySettingsPage } from "./MemorySettings";
import {
  BrowserPermissionsSettingsPage,
  BrowserUseSettingsPage,
  ComputerUseSettingsPage,
  DeveloperSettingsPage,
  DeviceSyncSettingsPage,
} from "./OperationalSettings";
import {
  GeneralSettingsPage,
  ImportSettingsPage,
  KeybindingsSettingsPage,
} from "./PersonalSettings";
import { PetSettings } from "./PetSettings";
import { ProfileSettings } from "./ProfileSettings";
import { ProjectSettingsPage } from "./ProjectSettings";
import { ProviderSettingsPage } from "./ProviderSettings";
import { RemoteEnvironmentsSettingsPage } from "./RemoteEnvironmentsSettings";
import { Page } from "./SettingsPrimitives";
import { WorktreeSettingsPage } from "./WorktreeSettings";

import "./settings-page.css";

export type SettingsTab =
  | "general"
  | "import"
  | "profile"
  | "appearance"
  | "pets"
  | "project"
  | "worktrees"
  | "memory"
  | "sync"
  | "environments"
  | "keybindings"
  | "providers"
  | "computer-use"
  | "appshots"
  | "browser-use"
  | "usage"
  | "developer"
  | "browser";

interface SettingsNavItem {
  id: SettingsTab;
  icon: typeof Keyboard;
  labelKey: StringKey;
}

const NAV_GROUPS: {
  id: "personal" | "workspace" | "integrations";
  labelKey: StringKey;
  items: SettingsNavItem[];
}[] = [
  {
    id: "personal",
    labelKey: "settings.navPersonal",
    items: [
      { id: "general", icon: SlidersHorizontal, labelKey: "settings.general" },
      { id: "import", icon: Download, labelKey: "settings.import" },
      { id: "profile", icon: UserRound, labelKey: "profile.title" },
      { id: "appearance", icon: Palette, labelKey: "settings.appearance" },
      { id: "pets", icon: PawPrint, labelKey: "settings.pets" },
      { id: "keybindings", icon: Keyboard, labelKey: "settings.keybindings" },
      { id: "usage", icon: ChartNoAxesColumn, labelKey: "usage.title" },
    ],
  },
  {
    id: "workspace",
    labelKey: "settings.navWorkspace",
    items: [
      { id: "project", icon: Folder, labelKey: "settings.project" },
      { id: "worktrees", icon: GitBranch, labelKey: "settings.worktrees" },
      { id: "memory", icon: BrainCircuit, labelKey: "memory.title" },
      { id: "sync", icon: RefreshCw, labelKey: "settings.sync" },
      { id: "environments", icon: Server, labelKey: "settings.environments" },
    ],
  },
  {
    id: "integrations",
    labelKey: "settings.navIntegrations",
    items: [
      { id: "providers", icon: Package, labelKey: "settings.providers" },
      {
        id: "computer-use",
        icon: MousePointer2,
        labelKey: "settings.computerUse",
      },
      { id: "appshots", icon: ScanText, labelKey: "settings.appshots" },
      { id: "browser-use", icon: Globe, labelKey: "settings.browserUse" },
      { id: "browser", icon: Globe, labelKey: "settings.browser" },
      { id: "developer", icon: Wrench, labelKey: "settings.developer" },
    ],
  },
];

const EMPTY_PROJECTS: Project[] = [];

/**
 * Settings as a full-window page: its own nav rail on the left (General, Memory, Keybindings,
 * Providers, Usage)
 * with a Back row above the category menu, and one scrolling column of rows per category. The
 * window-wide takeover is deliberate — a settings surface with its own sidebar reads as a *place*
 * you went to, which is what earns the explicit way back.
 */
export function SettingsPage({
  onSelectProject,
  onOpenDevices,
  sidebarWidth = 288,
  bindings,
  capturing,
  onCapture,
  onReset,
  onResetAll,
  providers,
  provider,
  projectPath,
  project,
  projects = EMPTY_PROJECTS,
  onProjectWorktreeMode,
  onProjectRename = async () => {
    /* empty */
  },
  onProjectIcon = async () => {
    /* empty */
  },
  onProjectAgentDefaults = async () => {
    /* empty */
  },
  onProjectRemove = async () => {
    /* empty */
  },
  projectIconPicker,
  projectActionsCount = 0,
  onAddProjectAction = () => {
    /* empty */
  },
  onOpenSession = () => {
    /* empty */
  },
  sessionImporter,
  onSessionsImported = async () => {
    /* empty */
  },
  worktreeLister = listProjectWorktrees,
  worktreeSettingsLoader = getWorktreeSettings,
  worktreeSettingsSaver = updateWorktreeSettings,
  sessionWorktreeDiscarder = discardSessionWorktree,
  orphanWorktreeDiscarder = discardOrphanWorktree,
  worktreeDiscardConfirmer = confirmNative,
  onReloadProviders,
  memoryEnabled,
  deviceSyncEnabled = true,
  initialTab = "general",
  onClose,
  updateStatusLoader = getAppUpdateStatus,
  updateCheckStarter = checkForAppUpdates,
  computerUseSettingsLoader,
  computerUseSelectionSaver,
  browserUseSettingsLoader,
  browserUseSelectionSaver,
  browserUseAccessSaver,
  appshotSettingsLoader,
  appshotSettingsSaver,
  appshotPermissionRequester,
  appshotPrivacyOpener,
  appshotCapturer,
  providerInstaller,
  providerUpgrader,
  providerEnabledSaver,
  providerConfigurationSaver,
  customProviderRegistrar,
  customProviderRemover,
  customProviderRemoveConfirmer,
  deviceSyncStatusLoader,
  deviceSyncEnabledSaver,
  deviceSyncStarter,
  pluginDeveloperStatusLoader,
  pluginDeveloperModeSaver,
  pluginDeveloperReloader,
  devtoolsOpener,
  diagnosticsExporter,
}: {
  /** Matches the persisted width of the main session rail. */
  onSelectProject?: (path: string) => void;
  onOpenDevices?: () => void;
  sidebarWidth?: number;
  bindings: KeymapEntry[];
  capturing: string | null;
  onCapture: (action: string) => void;
  onReset?: (action: string) => void;
  /** Restore every shortcut to the shipped default — the header's "Restore defaults" on that tab. */
  onResetAll?: () => void;
  providers: ProviderInfo[];
  provider: string;
  projectPath: string;
  project: Project | null;
  projects?: Project[];
  onProjectWorktreeMode: (
    path: string,
    mode: ProjectWorktreeMode | null
  ) => Promise<void>;
  onProjectRename?: (path: string, name: string) => Promise<void>;
  onProjectIcon?: (path: string, source: string | null) => Promise<void>;
  onProjectAgentDefaults?: (
    path: string,
    provider: string | null,
    model: string | null,
    reasoningEffort: string | null
  ) => Promise<void>;
  onProjectRemove?: (path: string) => Promise<void>;
  projectIconPicker?: () => Promise<string | null>;
  projectActionsCount?: number;
  onAddProjectAction?: () => void;
  onOpenSession?: (sessionId: string) => void;
  sessionImporter?: (
    fallbackCwd: string
  ) => Promise<SessionImportResult | null>;
  onSessionsImported?: () => void | Promise<unknown>;
  worktreeLister?: typeof listProjectWorktrees;
  worktreeSettingsLoader?: () => Promise<WorktreeSettings>;
  worktreeSettingsSaver?: (
    settings: WorktreeSettings
  ) => Promise<WorktreeSettings>;
  sessionWorktreeDiscarder?: typeof discardSessionWorktree;
  orphanWorktreeDiscarder?: typeof discardOrphanWorktree;
  worktreeDiscardConfirmer?: typeof confirmNative;
  onReloadProviders?: () => void | Promise<ProviderInfo[]>;
  memoryEnabled: boolean;
  deviceSyncEnabled?: boolean;
  initialTab?: SettingsTab;
  onClose: () => void;
  updateStatusLoader?: () => Promise<AppUpdateStatus>;
  updateCheckStarter?: () => Promise<AppUpdateStatus>;
  computerUseSettingsLoader?: () => Promise<ComputerUseSettings>;
  computerUseSelectionSaver?: (backend: string) => Promise<ComputerUseSettings>;
  browserUseSettingsLoader?: () => Promise<BrowserUseSettings>;
  browserUseSelectionSaver?: (backend: string) => Promise<BrowserUseSettings>;
  browserUseAccessSaver?: (enabled: boolean) => Promise<BrowserUseSettings>;
  appshotSettingsLoader?: () => Promise<AppshotSettings>;
  appshotSettingsSaver?: (
    patch: Partial<
      Pick<AppshotSettings, "hotkey" | "destination" | "play_sound">
    >
  ) => Promise<AppshotSettings>;
  appshotPermissionRequester?: (
    kind: "screen-recording" | "accessibility"
  ) => Promise<AppshotSettings>;
  appshotPrivacyOpener?: (
    kind: "screen-recording" | "accessibility"
  ) => Promise<boolean>;
  appshotCapturer?: () => Promise<unknown>;
  providerInstaller?: (provider: string) => Promise<ProviderInfo[]>;
  providerUpgrader?: (provider: string) => Promise<ProviderInfo[]>;
  providerEnabledSaver?: (
    provider: string,
    enabled: boolean
  ) => Promise<ProviderInfo[]>;
  providerConfigurationSaver?: (
    provider: string,
    configuration: ProviderRuntimeOverride
  ) => Promise<ProviderInfo[]>;
  customProviderRegistrar?: (
    configuration: CustomProviderConfiguration
  ) => Promise<void>;
  customProviderRemover?: (provider: string) => Promise<void>;
  customProviderRemoveConfirmer?: (message: string) => Promise<boolean>;
  deviceSyncStatusLoader?: () => Promise<DeviceSyncStatus>;
  deviceSyncEnabledSaver?: (enabled: boolean) => Promise<DeviceSyncStatus>;
  deviceSyncStarter?: () => Promise<DeviceSyncStatus>;
  pluginDeveloperStatusLoader?: () => Promise<PluginDeveloperStatus>;
  pluginDeveloperModeSaver?: (
    enabled: boolean
  ) => Promise<PluginDeveloperStatus>;
  pluginDeveloperReloader?: () => Promise<PluginDeveloperStatus>;
  devtoolsOpener?: () => Promise<void>;
  diagnosticsExporter?: () => Promise<DiagnosticsExportResult>;
}) {
  const t = useT();
  const { preference: theme, setPreference: setTheme } = useTheme();
  const { setPreference: setLanguage } = useLanguage();
  const providerNames = Object.fromEntries(
    providers.map((candidate) => [candidate.id, candidate.display_name])
  );
  const [tab, setTab] = useState<SettingsTab>(initialTab);
  const [projectNavigationLocked, setProjectNavigationLocked] = useState(false);
  useEffect(() => setTab(initialTab), [initialTab]);
  useEffect(() => {
    if (!memoryEnabled) {
      setTab((current) => (current === "memory" ? "general" : current));
    }
  }, [memoryEnabled]);
  useEffect(() => {
    if (!deviceSyncEnabled) {
      setTab((current) => (current === "sync" ? "general" : current));
    }
  }, [deviceSyncEnabled]);

  // What "Restore defaults" means depends on where you're standing.
  const restore = () => {
    if (tab === "general") {
      setLanguage("system");
    } else if (tab === "appearance") {
      resetVisualAppearanceSettings();
    } else if (tab === "keybindings") {
      onResetAll?.();
    }
  };

  return (
    <div className="animate-page-in flex min-h-0 min-w-0 flex-1">
      {/* ---- nav rail — same material as the app's rail, so settings still feels like this app */}
      <aside
        data-settings-sidebar
        className="settings-sidebar glass-rail flex shrink-0 flex-col"
        style={cssVars({
          "--settings-sidebar-width": `${Math.min(420, Math.max(220, sidebarWidth))}px`,
        })}
      >
        {/* Same 46px title bar as the main shell — clears the traffic lights and drags the window. */}
        <div className="electrobun-webkit-app-region-drag settings-titlebar shrink-0" />
        <Button
          data-settings-back
          type="button"
          variant="ghost"
          size="row"
          disabled={projectNavigationLocked}
          onClick={onClose}
          aria-label={t("settings.back")}
          title={t("settings.back")}
          className="text-muted-foreground mx-3 mb-2 w-auto"
        >
          <ArrowLeft className="size-3.5 shrink-0" />
          <span className="settings-back-label">{t("settings.back")}</span>
        </Button>
        <nav
          aria-label={t("settings.title")}
          className="min-h-0 flex-1 space-y-6 overflow-y-auto overscroll-contain px-3 pt-2 pb-6"
        >
          <div className="space-y-6">
            {NAV_GROUPS.map((group) => {
              const items = group.items
                .filter(({ id }) => memoryEnabled || id !== "memory")
                .filter(({ id }) => deviceSyncEnabled || id !== "sync")
                .filter(
                  ({ id }) =>
                    remoteEnvironmentsAvailable || id !== "environments"
                );
              const headingId = `settings-nav-${group.id}`;
              return (
                <section
                  key={group.id}
                  aria-label={t(group.labelKey)}
                  aria-labelledby={headingId}
                >
                  <h2
                    id={headingId}
                    className="settings-nav-heading text-metadata text-muted-foreground px-2 pb-2 font-medium"
                  >
                    {t(group.labelKey)}
                  </h2>
                  <div className="space-y-1">
                    {items.map(({ id, icon: Icon, labelKey }) => (
                      <NavigationRow
                        key={id}
                        className="settings-nav-item"
                        labelClassName="settings-nav-label"
                        label={t(labelKey)}
                        leading={<Icon />}
                        current={id === tab}
                        accessibilityLabel={t(labelKey)}
                        tooltip={t(labelKey)}
                        onSelect={() => setTab(id)}
                      />
                    ))}
                  </div>
                </section>
              );
            })}
          </div>
        </nav>
      </aside>

      {/* ---- the page ---- */}
      <main className="bg-background flex min-w-0 flex-1 flex-col overflow-hidden">
        {/* The same 46px bar as the main shell's header, border and all. */}
        <header
          data-settings-titlebar
          className="electrobun-webkit-app-region-drag settings-titlebar flex shrink-0 items-center gap-1.5 border-b pt-1.5 pr-3 pb-1.5 pl-6"
        >
          <span className="electrobun-webkit-app-region-drag text-body text-muted-foreground font-medium">
            {tab === "profile" ? t("profile.title") : t("settings.title")}
          </span>
          <div className="electrobun-webkit-app-region-drag flex-1" />
          {(tab === "general" ||
            tab === "appearance" ||
            tab === "keybindings") && (
            <Button
              variant="ghost"
              size="sm"
              className="gap-control-group text-metadata text-muted-foreground hover:text-foreground"
              onClick={restore}
            >
              <RotateCcw className="size-3.5" />
              {tab === "general"
                ? t("settings.restoreLanguage")
                : t("settings.restoreDefaults")}
            </Button>
          )}
        </header>

        <ScrollArea key={tab} className="min-h-0 flex-1">
          <div className="settings-page mx-auto w-full">
            {tab === "project" && onSelectProject && (
              <Select
                value={projectPath}
                onValueChange={(path) => {
                  if (path != null && path !== "") onSelectProject(path);
                }}
              >
                <SelectTrigger
                  aria-label={t("settings.project")}
                  className="mb-4 w-full"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {projects.map((item) => (
                    <SelectItem key={item.path} value={item.path}>
                      {item.name} · {item.path}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
            {tab === "general" && (
              <GeneralSettingsPage
                statusLoader={updateStatusLoader}
                checkStarter={updateCheckStarter}
              />
            )}

            {tab === "import" && (
              <ImportSettingsPage
                projectPath={projectPath}
                importer={sessionImporter}
                onImported={onSessionsImported}
                onOpenSession={onOpenSession}
              />
            )}

            {tab === "appearance" && (
              <Page
                title={t("settings.appearance")}
                description={t("settings.appearanceHint")}
              >
                <AppearanceSettings value={theme} onChange={setTheme} />
              </Page>
            )}

            {tab === "profile" && (
              <ProfileSettings providerNames={providerNames} />
            )}

            {tab === "pets" && (
              <Page title={t("settings.pets")}>
                <PetSettings />
              </Page>
            )}

            {tab === "sync" && deviceSyncEnabled && (
              <DeviceSyncSettingsPage
                onOpenDevices={onOpenDevices}
                loader={deviceSyncStatusLoader}
                enabledSaver={deviceSyncEnabledSaver}
                syncStarter={deviceSyncStarter}
              />
            )}

            {tab === "environments" && remoteEnvironmentsAvailable && (
              <RemoteEnvironmentsSettingsPage />
            )}

            {tab === "keybindings" && (
              <KeybindingsSettingsPage
                bindings={bindings}
                capturing={capturing}
                onCapture={onCapture}
                onReset={onReset}
              />
            )}

            {tab === "project" && (
              <ProjectSettingsPage
                project={project}
                providers={providers}
                onWorktreeMode={onProjectWorktreeMode}
                onRename={onProjectRename}
                onIcon={onProjectIcon}
                onAgentDefaults={onProjectAgentDefaults}
                onRemove={onProjectRemove}
                iconPicker={projectIconPicker}
                actionsCount={projectActionsCount}
                onAddAction={onAddProjectAction}
                onModeSavingChange={setProjectNavigationLocked}
              />
            )}

            {tab === "worktrees" && (
              <WorktreeSettingsPage
                projects={projects}
                onOpenSession={onOpenSession}
                lister={worktreeLister}
                settingsLoader={worktreeSettingsLoader}
                settingsSaver={worktreeSettingsSaver}
                sessionDiscarder={sessionWorktreeDiscarder}
                orphanDiscarder={orphanWorktreeDiscarder}
                confirmer={worktreeDiscardConfirmer}
              />
            )}

            {tab === "memory" && memoryEnabled && (
              <MemorySettingsPage
                projectPath={projectPath}
                projects={projects}
                onOpenSession={onOpenSession}
              />
            )}

            {tab === "usage" && (
              <UsagePanel
                provider={provider}
                providerName={
                  providers.find((candidate) => candidate.id === provider)
                    ?.display_name ?? provider
                }
                providerNames={providerNames}
              />
            )}

            {tab === "computer-use" && (
              <ComputerUseSettingsPage
                loader={computerUseSettingsLoader}
                saver={computerUseSelectionSaver}
              />
            )}

            {tab === "appshots" && (
              <AppshotsSettingsPage
                loader={appshotSettingsLoader}
                saver={appshotSettingsSaver}
                permissionRequester={appshotPermissionRequester}
                privacyOpener={appshotPrivacyOpener}
                capturer={appshotCapturer}
              />
            )}

            {tab === "browser-use" && (
              <BrowserUseSettingsPage
                loader={browserUseSettingsLoader}
                saver={browserUseSelectionSaver}
                accessSaver={browserUseAccessSaver}
              />
            )}

            {tab === "providers" && (
              <ProviderSettingsPage
                providers={providers}
                reload={onReloadProviders}
                installer={providerInstaller}
                upgrader={providerUpgrader}
                enabledSaver={providerEnabledSaver}
                configurationSaver={providerConfigurationSaver}
                customProviderRegistrar={customProviderRegistrar}
                customProviderRemover={customProviderRemover}
                customProviderRemoveConfirmer={customProviderRemoveConfirmer}
              />
            )}
            {tab === "developer" && (
              <DeveloperSettingsPage
                loader={pluginDeveloperStatusLoader}
                modeSaver={pluginDeveloperModeSaver}
                reloader={pluginDeveloperReloader}
                devtoolsOpener={devtoolsOpener}
                diagnosticsExporter={diagnosticsExporter}
              />
            )}

            {tab === "browser" && <BrowserPermissionsSettingsPage />}
          </div>
        </ScrollArea>
      </main>
    </div>
  );
}

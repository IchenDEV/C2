import {
  lazy,
  Suspense,
  useCallback,
  useMemo,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type { MutableRefObject } from "react";
import { flushSync } from "react-dom";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { Keyboard } from "@/components/ui/icons";
import {
  Archive,
  Check,
  Folder,
  FolderPlus,
  PanelLeft,
  Search,
  SquareKanban,
} from "@/components/ui/icons";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import {
  Tooltip,
  TooltipButton,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { asJsonObject, isOneOf, stringField } from "@/lib/jsonValue";
import { usePersistedNumber } from "@/lib/persist";
import { useLatestRef, useStableCallback } from "@/lib/useLatestRef";
import { cn } from "@/lib/utils";

import {
  answerElicitation,
  answerPermission,
  applyPluginScaffold,
  applyPluginChange,
  archiveSession,
  browserRegistryCreate,
  browserContext,
  canvasCreateDraft,
  canvasDuplicate,
  canvasFeatureState,
  canvasGetDraft,
  canvasNormalizeMedia,
  canvasPurge,
  canvasRestore,
  canvasFreeze,
  canvasTombstone,
  canvasUpdateDraft,
  cancelTurn,
  call,
  compileDoc,
  confirmNative,
  addProject,
  DEFAULT_KEYMAP,
  defaultCwd,
  describeBlock,
  discardSessionWorktree,
  getKeymap,
  getAppshot,
  getPromptImage,
  getTranscriptPage,
  gitCheckpoint,
  gitCheckpoints,
  gitCommit,
  gitDiffStat,
  gitPush,
  gitRevert,
  gitSourceControlInfo,
  gitStatus,
  githubCurrentPullRequest,
  githubImportPlugin,
  installMarketplacePlugin,
  importPromptImage,
  invokePluginConnector,
  invokePluginUi,
  commentIssue,
  issueContext,
  listArchivedSessions,
  listMemoryReceipts,
  lspSetRuntimeEnabled,
  listProjectScripts,
  listProjects,
  listProviders,
  listSessions,
  listSkills,
  listWorktreeBaselines,
  marketCatalog,
  marketInstall,
  planPluginChange,
  newSession,
  onBrowserAgentActivity,
  onBrowserDownloadBlocked,
  onAutoSceneChanged,
  onAppshotCaptured,
  onAppshotFailed,
  onDeviceSyncChanged,
  onDesktopRevealSession,
  onPluginConnectorEvent,
  onPluginsChanged,
  onEngineEvent,
  openProject,
  openWorkspace,
  pickPluginMarketplace,
  pinSession,
  pickDirectory,
  providerQuota,
  providerLabel,
  prepareSession,
  queuePrompt,
  removeProject,
  resetManagedPlugin,
  renameProject,
  renameSession,
  runProjectScript,
  saveProjectScript,
  saveSkill,
  searchSessions,
  sessionPreviews,
  setSystemBadgeCount,
  setConfigOption,
  setCallProjectPath,
  setKeymap,
  setModel,
  switchProvider,
  setPluginEnabled,
  setPluginTrusted,
  setProjectAgentDefaults,
  setProjectIcon,
  setProjectWorktreeMode,
  setSessionMemoryPolicy,
  setExecutionPolicy,
  steerPrompt,
  submitPrompt,
  uninstallPlugin,
  advancePipeline,
  applySceneToSession,
  dismissSceneBanner,
  bindPipelineSession,
  getPipelineInstance,
  recordIssueDelegation,
  setIssueDelegationComment,
  setIssueDelegationSession,
  getSessionScene,
  getSessionAutoScene,
  listPipelines,
  listScenes,
  sceneSessionPlan,
  sessionPipeline,
  setModel as setSessionModel,
  setSessionScene,
  setSessionAutoScene,
  startPipeline,
  structureBrief,
  usageBySession,
} from "./bridge";
import type {
  WorkspaceOpenTarget,
  Checkpoint,
  CanvasDraft,
  CanvasExport,
  CanvasFeatureState,
  CanvasPixelPolicy,
  CanvasSceneEnvelope,
  CanvasStaticAsset,
  CanvasSnapshot,
  CompiledPreview,
  ConfigOptionInfo,
  CoreEvent,
  DocBlock,
  ElicitationAnswer,
  ExecutionPolicy,
  Annotation,
  AppshotCapture,
  GitStatus,
  GitHubPullRequest,
  GitHubPullRequestDetail,
  Issue,
  KeymapEntry,
  MarketItem,
  MemoryAccess,
  MemoryReceipt,
  ModelChoice,
  PluginMarketplace,
  ManagedPluginCatalog,
  Project,
  ProjectScript,
  ProjectWorktreeMode,
  ProviderInfo,
  ProviderQuotaReport,
  PermissionMode,
  Sandbox,
  SessionActivity,
  SessionInfo,
  SourceControlInfo,
  SessionInteractionCapabilities,
  SkillInfo,
  WorktreeBaselineKind,
  WorktreeBaselineOption,
  WorkspaceContentMatch,
  PipelineInfo,
  PipelineInstanceDetail,
} from "./bridge";
import { BrowserPanel } from "./browser/Browser";
import {
  loadBrowserHistory,
  removeBrowserProject,
  saveBrowserHistory,
} from "./browser/history";
import { deriveCanvasManifest } from "./canvas/manifest";
import type { CanvasEnvelope as LocalCanvasEnvelope } from "./canvas/types";
import {
  Dock,
  shouldOverlayRailForDock,
  shouldOverlayRailForWorkspace,
} from "./dock/Dock";
import type { DockSurface, DockTab } from "./dock/Dock";
import type { DockerCommandCaller } from "./docker/DockerPage";
import { DocEditor } from "./editor/Editor";
import type { CanvasInsertOptions } from "./editor/Editor";
import { PreviewModal } from "./editor/Preview";
import { EnvironmentPopover } from "./environment/EnvironmentPopover";
import { FeishuWorkspacePage } from "./feishu/FeishuWorkspacePage";
import type {
  CollaborationConnectorEvent,
  CollaborationConnectorSubscriber,
} from "./feishu/FeishuWorkspacePage";
import { dirtyKey, isDirty as isFileDirty, markDirty } from "./files/dirty";
import { FileBrowserModal } from "./files/FileBrowser";
import { FileDockContent } from "./files/FileDockContent";
import type { FileRevealTarget } from "./files/FileViewer";
import { WorkspaceSearchModal } from "./files/WorkspaceSearch";
import { GitDockContent, PullRequestDockContent } from "./git/GitDockContent";
import { resolveGitNextAction } from "./git/nextAction";
import { SourceControlModal } from "./git/SourceControl";
import { workspaceStateForCwd } from "./git/state";
import type { WorkspaceLoadState } from "./git/state";
import { githubPullRequestReference } from "./github/pullRequests";
import type { PullRequestTaskLinkTarget } from "./github/PullRequestsPage";
import { useLanguage, useT } from "./i18n";
import { issueDeliveryApi, issueStageLabel } from "./issues/issueDelivery";
import type { IssueConnector, IssueDelivery } from "./issues/issueDelivery";
import {
  IssueConnectorSettings,
  IssueDeliveryDialog,
} from "./issues/IssueDeliveryDialog";
import { IssuesModal } from "./issues/Issues";
import {
  actionForEvent,
  comboFromEvent,
  isModifierOnly,
  keyHint,
} from "./keys";
import { configurePluginLanguageServers } from "./lsp/client";
import { synchronizeLspRuntimePolicy } from "./lsp/runtimePolicy";
import { CommandPalette } from "./palette/CommandPalette";
import type { Command } from "./palette/CommandPalette";
import { DesktopPetBridge } from "./pet/DesktopPet";
import {
  petAnimationForActivity,
  petConversationBubbleForActivity,
} from "./pet/state";
import { currentDesktopPlatform } from "./platform";
import {
  PluginUiSlot,
  activePluginConnectorContributions,
  activePluginLanguageServers,
  activePluginUiContributions,
  buildPluginManagerCatalog,
  createPluginManagerLabels,
  localizePluginManagerCatalog,
  normalizePluginProjectPath,
  pluginManagerComponentEnabled,
  toManagedPluginScope,
} from "./plugins";
import type {
  ActivePluginUiContribution,
  BuiltinUiComponentId,
  PluginManagerChangePlan,
  PluginManagerChangeRequest,
  PluginManagerScope,
} from "./plugins";
import {
  applyPluginManagerChange,
  planPluginManagerChange,
} from "./plugins/lifecycle";
import { usePluginSnapshot } from "./plugins/usePluginSnapshot";
import { loadProviderRegistry } from "./providers/registry";
import { RemoteModal } from "./remote/Remote";
import { Composer } from "./session/Composer";
import {
  composerDraftScopeKey,
  loadComposerDrafts,
  promoteComposerDraft,
  saveComposerDrafts,
  updateComposerDraft,
} from "./session/composerDrafts";
import type {
  ComposerDraft,
  ComposerDraftAttachment,
  ComposerDraftPosture,
  ComposerDraftScope,
} from "./session/composerDrafts";
import type { SessionConfig } from "./session/config";
import {
  activeContextWindow,
  clearContextWindow,
  updateContextWindow,
} from "./session/contextWindow";
import type { ContextWindowBySession } from "./session/contextWindow";
import { workspaceRelativeLinkPath } from "./session/MarkdownContent";
import type { BuiltinLinkTarget } from "./session/MarkdownContent";
import {
  SESSION_MODES,
  executionPolicyChangeDisabled,
  nextSessionMode,
  sessionExecutionPolicy,
  sessionMode,
  toSessionMode,
  withSessionExecutionPolicy,
} from "./session/mode";
import type { SessionMode } from "./session/mode";
import { PaneLayoutToolbar } from "./session/PaneChrome";
import {
  singlePaneLayout,
  splitFocused,
  closePane,
  focusPane,
  setSplitRatio,
  listPanes,
} from "./session/paneLayout";
import type { PaneLayout, PaneEdge } from "./session/paneLayout";
import { PaneTiles } from "./session/PaneTiles";
import { PermissionCard } from "./session/PermissionCard";
import { ProjectActionDialog } from "./session/ProjectActionDialog";
import { projectActionBindings } from "./session/projectActions";
import {
  nextSessionWorktreeBaseline,
  projectSwitchWorktreeBaseline,
} from "./session/projectDefaults";
import { QuestionDialog } from "./session/QuestionDialog";
import {
  escalationNeeded,
  nextSceneInRing,
  sceneCustomized,
  softApplyPending,
  MEMORY_PRESET_POLICY,
  sceneEffortChoice,
} from "./session/scene";
import type { SceneInfo } from "./session/scene";
import { SceneBanner, sceneBannerFromEvent } from "./session/SceneBanner";
import type { SceneBannerState } from "./session/SceneBanner";
import { SceneEscalationDialog, ScenePicker } from "./session/SceneChip";
import type { SceneEditorRequest } from "./session/SceneEditor";
import { SceneStudio } from "./session/SceneStudio";
import {
  activeSessionWorktreeState,
  enqueuePermission,
  activityIsBusy,
  isTerminalSessionEvent,
  latestActivity,
  matchesSessionCreation,
  permissionQueueAfterAnswer,
  permissionQueueAfterActivity,
  pendingInputsForSession,
  permissionsFromSessions,
  paneBoundToSession,
  sessionCreationBaseline,
  sessionCreationBaselineSha,
  sessionCreationReceipt,
  sessionCreationSource,
  sessionActivity,
  sessionProjectPath,
  sessionShellWithReceipt,
  shouldRenderSessionEvent,
} from "./session/sessionEvents";
import type {
  PermissionQueueItem,
  SessionCreationShell,
} from "./session/sessionEvents";
import { SessionHeaderActions } from "./session/SessionHeaderActions";
import { SessionTitlePair } from "./session/SessionTitlePair";
import { QuickChatPanel, SideChatPanel } from "./session/SideChatPanel";
import type { TransientChatSeed } from "./session/SideChatPanel";
import { StageTrack } from "./session/StageTrack";
// Explicit extension: `session/` holds both `statusline.ts` and `Statusline.tsx`, and bun's
// resolver matches the pair case-insensitively without it.
import { deriveBurnRate } from "./session/statusline.ts";
import { TaskHandoffDialog } from "./session/TaskHandoffDialog";
import { TemplateDialog } from "./session/TemplateDialog";
import {
  activeInteractivePreview,
  classifyToolSurface,
  followReduce,
  initialFollowState,
} from "./session/toolActivity";
import type {
  FollowEvent,
  FollowState,
  ToolSurfaceHint,
} from "./session/toolActivity";
import { TrajectoryView } from "./session/TrajectoryView";
import { TranscriptPane } from "./session/TranscriptPane";
import {
  applyEvent,
  canvasAcceptedRequestKey,
  canvasIdsToPurgeAfterTurnStart,
  canvasRetryDocument,
  canvasUnmountPlan,
  canvasRetryRefsForTerminal,
  isCanvasProviderImageError,
  matchesSubmittedEditorRevision,
  mergeLoadedTurns,
  newTurn,
  prependTranscriptTurns,
  transcriptTailState,
  turnsFromTranscript,
  withRunningSession,
  withoutUnacceptedTurn,
} from "./session/turns";
import type { PromptImage, Turn } from "./session/turns";
import type { TranscriptScrollController } from "./session/useTranscriptScroll";
import { SettingsPage } from "./settings/SettingsPage";
import type { SettingsTab } from "./settings/SettingsPage";
import { needsMeCount } from "./sidebar/missionControl.ts";
import { MissionControlPage } from "./sidebar/MissionControl.tsx";
import { APP_NAV_WIDTH, SessionRail } from "./sidebar/SessionRail";
import type { CanvasBlockRuntime } from "./skillInline";
import {
  associateTaskSession,
  associateTaskPullRequest,
  createBoardTask,
  githubPullRequestIdentity,
  loadBoardSnapshot,
  nextTaskNumber,
  saveBoardSnapshot,
  taskForPullRequest,
  taskForSession,
  unlinkTaskPullRequest,
} from "./taskboard/taskBoard";
import type { BoardTask } from "./taskboard/taskBoard";
import { continueTaskBoardPrompt } from "./taskboard/taskBoardContinuation";
import { TerminalDockContent } from "./terminal/TerminalDockContent";
import { useToast } from "./ui/toast";
import { quickQuotaProviderFor, quickQuotaSummary } from "./usage/quickQuota";
import { makeTranscriptHandler } from "./voice/VoiceButton";

const TaskBoardPage = lazy(
  async () =>
    await import("./taskboard/TaskBoardPage").then((module) => ({
      default: module.TaskBoardPage,
    }))
);
const PullRequestsPage = lazy(
  async () =>
    await import("./github/PullRequestsPage").then((module) => ({
      default: module.PullRequestsPage,
    }))
);
const AutomationsPage = lazy(
  async () =>
    await import("./automation/AutomationsPage").then((module) => ({
      default: module.AutomationsPage,
    }))
);
const PluginManagerPage = lazy(
  async () =>
    await import("./plugins/PluginManagerPage").then((module) => ({
      default: module.PluginManagerPage,
    }))
);
const DockerPage = lazy(
  async () =>
    await import("./docker/DockerPage").then((module) => ({
      default: module.DockerPage,
    }))
);

function PageLoadingFallback() {
  const t = useT();
  return (
    <div
      role="status"
      className="bg-background text-body text-muted-foreground flex min-h-0 min-w-0 flex-1 items-center justify-center gap-2"
    >
      <Spinner />
      {t("session.loading")}
    </div>
  );
}

function summarizeDoc(doc: DocBlock[]): string {
  return doc.map(describeBlock).join("\n\n");
}

function privateImageBlock(capture: AppshotCapture): DocBlock {
  return capture.kind === "attachment"
    ? { type: "attachment", id: capture.id, name: capture.window_title }
    : { type: "appshot", id: capture.id, title: capture.window_title };
}

function composerDraftAttachmentKey(scope: ComposerDraftScope): string {
  return scope.kind === "session"
    ? scope.sessionId
    : `draft:${scope.projectPath || "."}`;
}

function composerDraftAttachments(
  captures: readonly AppshotCapture[]
): ComposerDraftAttachment[] {
  return captures.map((capture) => ({
    id: capture.id,
    kind: capture.kind === "attachment" ? "attachment" : "appshot",
    name: capture.window_title,
  }));
}

function promptImagesForTurn(
  captures: readonly AppshotCapture[]
): PromptImage[] {
  return captures.flatMap((capture) =>
    capture.kind === "attachment"
      ? [
          {
            id: capture.id,
            name: capture.window_title,
            previewDataUrl: capture.preview_data_url,
            width: capture.width,
            height: capture.height,
          },
        ]
      : []
  );
}

const EMPTY_APPSHOTS: AppshotCapture[] = [];
/** Shared empty transcript so a pane with no turns reads a stable reference every render. */
const EMPTY_TURNS: Turn[] = [];

interface PendingPromptRequest {
  requestId: string;
  /** Pane whose editor produced this request; acceptance must never follow later focus changes. */
  paneId: string;
  /** Raw editor revision at the moment this immutable request was submitted. */
  editorSnapshot: DocBlock[];
  editorRevision: number;
  /** Exact submitted prompt, retained for an explicit provider retry after Composer clear. */
  submittedDoc: DocBlock[];
  /** Canvas heads included in this request; marked for mutable-head purge only after TurnStarted. */
  canvasIds: string[];
  /** Frozen immutable revisions retained for an explicit provider-error retry after Composer clear. */
  canvasRefs: { id: string; revision: number }[];
  /** Private Appshot captures attached to this turn; removed from Composer only after acceptance. */
  appshotIds: string[];
  /** Prompt actions submit their own document and must leave the user's Composer draft untouched. */
  clearEditor: boolean;
}

interface PendingCreation {
  /** Pane that owned the draft when session creation started. */
  paneId: string;
  doc: DocBlock[];
  /** Frozen Composer document retained for a provider retry. */
  canvasRetryDoc: DocBlock[];
  promptRequestId: string;
  editorSnapshot: DocBlock[];
  editorRevision: number;
  appshotIds: string[];
  clearEditor: boolean;
  /** Task staged for this creation, independent of whichever pane is focused when the event lands. */
  boardTaskId: string | null;
  autoScene: boolean;
  memoryRead: MemoryAccess;
  memoryWrite: MemoryAccess;
  /** Project default resolved before the provider publishes its session-owned selector id. */
  projectReasoningEffort: string | null;
}

interface NewSessionRunTarget {
  source: string;
  worktreeBase: WorktreeBaselineKind;
  worktreeBaseSha: string;
  parallelTask?: boolean;
}

interface PendingPolicyRequest {
  session: string;
  authoritative: ExecutionPolicy;
}

function localCanvasManifest(
  envelope: LocalCanvasEnvelope
): import("./bridge").CanvasManifest {
  const manifest = deriveCanvasManifest(envelope.elements);
  return {
    objects: manifest.objects.map((object) => ({
      id: object.id,
      kind: object.type === "freedraw" ? "pen" : object.type,
      originalText: object.originalText ?? undefined,
      bounds: object.geometry,
      layer: object.layer,
      arrowStart: object.arrowStart,
      arrowEnd: object.arrowEnd,
      assetId:
        object.type === "image"
          ? (() => {
              const element = envelope.elements.find(
                (candidate) => candidate.id === object.id
              );
              const fileId =
                element != null &&
                "fileId" in element &&
                typeof element.fileId === "string"
                  ? element.fileId
                  : null;
              return (
                envelope.assetRefs.find((asset) => asset.fileId === fileId)
                  ?.ref ?? fileId
              );
            })()
          : null,
    })),
  };
}

function localCanvasScene(
  envelope: LocalCanvasEnvelope,
  assets: readonly CanvasStaticAsset[]
): CanvasSceneEnvelope {
  return {
    engine: envelope.engine,
    engineVersion: envelope.engineVersion,
    schemaVersion: envelope.schemaVersion,
    revision: envelope.revision,
    theme: envelope.theme,
    assets: assets.map((asset) => ({
      id: asset.id,
      mimeType: asset.mimeType,
      width: asset.width,
      height: asset.height,
      sourceName: asset.sourceName ?? null,
    })),
    scene: {
      elements: envelope.elements,
      appState: envelope.appState,
    },
  };
}

interface GitWorkspaceData {
  status: GitStatus | null;
  diffStat: { added: number; deleted: number; truncated: boolean };
  sourceControl: SourceControlInfo | null;
  pullRequest: GitHubPullRequest | null;
  forgeError: string | null;
}

const EMPTY_DIFF_STAT = { added: 0, deleted: 0, truncated: false } as const;
const EMPTY_GIT_WORKSPACE: GitWorkspaceData = {
  status: null,
  diffStat: EMPTY_DIFF_STAT,
  sourceControl: null,
  pullRequest: null,
  forgeError: null,
};
const EMPTY_CHECKPOINTS: Checkpoint[] = [];

function slug(name: string): string {
  return name
    .toLowerCase()
    .replaceAll(/[^a-z0-9]+/gu, "-")
    .replaceAll(/^-|-$/gu, "");
}

/** A window-strip action with its accessible name and current shortcut. */
function IconAction({
  icon: Icon,
  label,
  hint,
  onClick,
  active,
}: {
  icon: typeof Keyboard;
  label: string;
  /** Live shortcut from the keymap, so a rebind shows up here too. */
  hint?: string;
  onClick: () => void;
  active?: boolean;
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant={active === true ? "secondary" : "ghost"}
            size="icon"
            aria-label={label}
            className={cn("size-7 shrink-0", active === true && "text-primary")}
            onClick={onClick}
          >
            <Icon className="size-4" />
          </Button>
        }
      />
      <TooltipContent>
        {label}
        {hint != null && hint !== "" && (
          <span className="ml-1.5 opacity-60">{hint}</span>
        )}
      </TooltipContent>
    </Tooltip>
  );
}

/** Preserve the selected wording while making its provenance unmistakable in the next prompt. */
function selectedExcerptMarkdown(text: string): string {
  return text
    .split(/\r?\n/)
    .map((line) => `> ${line}`)
    .join("\n");
}

const EMPTY_MANAGED_CATALOG: ManagedPluginCatalog = {
  graph_revision: 0,
  config_revision: 0,
  recovery: { kind: "normal" },
  plugins: [],
};
const EMPTY_SCENE_BY_SESSION = new Map<string, string>();

/** The single pane every workspace starts with, before any tiling split exists. */
const INITIAL_PANE_ID = "pane-1";

/**
 * The imperative command handles a pane's DocEditor exposes. Each pane owns one stable bundle;
 * focused App commands proxy to that bundle, while async requests retain the originating bundle.
 */
interface PaneEditorRefs {
  getBlocksRef: MutableRefObject<(() => DocBlock[]) | null>;
  insertTextRef: MutableRefObject<((text: string) => void) | null>;
  insertAnnotationRef: MutableRefObject<
    ((a: Annotation, context: string) => void) | null
  >;
  insertFileRef: MutableRefObject<((path: string) => void) | null>;
  insertSessionRef: MutableRefObject<
    | ((session: { id: string; title: string; throughSeq: number }) => void)
    | null
  >;
  focusRef: MutableRefObject<(() => void) | null>;
  clearRef: MutableRefObject<(() => void) | null>;
  insertMarkdownRef: MutableRefObject<
    ((markdown: string, mode: "replace" | "append") => Promise<void>) | null
  >;
  openSkillPickerRef: MutableRefObject<(() => void) | null>;
  insertSkillRef: MutableRefObject<((skill: SkillInfo) => void) | null>;
  insertBriefRef: MutableRefObject<
    ((scene: SceneInfo, values?: Record<string, string>) => void) | null
  >;
  insertIssueRef: MutableRefObject<
    ((issue: Issue, context: string, delegatedScene?: string) => void) | null
  >;
  insertCanvasRef: MutableRefObject<(() => Promise<void>) | null>;
  insertCanvasDraftRef: MutableRefObject<
    ((draft: CanvasDraft, options?: CanvasInsertOptions) => void) | null
  >;
  restoreCanvasDocumentRef: MutableRefObject<
    | ((
        doc: readonly DocBlock[],
        drafts: ReadonlyMap<string, CanvasDraft>,
        options?: CanvasInsertOptions
      ) => void)
    | null
  >;
  freezeCanvasesRef: MutableRefObject<
    ((doc: readonly DocBlock[]) => Promise<DocBlock[]>) | null
  >;
  canvasDeliveryErrorRef: MutableRefObject<
    | ((
        doc: readonly DocBlock[],
        message: string,
        kind: "provider_image" | "other"
      ) => void)
    | null
  >;
}

/** A fresh bundle of unbound (null) DocEditor command refs for one pane. */
function makePaneEditorRefs(): PaneEditorRefs {
  return {
    getBlocksRef: { current: null },
    insertTextRef: { current: null },
    insertAnnotationRef: { current: null },
    insertFileRef: { current: null },
    insertSessionRef: { current: null },
    focusRef: { current: null },
    clearRef: { current: null },
    insertMarkdownRef: { current: null },
    openSkillPickerRef: { current: null },
    insertSkillRef: { current: null },
    insertBriefRef: { current: null },
    insertIssueRef: { current: null },
    insertCanvasRef: { current: null },
    insertCanvasDraftRef: { current: null },
    restoreCanvasDocumentRef: { current: null },
    freezeCanvasesRef: { current: null },
    canvasDeliveryErrorRef: { current: null },
  };
}

/**
 * The per-pane runtime the tiling workspace owns. Today it only names the session (or draft, when
 * null) a pane points at; per-pane transcript/composer state migrates here as App is decomposed.
 */
interface PaneContent {
  sessionId: string | null;
}

interface PaneTranscriptState {
  loading: boolean;
  loadingEarlier: boolean;
  nextBefore: number | null;
}

const EMPTY_PANE_TRANSCRIPT_STATE: PaneTranscriptState = {
  loading: false,
  loadingEarlier: false,
  nextBefore: null,
};

export default function App() {
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [providersStatus, setProvidersStatus] = useState<
    "loading" | "ready" | "error"
  >("loading");
  const providerRegistryRequestRef = useRef(0);
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const { snapshot: pluginsSnapshot, refresh: refreshPluginsSnapshot } =
    usePluginSnapshot();
  const plugins = useMemo(
    () => pluginsSnapshot?.bundles ?? [],
    [pluginsSnapshot]
  );
  const managedUserCatalog =
    pluginsSnapshot?.catalogs.find((item) => item.scope.kind === "user")
      ?.catalog ?? null;
  const managedProjectCatalogs = useMemo(
    () =>
      Object.fromEntries(
        (pluginsSnapshot?.catalogs ?? []).flatMap(({ scope, catalog }) =>
          scope.kind === "project" ? [[scope.projectPath, catalog]] : []
        )
      ) as Record<string, ManagedPluginCatalog>,
    [pluginsSnapshot]
  );
  const [pluginManagerScope, setPluginManagerScope] =
    useState<PluginManagerScope>({ kind: "user" });
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [archivedSessions, setArchivedSessions] = useState<SessionInfo[]>([]);
  // Row 2 of every rail entry. Refreshed when a turn ends rather than per streamed chunk — the
  // preview is a glance, and requerying the transcript table on every token would be absurd.
  const [previews, setPreviews] = useState<Record<string, string>>({});
  const [provider, setProvider] = useState("");
  const [providerSwitchingSessions, setProviderSwitchingSessions] = useState<
    Set<string>
  >(() => new Set());
  const providerSwitchRequestsRef = useRef(new Set<string>());
  const [cwd, setCwd] = useState(".");
  const [mode, setMode] = useState<PermissionMode>("ask");
  const [sandbox, setSandboxState] = useState<Sandbox>("workspace_write");
  const [pendingPolicySessions, setPendingPolicySessions] = useState<
    Set<string>
  >(() => new Set());
  const [worktreeBase, setWorktreeBase] = useState<WorktreeBaselineKind | null>(
    null
  );
  const [worktreeOptions, setWorktreeOptions] = useState<
    WorktreeBaselineOption[]
  >([]);
  const [worktreeOptionsLoading, setWorktreeOptionsLoading] = useState(false);
  const worktreeOptionsRequestRef = useRef(0);
  const [memoryRead, setMemoryRead] = useState<MemoryAccess>("inherit");
  const [memoryWrite, setMemoryWrite] = useState<MemoryAccess>("inherit");
  // The tiling workspace: a recursive split tree plus the id -> content map. `activeSession` is the
  // focused pane's session, kept a derived value so the rail, dock, editor, palette and settings all
  // keep reading one "current session" unchanged while the column tiles underneath. The tree is now
  // real state, driven by the reducer (splitFocused/closePane/focusPane/setSplitRatio); a single
  // pane is just the tree's initial shape, so single-pane behaviour is unchanged.
  const [paneLayout, setPaneLayout] = useState<PaneLayout>(() =>
    singlePaneLayout(INITIAL_PANE_ID)
  );
  const [paneContents, setPaneContents] = useState<Record<string, PaneContent>>(
    () => ({ [INITIAL_PANE_ID]: { sessionId: null } })
  );
  const paneContentsRef = useRef(paneContents);
  const setPaneSession = (paneId: string, sessionId: string | null) => {
    const current = paneContentsRef.current[paneId];
    if (current?.sessionId === sessionId) return;
    const next = {
      ...paneContentsRef.current,
      [paneId]: { ...current, sessionId },
    };
    paneContentsRef.current = next;
    paneSessionsRef.current = new Set(
      Object.values(next)
        .map((content) => content.sessionId)
        .filter((id): id is string => id !== null)
    );
    setPaneContents(next);
  };
  const focusedPaneRef = useRef(paneLayout.focused);
  useEffect(() => {
    focusedPaneRef.current = paneLayout.focused;
  }, [paneLayout.focused]);
  // The event handler resolves splits/close against the live tree without re-subscribing.
  const paneLayoutRef = useRef(paneLayout);
  useEffect(() => {
    paneLayoutRef.current = paneLayout;
  }, [paneLayout]);
  // Fresh pane ids never collide with a reused slot; a monotonic counter is enough.
  const paneIdSeq = useRef(1);
  const nextPaneId = () => `pane-${(paneIdSeq.current += 1)}`;
  // PaneTiles owns pane geometry now; App only needs the count to gate per-pane close controls.
  const multiPane = listPanes(paneLayout.root).length > 1;
  const resizeSplitById = (splitId: string, ratio: number) => {
    setPaneLayout((layout) => setSplitRatio(layout, splitId, ratio));
  };
  const activeSession = paneContents[paneLayout.focused]?.sessionId ?? null;
  // A value-only setter shim: every existing caller assigns a concrete id (or null for a draft),
  // and all of them already mirror it into `activeSessionRef` first, so the ref stays authoritative
  // for the event handler while this points the focused pane at the same session.
  const setActiveSession = (id: string | null) => {
    setPaneSession(focusedPaneRef.current, id);
  };
  const [activeSessionReceipt, setActiveSessionReceipt] = useState<{
    session: string;
    shell: SessionCreationShell;
  } | null>(null);
  // Transcript state now lives per pane, keyed by paneId so a pane keeps its turns across the
  // draft -> session_created transition (its optimistic turn predates any session id). The focused
  // pane's slice is what existing focused-session code paths read as `turns`; async work and engine
  // events use the explicit pane/session writers below.
  const [turnsByPane, setTurnsByPane] = useState<Record<string, Turn[]>>(
    () => ({ [INITIAL_PANE_ID]: [] })
  );
  const turns = turnsByPane[paneLayout.focused] ?? EMPTY_TURNS;
  const setPaneTurns = (
    paneId: string,
    updater: Turn[] | ((prev: Turn[]) => Turn[])
  ) => {
    setTurnsByPane((prev) => {
      const current = prev[paneId] ?? EMPTY_TURNS;
      const next = typeof updater === "function" ? updater(current) : updater;
      if (next === current) return prev;
      return { ...prev, [paneId]: next };
    });
  };
  const setFocusedTurns = (updater: Turn[] | ((prev: Turn[]) => Turn[])) => {
    setPaneTurns(focusedPaneRef.current, updater);
  };
  // The event handler runs outside render and must resolve a live session -> pane mapping. Session
  // selection prevents duplicate bindings, so a session has at most one transcript destination.
  const paneForSession = (session: string | null): string => {
    if (session !== null) {
      const bound = paneBoundToSession(paneContentsRef.current, session);
      if (bound != null && bound !== "") return bound;
    }
    return focusedPaneRef.current;
  };
  const setTurnsForSession = (
    session: string | null,
    updater: Turn[] | ((prev: Turn[]) => Turn[])
  ) => {
    setPaneTurns(paneForSession(session), updater);
  };
  // The set of sessions currently bound to a pane, so the event handler can accumulate transcript
  // for background panes (not just the focused one). Kept in a ref for the out-of-render listener.
  const paneSessionsRef = useRef<ReadonlySet<string>>(new Set());
  useEffect(() => {
    paneSessionsRef.current = new Set(
      Object.values(paneContents)
        .map((content) => content.sessionId)
        .filter((id): id is string => id !== null)
    );
  }, [paneContents]);
  const [transcriptStateByPane, setTranscriptStateByPane] = useState<
    Record<string, PaneTranscriptState>
  >(() => ({ [INITIAL_PANE_ID]: EMPTY_PANE_TRANSCRIPT_STATE }));
  const transcriptStateByPaneRef = useRef(transcriptStateByPane);
  const updatePaneTranscriptState = (
    paneId: string,
    patch: Partial<PaneTranscriptState>
  ) => {
    const current =
      transcriptStateByPaneRef.current[paneId] ?? EMPTY_PANE_TRANSCRIPT_STATE;
    const nextState = { ...current, ...patch };
    const next = { ...transcriptStateByPaneRef.current, [paneId]: nextState };
    transcriptStateByPaneRef.current = next;
    setTranscriptStateByPane(next);
  };
  // Split whichever pane is passed (focusing it first, so the reducer's focused-relative split
  // lands on that pane), seeding an empty draft for the new leaf.
  const splitPaneById = (targetId: string, edge: PaneEdge) => {
    const id = nextPaneId();
    const nextContents = {
      ...paneContentsRef.current,
      [id]: { sessionId: null },
    };
    paneContentsRef.current = nextContents;
    setPaneContents(nextContents);
    setTurnsByPane((prev) => ({ ...prev, [id]: [] }));
    updatePaneTranscriptState(id, EMPTY_PANE_TRANSCRIPT_STATE);
    editorRevisionByPaneRef.current.set(id, 0);
    editorLocaleByPaneRef.current.set(id, localeRef.current);
    setEditorEmptyByPane((prev) => ({ ...prev, [id]: true }));
    setEditorKeyByPane((prev) => ({ ...prev, [id]: 0 }));
    focusedPaneRef.current = id;
    activeSessionRef.current = null;
    activeSessionProvenanceRef.current = null;
    setPaneLayout((layout) =>
      splitFocused(focusPane(layout, targetId), edge, id)
    );
  };
  // Closing a pane collapses its parent into the sibling and drops that pane's transcript; the
  // session itself keeps running in the background. The last pane is never closed.
  const closePaneById = (id: string) => {
    const panes = listPanes(paneLayoutRef.current.root);
    if (panes.length <= 1 || !panes.includes(id)) return;
    setPaneLayout((layout) => {
      const next = closePane(layout, id) ?? layout;
      focusedPaneRef.current = next.focused;
      activeSessionRef.current =
        paneContentsRef.current[next.focused]?.sessionId ?? null;
      activeSessionProvenanceRef.current = null;
      return next;
    });
    if (id in paneContentsRef.current) {
      const { [id]: _dropped, ...rest } = paneContentsRef.current;
      paneContentsRef.current = rest;
      paneSessionsRef.current = new Set(
        Object.values(rest)
          .map((content) => content.sessionId)
          .filter((sessionId): sessionId is string => sessionId !== null)
      );
      setPaneContents(rest);
    }
    setTurnsByPane((prev) => {
      if (!(id in prev)) return prev;
      const { [id]: _dropped, ...rest } = prev;
      return rest;
    });
    const { [id]: _droppedTranscript, ...remainingTranscript } =
      transcriptStateByPaneRef.current;
    transcriptStateByPaneRef.current = remainingTranscript;
    setTranscriptStateByPane(remainingTranscript);
    paneEditorRefsMap.current.delete(id);
    paneEmptyHandlersRef.current.delete(id);
    paneDocumentHandlersRef.current.delete(id);
    editorRevisionByPaneRef.current.delete(id);
    editorLocaleByPaneRef.current.delete(id);
    setEditorEmptyByPane((current) => {
      const { [id]: _dropped, ...rest } = current;
      return rest;
    });
    setEditorKeyByPane((current) => {
      const { [id]: _dropped, ...rest } = current;
      return rest;
    });
  };
  const [permissionQueue, setPermissionQueue] = useState<PermissionQueueItem[]>(
    []
  );
  const [runningSessions, setRunningSessions] = useState<Set<string>>(
    () => new Set()
  );
  const systemBadgeCount = needsMeCount(sessions);
  const [pendingSessionRunning, setPendingSessionRunning] = useState(false);
  const [pendingCreationPane, setPendingCreationPane] = useState<string | null>(
    null
  );
  const focusedTranscriptState =
    transcriptStateByPane[paneLayout.focused] ?? EMPTY_PANE_TRANSCRIPT_STATE;
  const sessionLoading = focusedTranscriptState.loading;
  const activePendingInputs = pendingInputsForSession(
    permissionQueue,
    activeSession
  );
  const permission = activePendingInputs[0] ?? null;
  const [skillDraft, setSkillDraft] = useState<{
    name: string;
    text: string;
  } | null>(null);
  /** R2 "save as template": the prompt text the TemplateDialog opens over. */
  const [templateDraft, setTemplateDraft] = useState<string | null>(null);
  const [gitWorkspace, setGitWorkspace] = useState<
    WorkspaceLoadState<GitWorkspaceData>
  >({
    cwd: ".",
    loading: true,
    value: EMPTY_GIT_WORKSPACE,
  });
  const [bindings, setBindings] = useState<KeymapEntry[]>([]);
  const [pendingAppshots, setPendingAppshots] = useState<
    Record<string, AppshotCapture[]>
  >({});
  // A blank tab, not a landing page: this browser's job is your localhost dev server, which you
  // type in.
  const [browserUrl, setBrowserUrl] = useState("about:blank");
  const [showSettings, setShowSettings] = useState(false);
  const [settingsInitialTab, setSettingsInitialTab] =
    useState<SettingsTab>("general");
  const [showAutomations, setShowAutomations] = useState(false);
  const [capturing, setCapturing] = useState<string | null>(null);
  const [showPluginManager, setShowPluginManager] = useState(false);
  const [showDocker, setShowDocker] = useState(false);
  const [showFeishu, setShowFeishu] = useState(false);
  const [issueConnector, setIssueConnector] = useState<IssueConnector | null>(
    null
  );
  const [issueDeliveries, setIssueDeliveries] = useState<IssueDelivery[]>([]);
  useEffect(() => {
    let disposed = false;
    let generation = 0;
    const refresh = async () => {
      const request = ++generation;
      try {
        const values = await issueDeliveryApi<IssueDelivery[]>("list");
        if (!disposed && request === generation) setIssueDeliveries(values);
      } catch {
        /* Existing tasks remain visible when the host reconnects. */
      }
    };
    void refresh();
    const timer = setInterval(() => {
      void refresh();
    }, 5000);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, []);
  const [feishuRailHost, setFeishuRailHost] = useState<HTMLDivElement | null>(
    null
  );
  const [feishuSettingsHost, setFeishuSettingsHost] =
    useState<HTMLDivElement | null>(null);
  const [pluginManagerInitialPluginId, setPluginManagerInitialPluginId] =
    useState<string | null>(null);
  const [market, setMarket] = useState<MarketItem[]>([]);
  const [localPluginMarketplace, setLocalPluginMarketplace] =
    useState<PluginMarketplace | null>(null);
  const [showSourceControl, setShowSourceControl] = useState(false);
  const [checkpointWorkspace, setCheckpointWorkspace] = useState<
    WorkspaceLoadState<Checkpoint[]>
  >({ cwd: ".", loading: true, value: EMPTY_CHECKPOINTS });
  const [showPalette, setShowPalette] = useState(false);
  const [showActionDialog, setShowActionDialog] = useState(false);
  const [showRemote, setShowRemote] = useState(false);
  const [showTaskHandoff, setShowTaskHandoff] = useState(false);
  const [showIssues, setShowIssues] = useState(false);
  const [preview, setPreview] = useState<CompiledPreview | null>(null);
  const [scripts, setScripts] = useState<ProjectScript[]>([]);
  const [showFiles, setShowFiles] = useState(false);
  const [showWorkspaceSearch, setShowWorkspaceSearch] = useState(false);
  const [quickQuotaReport, setQuickQuotaReport] =
    useState<ProviderQuotaReport | null>(null);
  const [quickQuotaLoading, setQuickQuotaLoading] = useState(true);
  const quickQuotaRequestRef = useRef(0);
  const [showMissionControl, setShowMissionControl] = useState(false);
  const [showTaskBoard, setShowTaskBoard] = useState(false);
  const [activeBoardTask, setActiveBoardTask] = useState<BoardTask | null>(
    null
  );
  const activeBoardTaskRef = useRef<BoardTask | null>(null);
  const [temporarySession, setTemporarySession] = useState(false);
  const temporarySessionRef = useRef(false);
  const [showPullRequests, setShowPullRequests] = useState(false);
  const [pullRequestTasks, setPullRequestTasks] = useState<BoardTask[]>([]);
  const [dockTab, setDockTab] = useState<DockTab | null>(null);
  const [quickChatOpen, setQuickChatOpen] = useState(false);
  const [sideChatSeed, setSideChatSeed] = useState<TransientChatSeed | null>(
    null
  );
  // ---- R10 dock follow (docs/archive/scenes-v1/frontend-implementation-plan.md Item 6) ----
  // The latch reducer's state lives in a ref because engine events arrive outside render; only
  // the badge hint is state, so the Dock can mark the surface the agent is working on.
  const dockTabRef = useRef<DockTab | null>(null);
  const dockFollowRef = useRef<FollowState>(initialFollowState);
  const [dockAutoHint, setDockAutoHint] = useState<ToolSurfaceHint | null>(
    null
  );
  useEffect(() => {
    void setSystemBadgeCount(systemBadgeCount).catch((error: unknown) => {
      console.warn("Could not update the system badge", error);
    });
  }, [systemBadgeCount]);
  // ---- scenes (Agent Scenes 1.0.0; docs/reference/scenes.md) ----
  const [scenes, setScenes] = useState<SceneInfo[]>([]);
  /** The active scene's canonical reference for the focused session (or the draft). */
  const [activeSceneName, setActiveSceneName] = useState<string | null>(null);
  const [autoScene, setAutoScene] = useState(false);
  const [scenePendingFields, setScenePendingFields] = useState<string[]>([]);
  const [showScenePicker, setShowScenePicker] = useState(false);
  const [showSceneStudio, setShowSceneStudio] = useState(false);
  const [sceneEditorRequest, setSceneEditorRequest] =
    useState<SceneEditorRequest | null>(null);
  const [sceneEscalation, setSceneEscalation] = useState<{
    reference: string;
    kind: "soft" | "restart" | "pipeline" | "pipeline_new";
    from: SessionMode;
    to: SessionMode;
    /** Set for kind "pipeline": confirming re-calls advance_pipeline with confirm=true. */
    pipeline?: { instanceId: string; toStage: string };
  } | null>(null);
  /** Scene completion/suggestion banner above the composer (R8); latest state key wins. */
  const [sceneBanner, setSceneBanner] = useState<SceneBannerState | null>(null);
  // ---- R9 pipeline instances (docs/reference/scenes.md §Pipelines) ----
  const [pipelines, setPipelines] = useState<PipelineInfo[]>([]);
  /** The active session's instance projection; the stage track renders only while this is set. */
  const [pipelineDetail, setPipelineDetail] =
    useState<PipelineInstanceDetail | null>(null);
  /** Scene to bind to the next created session (full-apply handshake). */
  const pendingSceneRef = useRef<string | null>(null);
  /** Per-session scene memory so switching sessions restores each one's scene. */
  const sceneBySessionRef = useRef(new Map<string, string>());
  const autoSceneBySessionRef = useRef(new Map<string, boolean>());
  const scenesRef = useRef<SceneInfo[]>([]);
  const activeSceneNameRef = useRef<string | null>(null);
  const autoSceneRef = useRef(false);
  /** Sessions whose scene reasoning_effort has been applied (once options arrived). */
  const sceneEffortAppliedRef = useRef(new Set<string>());
  /** Stage binding for the next created session (advance-in-new-session handshake). */
  const pendingPipelineBindRef = useRef<{
    instanceId: string;
    stageId: string;
  } | null>(null);
  /** Delegation row awaiting its session id (issue delegated → session created on first Run). */
  const pendingDelegationRef = useRef<number | null>(null);
  useEffect(() => {
    activeSceneNameRef.current = activeSceneName;
  }, [activeSceneName]);
  useEffect(() => {
    autoSceneRef.current = autoScene;
  }, [autoScene]);
  const [canvasFeature, setCanvasFeature] = useState<CanvasFeatureState>({
    feature: "CODETWO_CANVAS_INPUT_V1",
    enabled: false,
    status: "not production-enabled",
  });
  const canvasDraftsRef = useRef(new Map<string, CanvasDraft>());
  const canvasAssetsRef = useRef(
    new Map<string, Map<string, CanvasStaticAsset>>()
  );
  const canvasTombstonesRef = useRef(new Set<string>());
  const canvasPurgeRequestedRef = useRef(new Set<string>());
  const canvasFrozenRef = useRef(new Set<string>());
  const insertCanvasRef = useRef<(() => Promise<void>) | null>(null);
  const insertCanvasDraftRef = useRef<
    ((draft: CanvasDraft, options?: CanvasInsertOptions) => void) | null
  >(null);
  const restoreCanvasDocumentRef = useRef<
    | ((
        doc: readonly DocBlock[],
        drafts: ReadonlyMap<string, CanvasDraft>,
        options?: CanvasInsertOptions
      ) => void)
    | null
  >(null);
  const freezeCanvasesRef = useRef<
    ((doc: readonly DocBlock[]) => Promise<DocBlock[]>) | null
  >(null);
  const canvasDeliveryErrorRef = useRef<
    | ((
        doc: readonly DocBlock[],
        message: string,
        kind: "provider_image" | "other"
      ) => void)
    | null
  >(null);
  // Models are reported by the agent at session/new, so they arrive as an event rather than a call.
  const [models, setModels] = useState<ModelChoice[]>([]);
  const [currentModel, setCurrentModel] = useState<string | null>(null);
  // What the adapter itself picked at session/new — the picker badges it as "Default". Later
  // `models` events are switch echoes, so only the first one after a reset gets to set this.
  const [defaultModel, setDefaultModel] = useState<string | null>(null);
  // Session config options (model + reasoning effort) — the newer ACP surface, same lifecycle.
  const [configOptions, setConfigOptions] = useState<ConfigOptionInfo[]>([]);
  // Per-session snapshots of the same model/config surfaces, recorded from every session's events
  // (not just the focused one). A background pane's composer reads these so its model picker and
  // config reflect its own session; the focused pane keeps using the single values above.
  const [modelsBySession, setModelsBySession] = useState<
    Record<string, ModelChoice[]>
  >({});
  const [currentModelBySession, setCurrentModelBySession] = useState<
    Record<string, string | null>
  >({});
  const [defaultModelBySession, setDefaultModelBySession] = useState<
    Record<string, string | null>
  >({});
  const [configOptionsBySession, setConfigOptionsBySession] = useState<
    Record<string, ConfigOptionInfo[]>
  >({});
  const [interactionCapabilities, setInteractionCapabilities] = useState<
    Record<string, SessionInteractionCapabilities>
  >({});
  // Provider-reported context windows are session-level state, not transcript parts. Keeping the
  // map keyed by id prevents a late/background provider event from repainting the active session.
  const [contextWindows, setContextWindows] = useState<ContextWindowBySession>(
    {}
  );
  // Per-session cost/burn for the Composer statusline (R7). The core's `usage_by_session`
  // command lands in a later wave; until then the bridge feature-detects and this stays null,
  // which hides the cost segment entirely.
  const [sessionUsage, setSessionUsage] = useState<{
    input_tokens: number;
    output_tokens: number;
    costUsd: number | null;
    burnRate: number | null;
  } | null>(null);
  useEffect(() => {
    setSessionUsage(null);
    if (activeSession == null || activeSession === "") return;
    let cancelled = false;
    // Cumulative token counters per poll; burn rate is derived over a trailing window, so the
    // sample list lives and dies with the active session.
    const samples: { at: number; input: number; output: number }[] = [];
    const poll = async () => {
      const usage = await usageBySession(activeSession);
      if (cancelled) return;
      if (!usage) {
        setSessionUsage(null);
        return;
      }
      samples.push({
        at: Date.now(),
        input: usage.input_tokens,
        output: usage.output_tokens,
      });
      if (samples.length > 16) samples.shift();
      setSessionUsage({
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        costUsd: usage.cost_usd,
        burnRate: deriveBurnRate(samples),
      });
    };
    void poll();
    const timer = setInterval(() => void poll(), 30_000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [activeSession]);
  // Projects are the rail's organising idea: the conversation list and the git section below it
  // both describe whichever one is active.
  const [projects, setProjects] = useState<Project[]>([]);
  const [activeProject, setActiveProject] = useState<string | null>(null);
  const pendingAppshotsRef = useRef(pendingAppshots);
  pendingAppshotsRef.current = pendingAppshots;
  const composerDraftPostureRef = useRef<ComposerDraftPosture>({
    provider,
    model: currentModel,
    mode,
    sandbox,
    worktreeBase,
    memoryRead,
    memoryWrite,
    scene: activeSceneName,
    autoScene,
  });
  composerDraftPostureRef.current = {
    provider,
    model: currentModel,
    mode,
    sandbox,
    worktreeBase,
    memoryRead,
    memoryWrite,
    scene: activeSceneName,
    autoScene,
  };
  const activeAppshotKey =
    activeSession ?? `draft:${(activeProject ?? cwd) || "."}`;
  const activeAppshots = pendingAppshots[activeAppshotKey] ?? EMPTY_APPSHOTS;
  const removePendingAppshots = (ids: readonly string[]) => {
    if (ids.length === 0) return;
    const removed = new Set(ids);
    setPendingAppshots((current) => {
      const next = Object.fromEntries(
        Object.entries(current).flatMap(([key, captures]) => {
          const retained = captures.filter(
            (capture) => !removed.has(capture.id)
          );
          return retained.length > 0 ? [[key, retained]] : [];
        })
      );
      pendingAppshotsRef.current = next;
      return next;
    });
  };
  const [projectBootstrapComplete, setProjectBootstrapComplete] =
    useState(false);
  useEffect(() => {
    setCallProjectPath(
      activeProject != null && activeProject !== ""
        ? normalizePluginProjectPath(activeProject)
        : null
    );
  }, [activeProject]);
  const workspaceCwd = cwd || ".";
  const currentGitWorkspace = workspaceStateForCwd(
    gitWorkspace,
    workspaceCwd,
    EMPTY_GIT_WORKSPACE
  );
  const git = currentGitWorkspace.value.status;
  const { diffStat } = currentGitWorkspace.value;
  const activeSessionRecord =
    sessions.find((session) => session.id === activeSession) ??
    archivedSessions.find((session) => session.id === activeSession) ??
    null;
  const gitNextAction = resolveGitNextAction({
    status: git,
    loading: currentGitWorkspace.loading,
    sourceControl: currentGitWorkspace.value.sourceControl,
    pullRequest: currentGitWorkspace.value.pullRequest,
    forgeError: currentGitWorkspace.value.forgeError,
    taskWorktree:
      activeSessionRecord?.worktree_path != null &&
      activeSessionRecord?.worktree_path !== "" &&
      activeSessionRecord.worktree_discarded !== true,
    canCleanup:
      activeSessionRecord?.worktree_path != null &&
      activeSessionRecord?.worktree_path !== "" &&
      activeSessionRecord.worktree_discarded !== true,
  });
  const currentCheckpointWorkspace = workspaceStateForCwd(
    checkpointWorkspace,
    workspaceCwd,
    EMPTY_CHECKPOINTS
  );
  const checkpoints = currentCheckpointWorkspace.value;
  const running =
    activeSession != null && activeSession !== ""
      ? runningSessions.has(activeSession)
      : pendingSessionRunning && pendingCreationPane === paneLayout.focused;
  const interactivePreview = activeInteractivePreview(turns);
  const policyChangeDisabled = executionPolicyChangeDisabled(
    pendingSessionRunning,
    activeSession,
    pendingPolicySessions
  );
  const activeRunState =
    activeSession != null && activeSession !== ""
      ? sessionActivity(
          sessions.find((session) => session.id === activeSession) ??
            archivedSessions.find((session) => session.id === activeSession) ??
            {}
        ).state
      : null;
  const awaitingInput = activeRunState?.kind === "awaiting_input";
  const latestTurn = turns.at(-1)!;
  const petActivity = {
    loading: sessionLoading,
    running,
    awaitingInput,
    failed: activeRunState?.kind === "failed" || Boolean(latestTurn?.error),
    completed: Boolean(latestTurn?.endedAt),
  };
  const petAnimation = petAnimationForActivity(petActivity);
  const petConversationBubble = petConversationBubbleForActivity(
    petActivity,
    latestTurn?.text ?? ""
  );
  // The right panel's file editor: open tabs in open order, and which one is showing. Every tab
  // is directly editable — unsaved-ness lives in files/dirty.ts, which the close guard reads.
  const [openFiles, setOpenFiles] = useState<string[]>([]);
  const [activeFile, setActiveFile] = useState<string | null>(null);
  const [fileReveal, setFileReveal] = useState<FileRevealTarget | null>(null);
  const fileRevealRequestRef = useRef(0);
  const [dockTitlebarHost, setDockTitlebarHost] =
    useState<HTMLDivElement | null>(null);
  const [dockWidth, setDockWidth] = usePersistedNumber(
    "codetwo.dockWidth",
    440
  );
  const [railWidth, setRailWidth] = usePersistedNumber(
    "codetwo.railWidth",
    288
  );
  const [railCollapsedRaw, setRailCollapsedRaw] = usePersistedNumber(
    "codetwo.railCollapsed",
    0
  );
  const railCollapsed = railCollapsedRaw !== 0;
  const toggleRail = () => setRailCollapsedRaw(railCollapsed ? 0 : 1);
  const appliedRailWidth = Math.min(420, Math.max(220, railWidth));
  const [viewportWidth, setViewportWidth] = useState(() => window.innerWidth);
  const narrowLayout = shouldOverlayRailForWorkspace(
    viewportWidth - APP_NAV_WIDTH,
    appliedRailWidth
  );
  const [narrowRailOpen, setNarrowRailOpen] = useState(false);
  const wasNarrowLayoutRef = useRef(narrowLayout);
  useEffect(() => {
    const measure = () => {
      setViewportWidth(window.innerWidth);
    };
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, []);
  useLayoutEffect(() => {
    if (narrowLayout && !wasNarrowLayoutRef.current) setNarrowRailOpen(false);
    wasNarrowLayoutRef.current = narrowLayout;
  }, [narrowLayout]);
  const dockForcesRailOverlay =
    dockTab !== null &&
    shouldOverlayRailForDock(viewportWidth - APP_NAV_WIDTH, appliedRailWidth);
  const railOverlay = narrowLayout || dockForcesRailOverlay;
  const wasDockRailOverlayRef = useRef(dockForcesRailOverlay);
  useLayoutEffect(() => {
    if (dockForcesRailOverlay && !wasDockRailOverlayRef.current) {
      setNarrowRailOpen(false);
    }
    wasDockRailOverlayRef.current = dockForcesRailOverlay;
  }, [dockForcesRailOverlay]);
  const displayedRailCollapsed = railOverlay ? !narrowRailOpen : railCollapsed;
  const railInlineWidth =
    railOverlay || displayedRailCollapsed ? 0 : appliedRailWidth;
  const toggleDisplayedRail = () => {
    if (railOverlay) setNarrowRailOpen((open) => !open);
    else toggleRail();
  };
  const openTaskBoard = () => {
    setShowAutomations(false);
    setShowPluginManager(false);
    setShowPullRequests(false);
    setShowDocker(false);
    setShowFeishu(false);
    setShowTaskBoard(true);
    if (railOverlay) setNarrowRailOpen(false);
  };
  // New installs start with the bottom composer; the document toggle preserves the user's choice.
  const [docModeRaw, setDocModeRaw] = usePersistedNumber("codetwo.docMode", 0);
  const docMode = docModeRaw !== 0;
  const setDocMode = (v: boolean) => setDocModeRaw(v ? 1 : 0);
  const mainRef = useRef<HTMLElement | null>(null);
  const sessionWorkspaceRef = useRef<HTMLDivElement | null>(null);
  const heroScrollRef = useRef<HTMLDivElement | null>(null);
  useLayoutEffect(() => {
    if (!docMode && turns.length === 0 && !sessionLoading) {
      heroScrollRef.current?.scrollTo({ top: 0 });
    }
  }, [dockTab, docMode, sessionLoading, turns.length]);
  useEffect(() => {
    const workspace = sessionWorkspaceRef.current;
    if (!workspace) return;
    if (showTaskBoard) workspace.setAttribute("inert", "");
    else workspace.removeAttribute("inert");
  }, [showTaskBoard]);
  const toast = useToast();
  const t = useT();
  const { locale } = useLanguage();
  const localeRef = useRef(locale);
  localeRef.current = locale;
  const [editorEmptyByPane, setEditorEmptyByPane] = useState<
    Record<string, boolean>
  >(() => ({ [INITIAL_PANE_ID]: true }));
  const [editorKeyByPane, setEditorKeyByPane] = useState<
    Record<string, number>
  >(() => ({ [INITIAL_PANE_ID]: 0 }));
  const editorRevisionByPaneRef = useRef(
    new Map<string, number>([[INITIAL_PANE_ID, 0]])
  );
  const editorLocaleByPaneRef = useRef(
    new Map<string, string>([[INITIAL_PANE_ID, locale]])
  );
  const docEmpty = editorEmptyByPane[paneLayout.focused] ?? true;
  const desktopPlatform = currentDesktopPlatform();
  const editorLaunchersAvailable = desktopPlatform === "macos";
  const fileManagerLabel = editorLaunchersAvailable
    ? t("header.finder")
    : t("header.fileManager");

  const setTaskContext = (task: BoardTask | null, temporary: boolean) => {
    activeBoardTaskRef.current = task;
    temporarySessionRef.current = temporary;
    setActiveBoardTask(task);
    setTemporarySession(temporary);
  };

  const getBlocksRef = useRef<(() => DocBlock[]) | null>(null);
  const editorRevisionRef = useRef(0);
  const activeEditorDocRef = useRef<DocBlock[]>([]);
  const [initialComposerDrafts] = useState(() => loadComposerDrafts());
  const composerDraftsRef = useRef(initialComposerDrafts.drafts);
  const composerDraftLoadWarningRef = useRef(initialComposerDrafts.warning);
  const activeDraftScopeRef = useRef<ComposerDraftScope | null>(null);
  const composerDraftSaveTimerRef = useRef<number | null>(null);
  const composerDraftSaveWarningRef = useRef(false);
  const composerDraftRestoreGenerationRef = useRef(0);
  const composerDraftRestoringRef = useRef(false);
  const insertTextRef = useRef<((text: string) => void) | null>(null);
  const insertAnnotationRef = useRef<
    ((a: Annotation, context: string) => void) | null
  >(null);
  const insertFileRef = useRef<((path: string) => void) | null>(null);
  const insertSessionRef = useRef<
    | ((session: { id: string; title: string; throughSeq: number }) => void)
    | null
  >(null);
  const focusEditorRef = useRef<(() => void) | null>(null);
  const clearEditorRef = useRef<(() => void) | null>(null);
  const insertMarkdownRef = useRef<
    ((markdown: string, mode: "replace" | "append") => Promise<void>) | null
  >(null);
  const openSkillPickerRef = useRef<(() => void) | null>(null);
  const insertSkillRef = useRef<((skill: SkillInfo) => void) | null>(null);
  const insertBriefRef = useRef<
    ((scene: SceneInfo, values?: Record<string, string>) => void) | null
  >(null);
  // R12: issue references insert as dedicated provenance-carrying blocks, not plain text.
  const insertIssueRef = useRef<
    ((issue: Issue, context: string, delegatedScene?: string) => void) | null
  >(null);
  /**
   * An issue block staged for a delegated draft, consumed right after `createSession`'s
   * synchronous reset (mirror of `pendingSceneRef`; the editor itself survives New — only a
   * locale change remounts it — so the next tick is the "editor ready" point).
   */
  const pendingIssueInsertRef = useRef<{
    issue: Issue;
    context: string;
    delegatedScene: string;
  } | null>(null);
  const activeSessionRef = useRef<string | null>(null);
  // Every editor binds a stable pane-owned bundle. Shared App commands below are only live proxies
  // to the currently focused bundle, so an editor cleanup can never null another pane's handles.
  const paneEditorRefsMap = useRef(new Map<string, PaneEditorRefs>());
  const paneEditorRefsFor = (paneId: string): PaneEditorRefs => {
    const existing = paneEditorRefsMap.current.get(paneId);
    if (existing) return existing;
    const created = makePaneEditorRefs();
    paneEditorRefsMap.current.set(paneId, created);
    return created;
  };
  useEffect(() => {
    const focused = () => paneEditorRefsFor(focusedPaneRef.current);
    getBlocksRef.current = () => focused().getBlocksRef.current?.() ?? [];
    insertTextRef.current = (text) => focused().insertTextRef.current?.(text);
    insertAnnotationRef.current = (annotation, context) =>
      focused().insertAnnotationRef.current?.(annotation, context);
    insertFileRef.current = (path) => focused().insertFileRef.current?.(path);
    insertSessionRef.current = (session) =>
      focused().insertSessionRef.current?.(session);
    focusEditorRef.current = () => focused().focusRef.current?.();
    clearEditorRef.current = () => focused().clearRef.current?.();
    insertMarkdownRef.current = async (markdown, mode) =>
      await (focused().insertMarkdownRef.current?.(markdown, mode) ??
        Promise.resolve());
    openSkillPickerRef.current = () => focused().openSkillPickerRef.current?.();
    insertSkillRef.current = (skill) =>
      focused().insertSkillRef.current?.(skill);
    insertBriefRef.current = (scene, values) =>
      focused().insertBriefRef.current?.(scene, values);
    insertIssueRef.current = (issue, context, delegatedScene) =>
      focused().insertIssueRef.current?.(issue, context, delegatedScene);
    insertCanvasRef.current = async () =>
      await (focused().insertCanvasRef.current?.() ?? Promise.resolve());
    insertCanvasDraftRef.current = (draft, options) =>
      focused().insertCanvasDraftRef.current?.(draft, options);
    restoreCanvasDocumentRef.current = (doc, drafts, options) =>
      focused().restoreCanvasDocumentRef.current?.(doc, drafts, options);
    freezeCanvasesRef.current = async (doc) =>
      await (focused().freezeCanvasesRef.current?.(doc) ??
        Promise.resolve([...doc]));
    canvasDeliveryErrorRef.current = (doc, message, kind) =>
      focused().canvasDeliveryErrorRef.current?.(doc, message, kind);
  }, [paneEditorRefsFor]);
  // Event handlers and async continuations above the catalog projection need the same live policy
  // as rendered controls. Bootstrap closed: calling an unloadable command before the catalog and
  // active project realm agree is less safe than waiting one render for the policy snapshot.
  const componentEnabledRef = useRef<(id: BuiltinUiComponentId) => boolean>(
    () => false
  );
  // ---- R2 template-from-history (docs/archive/scenes-v1/frontend-implementation-plan.md Item 8) ----
  // Stable so the memoized TurnCards don't re-render on every App render.
  const openTemplateDraft = (promptText: string) => {
    setTemplateDraft(promptText);
  };
  const currentModelRef = useRef<string | null>(null);
  currentModelRef.current = currentModel;
  // Model changes invalidate the old provider context immediately. Keep the pending id until the
  // provider echoes its authoritative model, so an echo racing React state cannot restore stale
  // capacity; session/load responses without a model change leave replayed context intact.
  const knownModelsRef = useRef<Map<string, string>>(new Map());
  const pendingModelChangesRef = useRef<Set<string>>(new Set());
  // A correlated creation event is already durable even if the best-effort rail refresh fails.
  // Keep its source/worktree receipt beside the active id so New never treats isolated cwd as a
  // source checkout while React state or the backend list is temporarily unavailable.
  const activeSessionProvenanceRef = useRef<{
    session: string;
    shell: SessionCreationShell;
  } | null>(null);
  // Mirrors `activeProject` so `selectProject` can tell a real switch from a re-click without
  // remaking its callback (and the rail rows' props) on every project change.
  const activeProjectRef = useRef<string | null>(null);
  // Project list reads can overlap local preference writes. A read that started before or during
  // a write must never restore the older SQLite snapshot after the write completes.
  const projectLoadSeqRef = useRef(0);
  const projectMutationVersionRef = useRef(0);
  const memoryReadRef = useRef<MemoryAccess>("inherit");
  const memoryWriteRef = useRef<MemoryAccess>("inherit");
  const memoryReceiptsBySessionRef = useRef(new Map<string, MemoryReceipt[]>());
  const initializePluginSessionState = async (
    session: string,
    initial?: {
      memoryRead: MemoryAccess;
      memoryWrite: MemoryAccess;
      autoScene: boolean;
    }
  ) => {
    const updates: Promise<unknown>[] = [];
    if (componentEnabledRef.current("memory.settings")) {
      updates.push(
        setSessionMemoryPolicy(
          session,
          initial?.memoryRead ?? memoryReadRef.current,
          initial?.memoryWrite ?? memoryWriteRef.current
        )
      );
    }
    if (componentEnabledRef.current("scenes.surface")) {
      updates.push(
        setSessionAutoScene(session, initial?.autoScene ?? autoSceneRef.current)
      );
    }
    await Promise.all(updates);
  };
  const pendingCreationRef = useRef<PendingCreation | null>(null);
  // A picker change is provisional until the core publishes its durable correlated receipt.
  const pendingPolicyRequestsRef = useRef<Map<string, PendingPolicyRequest>>(
    new Map()
  );
  const pendingPolicyBySessionRef = useRef<Map<string, string>>(new Map());
  // Preserve a policy event that races a list refresh; the event is newer than that request's
  // snapshot and must remain the authoritative rail/session projection.
  const authoritativePoliciesRef = useRef<Map<string, ExecutionPolicy>>(
    new Map()
  );
  const policyVersionsRef = useRef<Map<string, number>>(new Map());
  // Prompt acknowledgements are broadcast to every client. Only the exact request initiated by
  // this window may clear its editor draft.
  const pendingPromptRequestsRef = useRef<Map<string, PendingPromptRequest>>(
    new Map()
  );
  const pendingDeferredPromptRequestsRef = useRef<
    Map<string, PendingPromptRequest>
  >(new Map());
  // TurnStarted consumes the pending entry, but a provider may reject images asynchronously after
  // the Composer has already cleared. Keep immutable refs until the terminal event so retry can
  // duplicate them without recovering a mutable draft.
  const acceptedCanvasRequestsRef = useRef<Map<string, PendingPromptRequest>>(
    new Map()
  );
  // An asynchronous Canvas image rejection keeps the immutable retry document until either the
  // original provider accepts a structure-only retry or an idle runtime switch succeeds.
  const canvasProviderRetrySessionRef = useRef<string | null>(null);
  // Only session/new calls initiated by this window may take over its active conversation. A
  // remote client can create sessions on the same engine without stealing desktop focus.
  const awaitingSessionRef = useRef<string | null>(null);
  const earlierLoadRunningByPaneRef = useRef(new Set<string>());
  const earlierLoadSeqByPaneRef = useRef(new Map<string, number>());
  // Each pane owns its load generation: focus changes neither redirect nor cancel another pane's
  // in-flight transcript request.
  const sessionLoadSeqByPaneRef = useRef(new Map<string, number>());
  const runningSessionsRef = useRef(runningSessions);
  const sessionActivitiesRef = useRef<Map<string, SessionActivity>>(new Map());
  // A null value means an authoritative TurnStarted arrived without a correlation id. Presence in
  // the map still matters: a later rejection for some local request must not stop that foreign turn.
  const runningPromptRequestsRef = useRef<Map<string, string | null>>(
    new Map()
  );
  const latestTurnRequestIdsRef = useRef<Map<string, string>>(new Map());
  const turnStartVersionsRef = useRef<Map<string, number>>(new Map());
  const gitRefreshSeq = useRef(0);
  const checkpointRefreshSeq = useRef(0);

  const finishPolicyRequest = (
    requestId: string
  ): PendingPolicyRequest | null => {
    const pending = pendingPolicyRequestsRef.current.get(requestId);
    if (!pending) return null;
    pendingPolicyRequestsRef.current.delete(requestId);
    if (pendingPolicyBySessionRef.current.get(pending.session) === requestId) {
      pendingPolicyBySessionRef.current.delete(pending.session);
      setPendingPolicySessions((current) => {
        if (!current.has(pending.session)) return current;
        const next = new Set(current);
        next.delete(pending.session);
        return next;
      });
    }
    return pending;
  };

  const applyAuthoritativeExecutionPolicy = (
    session: string,
    policy: ExecutionPolicy
  ) => {
    authoritativePoliciesRef.current.set(session, policy);
    policyVersionsRef.current.set(
      session,
      (policyVersionsRef.current.get(session) ?? 0) + 1
    );
    // If another client wins while our request is in flight, a later local rejection restores
    // this newest acknowledged value, not the value that preceded the remote change.
    for (const pending of pendingPolicyRequestsRef.current.values()) {
      if (pending.session === session) pending.authoritative = policy;
    }
    setSessions((current) =>
      withSessionExecutionPolicy(current, session, policy)
    );
    setArchivedSessions((current) =>
      withSessionExecutionPolicy(current, session, policy)
    );
    if (activeSessionRef.current === session) {
      setMode(policy.mode);
      setSandboxState(policy.sandbox);
    }
  };

  const restoreRejectedExecutionPolicy = (pending: PendingPolicyRequest) => {
    if (activeSessionRef.current !== pending.session) return;
    setMode(pending.authoritative.mode);
    setSandboxState(pending.authoritative.sandbox);
  };

  // Refs are updated before React schedules the render so transcript promises always see the same
  // running truth as the event handler that just mutated it.
  const updateRunningSession = (session: string, isRunning: boolean) => {
    const next = withRunningSession(
      runningSessionsRef.current,
      session,
      isRunning
    );
    runningSessionsRef.current = next;
    setRunningSessions(next);
  };

  const updateTranscriptCursor = (
    paneId: string,
    nextBefore: number | null
  ) => {
    updatePaneTranscriptState(paneId, { nextBefore });
  };

  const markSessionStarted = (session: string, requestId?: string | null) => {
    runningPromptRequestsRef.current.set(session, requestId ?? null);
    if (requestId != null && requestId !== "") {
      latestTurnRequestIdsRef.current.set(session, requestId);
    } else {
      latestTurnRequestIdsRef.current.delete(session);
    }
    turnStartVersionsRef.current.set(
      session,
      (turnStartVersionsRef.current.get(session) ?? 0) + 1
    );
    updateRunningSession(session, true);
  };

  const markSessionStopped = (
    session: string,
    requestId?: string | null
  ): boolean => {
    const current = runningPromptRequestsRef.current.get(session);
    if (
      requestId != null &&
      requestId !== "" &&
      runningPromptRequestsRef.current.has(session) &&
      requestId !== current
    ) {
      return false;
    }
    runningPromptRequestsRef.current.delete(session);
    updateRunningSession(session, false);
    return true;
  };

  // ---- R10 dock follow ----
  useEffect(() => {
    dockTabRef.current = dockTab;
  }, [dockTab]);

  /** The one dock-follow chokepoint: reduce, apply an emitted switch, mirror the badge hint. */
  const followDockEvent = (event: FollowEvent) => {
    const { state, setTab } = followReduce(dockFollowRef.current, event);
    dockFollowRef.current = state;
    if (setTab) setDockTab(setTab);
    if (event.kind === "tool") {
      setDockAutoHint(
        state.autoTab
          ? {
              surface: state.autoTab,
              file:
                state.autoTab === event.hint.surface
                  ? event.hint.file
                  : undefined,
            }
          : null
      );
    } else {
      setDockAutoHint(null);
    }
  };

  /** The event ladder's single follow call: classify the active session's tool call and feed the
   *  reducer. Auto-follow never opens a closed dock — the reducer only records the surface then. */
  const handleDockFollow = (ev: Extract<CoreEvent, { event: "tool_call" }>) => {
    const hint = classifyToolSurface({
      kind: ev.kind ?? null,
      title: ev.title,
      agentInput: ev.agent_input,
    });
    if (!hint) return;
    followDockEvent({
      kind: "tool",
      hint,
      now: Date.now(),
      dockOpen: dockTabRef.current !== null,
    });
  };

  /** Every user-driven dock change routes here so auto-follow latches off until the run ends. */
  const manualDockTab = (tab: DockTab | null) => {
    followDockEvent({ kind: "manual", tab });
    setDockTab(tab);
  };

  /** Restore immutable accepted Canvas refs after a terminal provider-image rejection.  The
   * original Composer heads may already have been cleared/purged, so each retry gets a new draft
   * id and an explicit error affordance; choosing structure-only remains a user action. */
  const restoreAcceptedCanvasForProviderError = async (
    session: string,
    request: PendingPromptRequest,
    message: string
  ) => {
    const refs = canvasRetryRefsForTerminal(
      "error",
      message,
      request.canvasRefs
    );
    if (refs.length === 0) return;
    const editorRefs = paneEditorRefsFor(request.paneId);
    if (!editorRefs.restoreCanvasDocumentRef.current) {
      throw new Error("Composer retry surface is unavailable");
    }
    const restored: CanvasDraft[] = [];
    try {
      for (const ref of refs)
        restored.push(await canvasDuplicate(ref.id, ref.revision));
    } catch (error) {
      // A failed duplicate must not leave an invisible mutable head behind. Immutable history
      // remains owned by core; only the newly created retry heads are tombstoned and purged.
      await Promise.all(
        restored.map(async (draft) => {
          try {
            await canvasTombstone(draft.id);
            await canvasPurge(draft.id);
          } catch {
            /* Best effort cleanup; the primary duplicate failure remains user-visible. */
          }
        })
      );
      throw error;
    }
    const replacements = new Map(
      refs.map((ref, index) => [
        ref.id,
        { id: restored[index].id, revision: restored[index].revision },
      ])
    );
    const retryDoc = canvasRetryDocument(request.submittedDoc, replacements);
    const restoredDrafts = new Map(restored.map((draft) => [draft.id, draft]));
    for (const draft of restored) {
      canvasDraftsRef.current.set(draft.id, draft);
      canvasAssetsRef.current.set(
        draft.id,
        new Map(draft.assets.map((asset) => [asset.id, asset]))
      );
    }
    editorRefs.restoreCanvasDocumentRef.current(retryDoc, restoredDrafts, {
      deliveryError: message,
      deliveryErrorKind: "provider_image",
    });
    canvasProviderRetrySessionRef.current = session;
    toast(
      "Canvas images are unsupported by this provider. Choose Send structure only in each restored Canvas, or switch provider and retry in this conversation.",
      "error"
    );
  };

  const saveComposerDraftCollection = (drafts: Map<string, ComposerDraft>) => {
    composerDraftsRef.current = drafts;
    if (saveComposerDrafts(drafts)) {
      composerDraftSaveWarningRef.current = false;
      return;
    }
    if (!composerDraftSaveWarningRef.current) {
      composerDraftSaveWarningRef.current = true;
      toast(t("toast.draftSaveFailed"), "error");
    }
  };

  const persistActiveComposerDraft = () => {
    const scope = activeDraftScopeRef.current;
    if (!scope) return;
    const captures =
      pendingAppshotsRef.current[composerDraftAttachmentKey(scope)] ?? [];
    saveComposerDraftCollection(
      updateComposerDraft(composerDraftsRef.current, {
        scope,
        doc: activeEditorDocRef.current,
        attachments: composerDraftAttachments(captures),
        posture: composerDraftPostureRef.current,
      })
    );
  };

  const flushActiveComposerDraft = () => {
    if (composerDraftSaveTimerRef.current !== null) {
      window.clearTimeout(composerDraftSaveTimerRef.current);
      composerDraftSaveTimerRef.current = null;
    }
    persistActiveComposerDraft();
  };

  const scheduleActiveComposerDraftSave = () => {
    if (composerDraftRestoringRef.current || !activeDraftScopeRef.current)
      return;
    if (composerDraftSaveTimerRef.current !== null) {
      window.clearTimeout(composerDraftSaveTimerRef.current);
    }
    composerDraftSaveTimerRef.current = window.setTimeout(() => {
      composerDraftSaveTimerRef.current = null;
      persistActiveComposerDraft();
    }, 250);
  };

  const applyComposerDraftPosture = (posture: ComposerDraftPosture) => {
    setProvider(posture.provider);
    setCurrentModel(posture.model);
    setDefaultModel(null);
    setConfigOptions([]);
    setMode(posture.mode);
    setSandboxState(posture.sandbox);
    setWorktreeBase(posture.worktreeBase);
    memoryReadRef.current = posture.memoryRead;
    memoryWriteRef.current = posture.memoryWrite;
    setMemoryRead(posture.memoryRead);
    setMemoryWrite(posture.memoryWrite);
    activeSceneNameRef.current = posture.scene;
    autoSceneRef.current = posture.autoScene;
    setActiveSceneName(posture.scene);
    setAutoScene(posture.autoScene);
  };

  const restoreComposerDraftScope = (
    scope: ComposerDraftScope,
    options: { restorePosture?: boolean } = {}
  ) => {
    const generation = (composerDraftRestoreGenerationRef.current += 1);
    composerDraftRestoringRef.current = true;
    activeDraftScopeRef.current = scope;
    clearEditorRef.current?.();
    activeEditorDocRef.current = [];
    const record = composerDraftsRef.current.get(composerDraftScopeKey(scope));
    const attachmentKey = composerDraftAttachmentKey(scope);
    if (
      record &&
      options.restorePosture !== false &&
      scope.kind === "project"
    ) {
      applyComposerDraftPosture(record.posture);
    }

    const updateAttachments = (captures: AppshotCapture[]) => {
      if (
        generation !== composerDraftRestoreGenerationRef.current ||
        composerDraftScopeKey(activeDraftScopeRef.current ?? scope) !==
          composerDraftScopeKey(scope)
      ) {
        return;
      }
      setPendingAppshots((current) => {
        const next = { ...current };
        if (captures.length > 0) next[attachmentKey] = captures;
        else delete next[attachmentKey];
        pendingAppshotsRef.current = next;
        return next;
      });
    };

    if (!record) {
      composerDraftRestoringRef.current = false;
      updateAttachments([]);
      return;
    }

    void Promise.all(
      record.attachments.map(async (attachment) => {
        try {
          return attachment.kind === "attachment"
            ? await getPromptImage(attachment.id)
            : await getAppshot(attachment.id);
        } catch {
          return null;
        }
      })
    ).then((loaded) => {
      const captures = loaded.filter(
        (capture): capture is AppshotCapture => capture !== null
      );
      updateAttachments(captures);
      if (captures.length !== record.attachments.length) {
        toast(t("toast.draftAttachmentMissing"), "error");
      }
    });

    const applyDocument = (drafts: ReadonlyMap<string, CanvasDraft>) => {
      if (
        generation !== composerDraftRestoreGenerationRef.current ||
        composerDraftScopeKey(activeDraftScopeRef.current ?? scope) !==
          composerDraftScopeKey(scope)
      ) {
        return;
      }
      const restore = restoreCanvasDocumentRef.current;
      if (!restore) {
        composerDraftRestoringRef.current = false;
        toast(t("toast.draftRestoreFailed"), "error");
        return;
      }
      composerDraftRestoringRef.current = true;
      try {
        restore(record.doc, drafts, { mode: "replace" });
        activeEditorDocRef.current = record.doc.map((block) =>
          block.type === "skill"
            ? { ...block, params: { ...block.params } }
            : { ...block }
        );
      } catch {
        toast(t("toast.draftRestoreFailed"), "error");
      } finally {
        composerDraftRestoringRef.current = false;
      }
    };

    const canvasIds = record.doc.flatMap((block) =>
      block.type === "canvas" ? [block.id] : []
    );
    if (canvasIds.length === 0) {
      applyDocument(new Map());
      return;
    }
    const editorRevision = editorRevisionRef.current;
    void Promise.all(
      canvasIds.map(async (id) => {
        const cached = canvasDraftsRef.current.get(id);
        if (cached) return cached;
        const loaded = await canvasGetDraft(id);
        if (!loaded) throw new Error(`Canvas draft ${id} is unavailable`);
        return loaded;
      })
    )
      .then((drafts) => {
        if (editorRevisionRef.current !== editorRevision) {
          if (generation === composerDraftRestoreGenerationRef.current) {
            composerDraftRestoringRef.current = false;
            scheduleActiveComposerDraftSave();
          }
          return;
        }
        for (const draft of drafts) {
          canvasDraftsRef.current.set(draft.id, draft);
          canvasAssetsRef.current.set(
            draft.id,
            new Map(draft.assets.map((asset) => [asset.id, asset]))
          );
        }
        applyDocument(new Map(drafts.map((draft) => [draft.id, draft])));
      })
      .catch(() => {
        if (generation === composerDraftRestoreGenerationRef.current) {
          composerDraftRestoringRef.current = false;
          toast(t("toast.draftRestoreFailed"), "error");
        }
      });
  };

  const promoteActiveComposerDraft = (
    session: string,
    projectPath: string | null
  ) => {
    flushActiveComposerDraft();
    const from = activeDraftScopeRef.current;
    const to: ComposerDraftScope = {
      kind: "session",
      sessionId: session,
      projectPath,
    };
    if (from) {
      const promotion = promoteComposerDraft(
        composerDraftsRef.current,
        from,
        to
      );
      if (promotion.outcome !== "conflict") {
        saveComposerDraftCollection(promotion.drafts);
      }
      const fromAttachmentKey = composerDraftAttachmentKey(from);
      const toAttachmentKey = composerDraftAttachmentKey(to);
      if (fromAttachmentKey !== toAttachmentKey) {
        setPendingAppshots((current) => {
          const source = current[fromAttachmentKey];
          if (source == null || current[toAttachmentKey]?.length)
            return current;
          const next = { ...current, [toAttachmentKey]: source };
          delete next[fromAttachmentKey];
          pendingAppshotsRef.current = next;
          return next;
        });
      }
    }
    activeDraftScopeRef.current = to;
  };

  useEffect(() => {
    const warning = composerDraftLoadWarningRef.current;
    composerDraftLoadWarningRef.current = null;
    if (warning === "corrupt") toast(t("toast.draftCorrupt"), "error");
    else if (warning === "unavailable")
      toast(t("toast.draftStorageUnavailable"), "error");
  }, [t, toast]);

  useEffect(() => {
    const flush = () => persistActiveComposerDraft();
    window.addEventListener("beforeunload", flush);
    return () => {
      window.removeEventListener("beforeunload", flush);
      if (composerDraftSaveTimerRef.current !== null) {
        window.clearTimeout(composerDraftSaveTimerRef.current);
        composerDraftSaveTimerRef.current = null;
      }
      persistActiveComposerDraft();
    };
  }, [persistActiveComposerDraft]);

  const invalidatePendingCreation = (paneId = focusedPaneRef.current) => {
    const pending = pendingCreationRef.current;
    if (pending && pending.paneId !== paneId) return;
    awaitingSessionRef.current = null;
    pendingCreationRef.current = null;
    setPendingSessionRunning(false);
    setPendingCreationPane(null);
    if (pending) {
      setPaneTurns(pending.paneId, (turns) =>
        withoutUnacceptedTurn(turns, pending.promptRequestId)
      );
    }
  };

  const handleEditorEmptyChange = (paneId: string, empty: boolean) => {
    setEditorEmptyByPane((current) =>
      current[paneId] === empty ? current : { ...current, [paneId]: empty }
    );
    if (
      empty &&
      editorLocaleByPaneRef.current.get(paneId) !== localeRef.current
    ) {
      editorLocaleByPaneRef.current.set(paneId, localeRef.current);
      setEditorKeyByPane((current) => ({
        ...current,
        [paneId]: (current[paneId] ?? 0) + 1,
      }));
    }
  };
  const paneEmptyHandlersRef = useRef(
    new Map<string, (empty: boolean) => void>()
  );
  const paneEmptyHandlerFor = (paneId: string) => {
    const existing = paneEmptyHandlersRef.current.get(paneId);
    if (existing) return existing;
    const created = (empty: boolean) => handleEditorEmptyChange(paneId, empty);
    paneEmptyHandlersRef.current.set(paneId, created);
    return created;
  };

  const handleEditorDocumentChange = (paneId: string, doc: DocBlock[]) => {
    editorRevisionByPaneRef.current.set(
      paneId,
      (editorRevisionByPaneRef.current.get(paneId) ?? 0) + 1
    );
    if (focusedPaneRef.current !== paneId) return;
    editorRevisionRef.current += 1;
    activeEditorDocRef.current = doc;
    scheduleActiveComposerDraftSave();
  };
  const paneDocumentHandlersRef = useRef(
    new Map<string, (doc: DocBlock[]) => void>()
  );
  const paneDocumentHandlerFor = (paneId: string) => {
    const existing = paneDocumentHandlersRef.current.get(paneId);
    if (existing) return existing;
    const created = (doc: DocBlock[]) =>
      handleEditorDocumentChange(paneId, doc);
    paneDocumentHandlersRef.current.set(paneId, created);
    return created;
  };

  useEffect(() => {
    scheduleActiveComposerDraftSave();
  }, [
    activeSceneName,
    autoScene,
    currentModel,
    memoryRead,
    memoryWrite,
    mode,
    pendingAppshots,
    provider,
    sandbox,
    scheduleActiveComposerDraftSave,
    worktreeBase,
  ]);

  useEffect(() => {
    if (!projectBootstrapComplete || activeDraftScopeRef.current) return;
    restoreComposerDraftScope({
      kind: "project",
      projectPath: (activeProjectRef.current ?? cwd) || ".",
    });
  }, [cwd, projectBootstrapComplete, restoreComposerDraftScope]);

  // BlockNote bakes its dictionary in at creation, so the placeholder only changes language on a
  // remount — and a remount discards whatever is in the document. Wait for the document to be empty
  // before taking the change: then a remount costs nothing, and a draft is never traded for a
  // placeholder. If the language changes mid-draft this simply defers until the draft is gone.
  useEffect(() => {
    const emptyPanes = Object.entries(editorEmptyByPane)
      .filter(([, empty]) => empty)
      .map(([paneId]) => paneId)
      .filter((paneId) => editorLocaleByPaneRef.current.get(paneId) !== locale);
    if (emptyPanes.length === 0) return;
    for (const paneId of emptyPanes)
      editorLocaleByPaneRef.current.set(paneId, locale);
    setEditorKeyByPane((current) => {
      const next = { ...current };
      for (const paneId of emptyPanes) next[paneId] = (next[paneId] ?? 0) + 1;
      return next;
    });
  }, [editorEmptyByPane, locale]);

  // Startup reads need stable identities even when React Compiler skips this component.
  const refreshSessions = useStableCallback(async () => {
    const policyVersionsAtStart = new Map(policyVersionsRef.current);
    try {
      const [active, archived, nextPreviews] = await Promise.all([
        listSessions(),
        listArchivedSessions(),
        sessionPreviews(),
      ]);
      const allIncoming = [...active, ...archived];
      const hasAuthoritativeActivity = allIncoming.some(
        (session) => session.activity !== undefined
      );
      const merge = (items: SessionInfo[]) =>
        items.map((session) => {
          let next = session;
          if (hasAuthoritativeActivity) {
            const activity = latestActivity(
              sessionActivitiesRef.current.get(session.id),
              session.activity
            );
            sessionActivitiesRef.current.set(session.id, activity);
            next = { ...next, activity };
          }

          const versionAtStart = policyVersionsAtStart.get(session.id) ?? 0;
          const currentVersion = policyVersionsRef.current.get(session.id) ?? 0;
          const eventPolicy = authoritativePoliciesRef.current.get(session.id);
          if (currentVersion !== versionAtStart && eventPolicy) {
            return {
              ...next,
              permission_mode: eventPolicy.mode,
              sandbox_policy: eventPolicy.sandbox,
            };
          }
          authoritativePoliciesRef.current.set(session.id, {
            mode: next.permission_mode,
            sandbox: next.sandbox_policy,
          });
          return next;
        });
      const nextActive = merge(active);
      const nextArchived = merge(archived);
      setSessions(nextActive);
      setArchivedSessions(nextArchived);
      setPreviews(nextPreviews);

      const all = [...nextActive, ...nextArchived];
      if (hasAuthoritativeActivity) {
        const busy = new Set(
          all
            .filter((session) => activityIsBusy(session.activity))
            .map((session) => session.id)
        );
        // A locally submitted draft remains optimistic until the core publishes its first
        // activity revision / TurnStarted; a concurrent stale list read cannot undo that shell.
        for (const session of pendingPromptRequestsRef.current.keys())
          busy.add(session);
        runningSessionsRef.current = busy;
        setRunningSessions(busy);
        setPermissionQueue(permissionsFromSessions(all));
      }
      return all;
    } catch (error) {
      // Preserve the last good rail. Returning null distinguishes failure from an authoritative
      // empty list, and the visible error prevents stale session state masquerading as success.
      console.error("Could not refresh sessions", error);
      toast(t("toast.sessionLoadFailed", { error: String(error) }), "error");
      return null;
    }
  });

  const refreshProjects = () => {
    const seq = (projectLoadSeqRef.current += 1);
    const mutationVersion = projectMutationVersionRef.current;
    listProjects()
      .then((items) => {
        if (
          seq === projectLoadSeqRef.current &&
          mutationVersion === projectMutationVersionRef.current
        ) {
          setProjects(items);
        }
      })
      .catch(() => {
        /* empty */
      });
  };

  const updateProjectWorktreeMode = async (
    path: string,
    nextMode: ProjectWorktreeMode | null
  ) => {
    const patchMode =
      (mode: ProjectWorktreeMode | null) => (items: Project[]) =>
        items.map((project) =>
          project.path === path
            ? { ...project, default_worktree_mode: mode }
            : project
        );

    projectMutationVersionRef.current += 1;
    try {
      await setProjectWorktreeMode(path, nextMode);
      projectMutationVersionRef.current += 1;
      // A project default seeds future drafts. It must not overwrite an explicit choice in the
      // current Composer while Settings is open or while this native write is in flight.
      setProjects(patchMode(nextMode));
    } catch (error) {
      projectMutationVersionRef.current += 1;
      toast(t("toast.projectDefaultFailed", { error: String(error) }), "error");
    }
  };

  const updateProjectName = async (path: string, name: string) => {
    projectMutationVersionRef.current += 1;
    try {
      await renameProject(path, name);
      projectMutationVersionRef.current += 1;
      setProjects((items) =>
        items.map((project) =>
          project.path === path ? { ...project, name: name.trim() } : project
        )
      );
    } catch (error) {
      projectMutationVersionRef.current += 1;
      throw error;
    }
  };

  const updateProjectIcon = async (path: string, source: string | null) => {
    projectMutationVersionRef.current += 1;
    try {
      const iconUpdatedAt = await setProjectIcon(path, source);
      projectMutationVersionRef.current += 1;
      setProjects((items) =>
        items.map((project) =>
          project.path === path
            ? {
                ...project,
                has_icon: source !== null,
                icon_updated_at: iconUpdatedAt,
              }
            : project
        )
      );
    } catch (error) {
      projectMutationVersionRef.current += 1;
      throw error;
    }
  };

  const updateProjectAgentDefaults = async (
    path: string,
    nextProvider: string | null,
    nextModel: string | null,
    nextReasoningEffort: string | null
  ) => {
    projectMutationVersionRef.current += 1;
    try {
      await setProjectAgentDefaults(
        path,
        nextProvider,
        nextModel,
        nextReasoningEffort
      );
      projectMutationVersionRef.current += 1;
      setProjects((items) =>
        items.map((project) =>
          project.path === path
            ? {
                ...project,
                default_provider: nextProvider,
                default_model:
                  nextProvider != null && nextProvider !== ""
                    ? nextModel
                    : null,
                default_reasoning_effort:
                  nextProvider != null && nextProvider !== ""
                    ? nextReasoningEffort
                    : null,
              }
            : project
        )
      );
      if (
        path === activeProjectRef.current &&
        activeSessionRef.current === null
      ) {
        if (nextProvider != null && nextProvider !== "")
          setProvider(nextProvider);
        setCurrentModel(
          nextProvider != null && nextProvider !== "" ? nextModel : null
        );
        setDefaultModel(null);
        setConfigOptions([]);
      }
    } catch (error) {
      projectMutationVersionRef.current += 1;
      throw error;
    }
  };

  /** Switch projects: the working directory, the conversation list and the git section all follow. */
  const selectProject = (path: string) => {
    // Re-clicking the current project is still an explicit navigation choice: a late creation
    // request must not take focus or submit the draft it captured before that choice.
    flushActiveComposerDraft();
    invalidatePendingCreation();
    setCallProjectPath(normalizePluginProjectPath(path));
    setActiveProject(path);
    setCwd(path);
    const project = projects.find((item) => item.path === path);
    if (project?.default_provider != null && project?.default_provider !== "")
      setProvider(project.default_provider);
    setCurrentModel(
      project?.default_provider != null && project?.default_provider !== ""
        ? (project.default_model ?? null)
        : null
    );
    setDefaultModel(null);
    setConfigOptions([]);
    setWorktreeBase(
      projectSwitchWorktreeBaseline(project?.default_worktree_mode ?? null)
    );
    // Selecting a project opens its source-checkout draft. Keeping an active worktree session
    // while `cwd` switches to the source would make file/Git/terminal surfaces show one checkout
    // while the agent keeps editing another.
    const paneId = focusedPaneRef.current;
    sessionLoadSeqByPaneRef.current.set(
      paneId,
      (sessionLoadSeqByPaneRef.current.get(paneId) ?? 0) + 1
    );
    updatePaneTranscriptState(paneId, {
      loading: false,
      loadingEarlier: false,
      nextBefore: null,
    });
    activeProjectRef.current = path;
    activeSessionRef.current = null;
    activeSessionProvenanceRef.current = null;
    setActiveSessionReceipt(null);
    setActiveSession(null);
    // A project opens a normal Task draft. Its Task record is created from the first prompt;
    // only the explicit Temporary session action opts out.
    setTaskContext(null, false);
    setFocusedTurns([]);
    setModels([]);
    memoryReadRef.current = "inherit";
    memoryWriteRef.current = "inherit";
    memoryReceiptsBySessionRef.current.clear();
    setMemoryRead("inherit");
    setMemoryWrite("inherit");
    restoreComposerDraftScope({ kind: "project", projectPath: path });
    void openProject(path).then(refreshProjects);
  };

  const removeProjectEntry = async (path: string) => {
    await removeProject(path);
    const removedScope: ComposerDraftScope = {
      kind: "project",
      projectPath: path,
    };
    const removedAttachmentKey = composerDraftAttachmentKey(removedScope);
    const drafts = new Map(composerDraftsRef.current);
    drafts.delete(composerDraftScopeKey(removedScope));
    saveComposerDraftCollection(drafts);
    const removedActiveDraft = path === activeProjectRef.current;
    if (removedActiveDraft) {
      // Project removal is the explicit discard boundary for its unsent project draft. Prevent the
      // next project's navigation flush from recreating the just-deleted record.
      activeDraftScopeRef.current = null;
      activeEditorDocRef.current = [];
    }
    setPendingAppshots((current) => {
      if (!(removedAttachmentKey in current)) return current;
      const next = { ...current };
      delete next[removedAttachmentKey];
      pendingAppshotsRef.current = next;
      return next;
    });
    try {
      const history = loadBrowserHistory(window.localStorage);
      saveBrowserHistory(
        window.localStorage,
        removeBrowserProject(history, path)
      );
    } catch {
      // Browser history is a convenience; a blocked local store must not block removal.
    }
    refreshProjects();
    if (!removedActiveDraft) return;
    const next = projects.find((project) => project.path !== path);
    if (next) selectProject(next.path);
    else {
      clearEditorRef.current?.();
      activeProjectRef.current = null;
      setCallProjectPath(null);
      setActiveProject(null);
    }
  };

  // Rail menu removal keeps the settings page's confirmation so a misclick can't drop a project.
  const requestProjectRemoval = async (path: string) => {
    const project = projects.find((candidate) => candidate.path === path);
    if (!project) return;
    if (
      !(await confirmNative(
        t("settings.removeProjectConfirm", { name: project.name })
      ))
    )
      return;
    try {
      await removeProjectEntry(path);
    } catch (error) {
      toast(t("settings.projectSaveFailed", { error: String(error) }), "error");
    }
  };

  const addProjectFolder = async () => {
    const picked = await pickDirectory();
    if (picked == null || picked === "") return; // cancelled — a normal outcome, not an error
    try {
      const resolved = await addProject(picked);
      refreshProjects();
      selectProject(resolved);
    } catch (error) {
      toast(t("toast.projectFailed", { error: String(error) }), "error");
    }
  };

  const activeSessionTitle = activeSessionRecord?.title ?? null;
  const activeTitle =
    activeBoardTask?.title ??
    activeSessionTitle ??
    t(temporarySession ? "rail.newTemporarySession" : "rail.newTask");

  // A pane's display title for the tiling previews: its session's title, or the draft placeholder.
  const titleForSession = (sessionId: string | null): string => {
    if (sessionId === null) return t("rail.newTask");
    return (
      (
        sessions.find((s) => s.id === sessionId) ??
        archivedSessions.find((s) => s.id === sessionId)
      )?.title ?? t("rail.newTask")
    );
  };

  // An archived chat is read-only: browsing it is fine, continuing it is not. The composer steps
  // aside for a notice until the session is restored.
  const activeArchived = archivedSessions.some((s) => s.id === activeSession);

  // Focus-only surfaces (full-page document, trajectory panel, transcript loading) belong to the
  // focused pane; renderPane reads these aliases so a background pane never takes them over.
  const focusedDocMode = docMode;
  // The focused pane keeps the richer header title (board task / temporary-session wording); a
  // background pane falls back to its session title, or the draft placeholder.
  const focusedActiveTitle = activeTitle;
  // The focused pane keeps the authoritative model/config/usage state (it also covers the draft,
  // project-default and dev-mode paths); a background pane reads its own session's snapshot maps.
  const focusedModels = models;
  const focusedCurrentModel = currentModel;
  const focusedDefaultModel = defaultModel;
  const focusedConfigOptions = configOptions;
  const focusedSessionUsage = sessionUsage;
  const activeWorktreeState = activeSessionWorktreeState(
    activeSession,
    activeSessionRecord ?? undefined,
    activeSessionReceipt
  );
  const activeWorktreeBaseline = activeWorktreeState.baseline;
  const activeWorktreeUnknown = activeWorktreeState.legacyUnknown;

  // The title bar's project badge — the workspace this session lives in, at a glance.
  const activeProjectRecord =
    projects.find((project) => project.path === activeProject) ?? null;
  const activeProjectName = activeProjectRecord?.name ?? null;
  // The focused pane keeps the authoritative project/cwd/git (which the app also drives elsewhere);
  // a background pane derives these from its own session record so its header breadcrumb and
  // composer checkout name the project the pane actually belongs to.
  const focusedCwd = cwd;
  const focusedActiveProject = activeProject;
  const focusedActiveProjectRecord = activeProjectRecord;
  const focusedActiveProjectName = activeProjectName;
  const focusedGit = git;
  const focusedGitNextAction = gitNextAction;

  const taskBoardSessions = [
    ...sessions.map((session) => ({
      id: session.id,
      title: session.title,
      archived: false,
      activity: session.activity,
      running: runningSessions.has(session.id),
      cwd: session.cwd,
      worktreePath: session.worktree_path,
      projectPath: session.project_path,
      worktreeDiscarded: session.worktree_discarded === true,
      createdAt: session.created_at,
      lastActiveAt: session.last_active_at,
    })),
    ...archivedSessions.map((session) => ({
      id: session.id,
      title: session.title,
      archived: true,
      activity: session.activity,
      running: false,
      cwd: session.cwd,
      worktreePath: session.worktree_path,
      projectPath: session.project_path,
      worktreeDiscarded: session.worktree_discarded === true,
      createdAt: session.created_at,
      lastActiveAt: session.last_active_at,
    })),
  ];

  const quickQuotaProvider = (() => {
    const focused = [...sessions, ...archivedSessions].find(
      (session) => session.id === activeSession
    );
    return quickQuotaProviderFor(
      providerLabel(provider),
      focused ? providerLabel(focused.provider) : null,
      sessions.map((session) => providerLabel(session.provider))
    );
  })();
  const quickQuotaProviderName =
    providers.find((candidate) => candidate.id === quickQuotaProvider)
      ?.display_name ?? quickQuotaProvider;
  const railQuickQuota = quickQuotaSummary(quickQuotaReport);

  const refreshQuickQuota = useCallback(() => {
    const request = (quickQuotaRequestRef.current += 1);
    setQuickQuotaLoading(true);
    void providerQuota(quickQuotaProvider)
      .then((report) => {
        if (request === quickQuotaRequestRef.current)
          setQuickQuotaReport(report);
      })
      .catch(() => {
        if (request === quickQuotaRequestRef.current) setQuickQuotaReport(null);
      })
      .finally(() => {
        if (request === quickQuotaRequestRef.current)
          setQuickQuotaLoading(false);
      });
  }, [quickQuotaProvider]);

  useEffect(() => {
    setQuickQuotaReport(null);
    refreshQuickQuota();
    const interval = window.setInterval(refreshQuickQuota, 5 * 60_000);
    window.addEventListener("focus", refreshQuickQuota);
    return () => {
      quickQuotaRequestRef.current += 1;
      window.clearInterval(interval);
      window.removeEventListener("focus", refreshQuickQuota);
    };
  }, [refreshQuickQuota]);

  // Sessions store a provider id; show the registry's display name where we have one.
  const displayProvider = (p: SessionInfo["provider"]) => {
    const id = providerLabel(p);
    return providers.find((x) => x.id === id)?.display_name ?? id;
  };

  // Track whether the user has hand-picked a provider; until then we auto-pick an available one.
  const providerPinned = useRef(false);

  const refreshProviders = useCallback(
    async (checkUpdates = false): Promise<ProviderInfo[]> => {
      const request = (providerRegistryRequestRef.current += 1);
      setProvidersStatus("loading");
      try {
        let list = checkUpdates
          ? await listProviders(true)
          : await loadProviderRegistry(listProviders);
        if (import.meta.env.DEV) {
          const query = new URLSearchParams(window.location.search);
          const fixtureProvider = query.get("mockProviderSettings");
          if (fixtureProvider != null && fixtureProvider !== "") {
            const fixtureModels = [
              ...new Set(
                (
                  query.get("mockModels") ??
                  "gpt-5.6-sol,gpt-5.6-terra,gpt-5.6-luna"
                )
                  .split(",")
                  .map((id) => id.trim())
                  .filter((id) => id.length > 0 && id.length <= 120)
              ),
            ]
              .slice(0, 20)
              .map((id) => ({ id, name: id, description: null }));
            list = list.map((candidate) =>
              candidate.id === fixtureProvider
                ? {
                    ...candidate,
                    available: true,
                    models: fixtureModels,
                    management: {
                      ...candidate.management,
                      installed: true,
                      version: "0.151.0",
                      latest_version: "0.151.0",
                      update_available: false,
                      launch_mode: "installed" as const,
                    },
                    configuration: {
                      ...candidate.configuration,
                      effective_command:
                        fixtureProvider === "codex"
                          ? "codex-acp"
                          : candidate.configuration.effective_command,
                      effective_args: ["--stdio"],
                    },
                  }
                : candidate
            );
          }
        }
        if (request !== providerRegistryRequestRef.current) return list;
        setProviders(list);
        setProvidersStatus("ready");
        // Default to a provider whose runtime is enabled and launchable. Shipping `grok` as the
        // default meant a machine without it failed on the first session with a raw spawn error.
        setProvider((current) => {
          const selected = list.find((candidate) => candidate.id === current);
          // Explicitly disabling the selected provider must leave new sessions with a runnable
          // choice even when that provider had previously been pinned in the Composer.
          if (selected?.enabled === false) {
            return list.find((candidate) => candidate.available)?.id ?? current;
          }
          if (providerPinned.current) return current;
          return selected?.available === true
            ? current
            : (list.find((candidate) => candidate.available)?.id ?? current);
        });
        return list;
      } catch (error) {
        if (request === providerRegistryRequestRef.current) {
          console.error("Could not load the provider registry", error);
          setProvidersStatus("error");
        }
        throw error;
      }
    },
    []
  );

  const refreshProviderUpdates = useCallback(
    async () => await refreshProviders(true),
    [refreshProviders]
  );

  useEffect(() => {
    void refreshProviders().catch(() => {
      /* empty */
    });
    return () => {
      providerRegistryRequestRef.current += 1;
    };
  }, [refreshProviders]);

  useEffect(() => {
    if (activeSession !== null || currentModel === null) return;
    const availableModels =
      providers.find((candidate) => candidate.id === provider)?.models ?? [];
    if (
      availableModels.length > 0 &&
      !availableModels.some((model) => model.id === currentModel)
    ) {
      setCurrentModel(null);
    }
  }, [activeSession, currentModel, provider, providers]);

  useEffect(() => {
    const activity = onBrowserAgentActivity(() => {
      setDockTab("browser");
      setTimeout(() => window.dispatchEvent(new Event("resize")), 0);
    });
    const downloads = onBrowserDownloadBlocked(() => {
      toast(
        "Download blocked. Approve it from the agent request or take control of the tab.",
        "error"
      );
    });
    return () => {
      void activity.then((unlisten) => unlisten());
      void downloads.then((unlisten) => unlisten());
    };
  }, [toast]);

  const coreEventCallbacks = useLatestRef({
    applyAuthoritativeExecutionPolicy,
    finishPolicyRequest,
    followDockEvent,
    handleDockFollow,
    initializePluginSessionState,
    invalidatePendingCreation,
    markSessionStarted,
    markSessionStopped,
    promoteActiveComposerDraft,
    refreshSessions,
    restoreAcceptedCanvasForProviderError,
    restoreRejectedExecutionPolicy,
    removePendingAppshots,
    toast,
    t,
    updateRunningSession,
  });
  useEffect(() => {
    void refreshSessions();

    let unlisten: (() => void) | null = null;
    let disposed = false;
    void (async () => {
      unlisten = await onEngineEvent((ev: CoreEvent) => {
        const {
          applyAuthoritativeExecutionPolicy,
          finishPolicyRequest,
          followDockEvent,
          handleDockFollow,
          initializePluginSessionState,
          invalidatePendingCreation,
          markSessionStarted,
          markSessionStopped,
          promoteActiveComposerDraft,
          refreshSessions,
          restoreAcceptedCanvasForProviderError,
          restoreRejectedExecutionPolicy,
          removePendingAppshots,
          toast,
          t,
          updateRunningSession,
        } = coreEventCallbacks.current;
        // Shared Task clients refetch the revisioned Task snapshot. The production TaskBoard is
        // still local in this P0 and must not render this control-plane event as transcript data.
        if (ev.event === "task_snapshot_changed") return;
        if (ev.event === "session_created") {
          const refreshed = refreshSessions();
          if (!matchesSessionCreation(ev, awaitingSessionRef.current)) return;
          const pending = pendingCreationRef.current;
          if (!pending) return;
          const originFocused = focusedPaneRef.current === pending.paneId;
          awaitingSessionRef.current = null;
          if (originFocused) {
            promoteActiveComposerDraft(
              ev.session,
              ev.project_path ?? activeProjectRef.current
            );
          }
          pendingCreationRef.current = null;
          sessionLoadSeqByPaneRef.current.set(
            pending.paneId,
            (sessionLoadSeqByPaneRef.current.get(pending.paneId) ?? 0) + 1
          );
          updatePaneTranscriptState(pending.paneId, { loading: false });
          setPaneSession(pending.paneId, ev.session);
          if (originFocused) activeSessionRef.current = ev.session;
          knownModelsRef.current.delete(ev.session);
          const receipt = sessionCreationReceipt(ev);
          const provenance = receipt
            ? { session: ev.session, shell: receipt }
            : null;
          if (originFocused) {
            activeSessionProvenanceRef.current = provenance;
            setActiveSessionReceipt(provenance);
          }
          {
            const stagedTaskId = pending.boardTaskId;
            const stagedTask =
              stagedTaskId != null && stagedTaskId !== ""
                ? (loadBoardSnapshot().tasks.find(
                    (task) => task.id === stagedTaskId
                  ) ?? null)
                : null;
            if (stagedTask) {
              const board = loadBoardSnapshot();
              const associated = associateTaskSession(
                board.tasks,
                stagedTask.id,
                ev.session
              );
              if (associated) {
                const saved = saveBoardSnapshot(associated);
                if (saved.ok) {
                  const task =
                    associated.find(
                      (candidate) => candidate.id === stagedTask.id
                    ) ?? null;
                  if (originFocused) setTaskContext(task, false);
                } else {
                  // The Session exists, but the durable association does not. Keep the UI honest
                  // instead of leaving the staged Task title visible as if persistence succeeded.
                  if (originFocused) setTaskContext(null, true);
                  toast(saved.warning, "error");
                }
              } else {
                // The Task was deleted from another board view while this draft was open. Keep the
                // new Session usable, but never claim that its missing Task association succeeded.
                if (originFocused) setTaskContext(null, true);
                toast("任务已不存在，本次会话已转为临时会话。", "error");
              }
            }
          }
          // Connect as soon as the durable shell exists so provider configuration can be
          // provider-authored before the next prompt instead of appearing only after it runs.
          void prepareSession(ev.session).catch(() => {
            /* empty */
          });
          autoSceneBySessionRef.current.set(ev.session, pending.autoScene);
          {
            // Delegation trail: the delegated draft just became a real session.
            const delegation = pendingDelegationRef.current;
            if (delegation !== null) {
              pendingDelegationRef.current = null;
              void setIssueDelegationSession(delegation, ev.session);
            }
          }
          {
            // Advance-in-new-session handshake: bind the created session to its pipeline stage.
            const bind = pendingPipelineBindRef.current;
            if (bind) {
              pendingPipelineBindRef.current = null;
              if (componentEnabledRef.current("scenes.surface")) {
                void bindPipelineSession(
                  bind.instanceId,
                  bind.stageId,
                  ev.session
                ).then(async () => {
                  if (!componentEnabledRef.current("scenes.surface")) return;
                  const detail = await getPipelineInstance(bind.instanceId);
                  if (
                    componentEnabledRef.current("scenes.surface") &&
                    activeSessionRef.current === ev.session
                  ) {
                    setPipelineDetail(detail);
                  }
                });
              }
            }
          }
          {
            // Full-apply handshake: bind the staged scene to the session the moment it exists.
            const pendingScene = pendingSceneRef.current;
            if (pendingScene != null && pendingScene !== "") {
              pendingSceneRef.current = null;
              if (componentEnabledRef.current("scenes.surface")) {
                sceneBySessionRef.current.set(ev.session, pendingScene);
                void setSessionScene(ev.session, pendingScene, false);
                const scene = scenesRef.current.find(
                  (s) => s.reference === pendingScene
                );
                if (
                  scene?.execution?.model != null &&
                  scene?.execution?.model !== ""
                ) {
                  void setSessionModel(ev.session, scene.execution.model);
                }
                if (originFocused) setActiveSceneName(pendingScene);
                // Provider-owned config ids do not exist until the session reports its options,
                // so scene effort stays pending until that handshake.
                const pending: string[] = [];
                if (
                  scene?.execution?.reasoning_effort != null &&
                  scene?.execution?.reasoning_effort !== ""
                )
                  pending.push("reasoning_effort");
                if (originFocused) setScenePendingFields(pending);
              } else {
                if (originFocused) {
                  setActiveSceneName(null);
                  setScenePendingFields([]);
                }
              }
            } else if (originFocused) {
              setActiveSceneName(
                sceneBySessionRef.current.get(ev.session) ?? null
              );
              setScenePendingFields([]);
            }
          }
          memoryReceiptsBySessionRef.current.delete(ev.session);
          // The creation event carries the cwd that was persisted before publication. File, Git,
          // terminal and hook surfaces switch with the active id even if a best-effort list/preview
          // refresh fails independently. Older event producers fall back to the list shell.
          if (originFocused && ev.cwd != null && ev.cwd !== "") setCwd(ev.cwd);
          void refreshed.then((items) => {
            if (!items || activeSessionRef.current !== ev.session) return;
            const created = items.find((session) => session.id === ev.session);
            if (!created) return;
            if (ev.cwd == null || ev.cwd === "") setCwd(created.cwd);
            if (activeSessionProvenanceRef.current?.session !== ev.session) {
              const provenance = { session: ev.session, shell: created };
              activeSessionProvenanceRef.current = provenance;
              setActiveSessionReceipt(provenance);
            }
          });
          {
            setPendingSessionRunning(false);
            setPendingCreationPane(null);
            updateRunningSession(ev.session, true);
            pendingPromptRequestsRef.current.set(ev.session, {
              requestId: pending.promptRequestId,
              paneId: pending.paneId,
              editorSnapshot: pending.editorSnapshot,
              editorRevision: pending.editorRevision,
              submittedDoc: pending.canvasRetryDoc,
              canvasIds: pending.doc.flatMap((block) =>
                block.type === "canvas" ? [block.id] : []
              ),
              canvasRefs: pending.doc.flatMap((block) =>
                block.type === "canvas"
                  ? [{ id: block.id, revision: block.frozen_revision }]
                  : []
              ),
              appshotIds: pending.appshotIds,
              clearEditor: pending.clearEditor,
            });
            void initializePluginSessionState(ev.session, pending)
              .then(
                async () =>
                  await submitPrompt(
                    ev.session,
                    pending.doc,
                    pending.promptRequestId
                  )
              )
              .then(() => {
                void refreshSessions();
              })
              .catch((error: unknown) => {
                if (
                  pendingPromptRequestsRef.current.get(ev.session)
                    ?.requestId === pending.promptRequestId
                ) {
                  pendingPromptRequestsRef.current.delete(ev.session);
                }
                markSessionStopped(ev.session, pending.promptRequestId);
                const message = String(error);
                setTurnsForSession(ev.session, (previous) =>
                  applyEvent(previous, {
                    event: "error",
                    session: ev.session,
                    message,
                    terminal: true,
                    request_id: pending.promptRequestId,
                  })
                );
                if (isCanvasProviderImageError(message)) {
                  paneEditorRefsFor(
                    pending.paneId
                  ).canvasDeliveryErrorRef.current?.(
                    pending.doc,
                    message,
                    "provider_image"
                  );
                  toast(
                    "Canvas images are unsupported by this provider. Choose Send structure only in each Canvas or switch provider, then retry.",
                    "error"
                  );
                } else {
                  toast(t("toast.turnFailed", { error: message }), "error");
                }
              });
          }
          return;
        }
        if (ev.event === "session_title_changed") {
          const rename = (items: SessionInfo[]) =>
            items.map((session) =>
              session.id === ev.session
                ? {
                    ...session,
                    title: ev.title,
                    title_origin: "automatic" as const,
                  }
                : session
            );
          setSessions(rename);
          setArchivedSessions(rename);
          return;
        }
        if (ev.event === "provider_changed") {
          const nextProvider = providerLabel(ev.provider);
          const nextModel = ev.model ?? null;
          const applyProvider = (items: SessionInfo[]) =>
            items.map((session) =>
              session.id === ev.session
                ? {
                    ...session,
                    provider: ev.provider,
                    model: nextModel,
                    acp_session_id: null,
                  }
                : session
            );
          setSessions(applyProvider);
          setArchivedSessions(applyProvider);
          setProviderSwitchingSessions((current) => {
            if (!current.has(ev.session)) return current;
            const next = new Set(current);
            next.delete(ev.session);
            return next;
          });
          knownModelsRef.current.delete(ev.session);
          pendingModelChangesRef.current.delete(ev.session);
          setModelsBySession((current) => {
            const { [ev.session]: _old, ...rest } = current;
            return rest;
          });
          setCurrentModelBySession((current) => ({
            ...current,
            [ev.session]: nextModel,
          }));
          setDefaultModelBySession((current) => {
            const { [ev.session]: _old, ...rest } = current;
            return rest;
          });
          setConfigOptionsBySession((current) => ({
            ...current,
            [ev.session]: [],
          }));
          setContextWindows((current) =>
            clearContextWindow(current, ev.session)
          );
          setInteractionCapabilities((current) => {
            const { [ev.session]: _old, ...rest } = current;
            return rest;
          });
          sceneEffortAppliedRef.current.delete(ev.session);
          if (ev.session === activeSessionRef.current) {
            setProvider(nextProvider);
            setModels([]);
            setCurrentModel(nextModel);
            setDefaultModel(null);
            setConfigOptions([]);
            const scene = scenesRef.current.find(
              (candidate) => candidate.reference === activeSceneNameRef.current
            );
            setScenePendingFields([
              ...(scene?.execution?.reasoning_effort != null &&
              scene?.execution?.reasoning_effort !== ""
                ? ["reasoning_effort"]
                : []),
            ]);
          }
          if (canvasProviderRetrySessionRef.current === ev.session) {
            canvasProviderRetrySessionRef.current = null;
          }
          void refreshSessions();
          return;
        }
        if (ev.event === "worktree_discarded") {
          // Mark the row immediately — the checkout is already gone — and let the list refresh
          // reconcile whatever else the discard changed (a deleted branch, a dropped stale row).
          const markDiscarded = (items: SessionInfo[]) =>
            items.map((session) =>
              session.id === ev.session
                ? { ...session, worktree_discarded: true }
                : session
            );
          setSessions(markDiscarded);
          setArchivedSessions(markDiscarded);
          void refreshSessions();
          toast(t("toast.worktreeDiscarded"), "success");
          return;
        }
        if (ev.event === "session_activity_changed") {
          const current = sessionActivitiesRef.current.get(ev.session);
          if (current && ev.activity.revision < current.revision) return;
          sessionActivitiesRef.current.set(ev.session, ev.activity);
          const applyActivity = (items: SessionInfo[]) =>
            items.map((session) =>
              session.id === ev.session
                ? { ...session, activity: ev.activity }
                : session
            );
          setSessions(applyActivity);
          setArchivedSessions(applyActivity);
          updateRunningSession(ev.session, activityIsBusy(ev.activity));
          // R10: only a finished run (idle | failed) releases the manual dock latch —
          // `awaiting_input` counts as busy, so it deliberately keeps it.
          if (
            ev.session === activeSessionRef.current &&
            !activityIsBusy(ev.activity)
          ) {
            followDockEvent({ kind: "run_ended" });
          }

          setPermissionQueue((previous) =>
            permissionQueueAfterActivity(previous, ev.session, ev.activity)
          );
          return;
        }
        if (ev.event === "context_window") {
          if (pendingModelChangesRef.current.has(ev.session)) return;
          // This is deliberately handled before transcript projection: a context update is
          // session state, never a persisted/rendered transcript part.
          setContextWindows((previous) => updateContextWindow(previous, ev));
          return;
        }
        if (ev.event === "session_capabilities") {
          setInteractionCapabilities((previous) => ({
            ...previous,
            [ev.session]: {
              steering: ev.steering,
              compact_context: ev.compact_context ?? false,
            },
          }));
          return;
        }
        if (
          ev.event === "exit_criteria_met" ||
          ev.event === "hook_suggestion" ||
          ev.event === "test_signal" ||
          ev.event === "artifact_produced" ||
          ev.event === "hook_turn_started" ||
          ev.event === "session_cost"
        ) {
          // Scene-layer facts (R8) are session state, never transcript parts. Only the two
          // banner-worthy ones render; the rest are consumed by the core's SceneRuntime.
          if (
            (ev.event === "exit_criteria_met" ||
              ev.event === "hook_suggestion") &&
            ev.session === activeSessionRef.current &&
            componentEnabledRef.current("scenes.surface")
          ) {
            const banner = sceneBannerFromEvent(ev);
            if (banner) setSceneBanner(banner);
          }
          return;
        }
        if (ev.event === "models") {
          const known = knownModelsRef.current.get(ev.session);
          const pending = pendingModelChangesRef.current.has(ev.session);
          if (
            ev.current &&
            (pending || (known !== undefined && ev.current !== known))
          ) {
            setContextWindows((previous) =>
              clearContextWindow(previous, ev.session)
            );
          }
          if (ev.current) {
            knownModelsRef.current.set(ev.session, ev.current);
            pendingModelChangesRef.current.delete(ev.session);
          }
          // Record every session's model surface so background panes stay accurate.
          if (ev.available.length > 0)
            setModelsBySession((prev) => ({
              ...prev,
              [ev.session]: ev.available,
            }));
          setCurrentModelBySession((prev) => ({
            ...prev,
            [ev.session]: ev.current || null,
          }));
          if (ev.current)
            setDefaultModelBySession((prev) =>
              prev[ev.session] == null
                ? { ...prev, [ev.session]: ev.current }
                : prev
            );
          if (ev.session !== activeSessionRef.current) return;
          // A switch echoes back the same list; only session/new carries a fresh one.
          if (ev.available.length > 0) setModels(ev.available);
          setCurrentModel(ev.current || null);
          setDefaultModel((prev) => prev ?? (ev.current || null));
          return;
        }
        if (ev.event === "config_options") {
          const model = ev.options.find(
            (o) => o.category === "model" || o.id === "model"
          );
          if (model?.current != null && model?.current !== "") {
            const known = knownModelsRef.current.get(ev.session);
            const pending = pendingModelChangesRef.current.has(ev.session);
            if (pending || (known !== undefined && model.current !== known)) {
              setContextWindows((previous) =>
                clearContextWindow(previous, ev.session)
              );
            }
            knownModelsRef.current.set(ev.session, model.current);
            pendingModelChangesRef.current.delete(ev.session);
          }
          // Record every session's config so background panes stay accurate.
          setConfigOptionsBySession((prev) => ({
            ...prev,
            [ev.session]: ev.options,
          }));
          if (model?.current != null && model?.current !== "") {
            const { current } = model;
            setCurrentModelBySession((prev) => ({
              ...prev,
              [ev.session]: current,
            }));
            setDefaultModelBySession((prev) =>
              prev[ev.session] == null
                ? { ...prev, [ev.session]: current }
                : prev
            );
          }
          if (ev.session !== activeSessionRef.current) return;
          // The agent's set is authoritative — it replaces any optimistic UI state wholesale.
          setConfigOptions(ev.options);
          if (model?.current != null && model?.current !== "") {
            setCurrentModel(model.current);
            // Same rule as `models`: the first report after a reset is the adapter's own pick.
            setDefaultModel((prev) => prev ?? model.current);
          }
          {
            // The scene's reasoning_effort had to pend until the provider named its effort
            // option (binding matrix); the first matching report applies it, once per session.
            const scene = scenesRef.current.find(
              (s) => s.reference === activeSceneNameRef.current
            );
            const wanted = scene?.execution?.reasoning_effort;
            if (
              wanted != null &&
              wanted !== "" &&
              !sceneEffortAppliedRef.current.has(ev.session)
            ) {
              const choice = sceneEffortChoice(ev.options, wanted);
              if (choice) {
                sceneEffortAppliedRef.current.add(ev.session);
                void setConfigOption(ev.session, choice.configId, choice.value)
                  .then(() => {
                    setScenePendingFields((prev) =>
                      prev.filter((field) => field !== "reasoning_effort")
                    );
                  })
                  .catch(() => {
                    sceneEffortAppliedRef.current.delete(ev.session);
                  });
              }
            }
          }
          return;
        }
        if (ev.event === "execution_policy_changed") {
          if (ev.request_id != null && ev.request_id !== "")
            finishPolicyRequest(ev.request_id);
          applyAuthoritativeExecutionPolicy(ev.session, ev.policy);
          return;
        }
        if (
          ev.event === "error" &&
          ev.request_id != null &&
          ev.request_id !== ""
        ) {
          const rejectedPolicy = finishPolicyRequest(ev.request_id);
          if (rejectedPolicy) {
            restoreRejectedExecutionPolicy(rejectedPolicy);
            toast(`Could not update execution policy: ${ev.message}`, "error");
            return;
          }
        }
        if (ev.event === "permission_request") {
          const request = {
            session: ev.session,
            requestId: ev.request_id,
            title: ev.title,
            options: ev.options,
            context: ev.context,
          };
          setPermissionQueue((previous) =>
            enqueuePermission(previous, request)
          );
          return;
        }
        if (ev.event === "elicitation_request") {
          // A question joins the same queue as permissions: one agent, one blocked turn, one
          // dialog at a time. `form` is what makes it render as a question instead of an approval.
          setPermissionQueue((previous) =>
            enqueuePermission(previous, {
              session: ev.session,
              requestId: ev.request_id,
              title: ev.form.message,
              options: [],
              form: ev.form,
            })
          );
          return;
        }
        if (ev.event === "prompt_queued" || ev.event === "steer_accepted") {
          const requestId = ev.request_id ?? undefined;
          const pendingRequest =
            requestId != null && requestId !== ""
              ? pendingDeferredPromptRequestsRef.current.get(requestId)
              : undefined;
          if (pendingRequest) {
            const editorRefs = paneEditorRefsFor(pendingRequest.paneId);
            const currentEditor = editorRefs.getBlocksRef.current?.();
            if (
              pendingRequest.clearEditor &&
              currentEditor &&
              matchesSubmittedEditorRevision(
                currentEditor,
                editorRevisionByPaneRef.current.get(pendingRequest.paneId) ?? 0,
                pendingRequest.editorSnapshot,
                pendingRequest.editorRevision
              )
            ) {
              for (const id of canvasIdsToPurgeAfterTurnStart(
                true,
                pendingRequest.canvasIds
              )) {
                canvasPurgeRequestedRef.current.add(id);
              }
              editorRefs.clearRef.current?.();
            }
            if (ev.event === "steer_accepted") {
              pendingDeferredPromptRequestsRef.current.delete(requestId!);
              if (pendingRequest.canvasRefs.length > 0) {
                acceptedCanvasRequestsRef.current.set(
                  `${ev.session}:${requestId}`,
                  pendingRequest
                );
              }
            }
          }
          if (ev.event === "steer_accepted") {
            markSessionStarted(ev.session, ev.request_id);
          }
          setTurnsForSession(ev.session, (previous) =>
            applyEvent(previous, ev)
          );
          return;
        }
        if (ev.event === "turn_started") {
          markSessionStarted(ev.session, ev.request_id);
          const pendingRequest =
            pendingPromptRequestsRef.current.get(ev.session) ??
            (ev.request_id != null && ev.request_id !== ""
              ? pendingDeferredPromptRequestsRef.current.get(ev.request_id)
              : undefined);
          if (pendingRequest && ev.request_id === pendingRequest.requestId) {
            pendingPromptRequestsRef.current.delete(ev.session);
            pendingDeferredPromptRequestsRef.current.delete(
              pendingRequest.requestId
            );
            if (canvasProviderRetrySessionRef.current === ev.session) {
              // An explicit structure-only retry was accepted in the original session; do not
              // force a later unrelated provider selection into a new session.
              canvasProviderRetrySessionRef.current = null;
            }
            if (pendingRequest.canvasRefs.length > 0) {
              acceptedCanvasRequestsRef.current.set(
                `${ev.session}:${pendingRequest.requestId}`,
                pendingRequest
              );
            }
            removePendingAppshots(pendingRequest.appshotIds);
            const editorRefs = paneEditorRefsFor(pendingRequest.paneId);
            const currentEditor = editorRefs.getBlocksRef.current?.();
            if (
              pendingRequest.clearEditor &&
              currentEditor &&
              matchesSubmittedEditorRevision(
                currentEditor,
                editorRevisionByPaneRef.current.get(pendingRequest.paneId) ?? 0,
                pendingRequest.editorSnapshot,
                pendingRequest.editorRevision
              )
            ) {
              // Core acceptance makes the frozen revision immutable history. Mark the mutable
              // Composer heads only when the submitted editor is still unchanged and we are
              // about to clear it; otherwise a later ordinary delete must remain undoable.
              for (const id of canvasIdsToPurgeAfterTurnStart(
                true,
                pendingRequest.canvasIds
              )) {
                canvasPurgeRequestedRef.current.add(id);
              }
              editorRefs.clearRef.current?.();
            }
          }
        }
        const awaitingCreationRequest = awaitingSessionRef.current;
        const awaitingCreationPane = pendingCreationRef.current?.paneId ?? null;
        const eventSession = ev.session;
        // Capture before a terminal event clears the authoritative running request. Deltas do not
        // carry request ids themselves, so this is how a transcript load that began after
        // TurnStarted still assigns them to the correct live turn.
        const activeTurnRequestId =
          eventSession != null && eventSession !== ""
            ? runningPromptRequestsRef.current.get(eventSession)
            : undefined;
        if (
          ev.event === "error" &&
          eventSession != null &&
          eventSession !== "" &&
          ev.request_id != null &&
          pendingPromptRequestsRef.current.get(eventSession)?.requestId ===
            ev.request_id
        ) {
          // No matching TurnStarted arrived, so the core did not durably accept this draft.
          pendingPromptRequestsRef.current.delete(eventSession);
        }
        if (
          ev.event === "error" &&
          ev.request_id != null &&
          ev.request_id !== ""
        ) {
          pendingDeferredPromptRequestsRef.current.delete(ev.request_id);
        }
        const ended = isTerminalSessionEvent(ev);
        if (ended) {
          if (eventSession != null && eventSession !== "") {
            const terminalRequestId =
              ev.event === "error"
                ? (ev.request_id ?? activeTurnRequestId)
                : activeTurnRequestId;
            if (terminalRequestId != null && terminalRequestId !== "") {
              const acceptedKey = canvasAcceptedRequestKey(
                eventSession,
                terminalRequestId
              );
              const acceptedCanvasRequest =
                acceptedCanvasRequestsRef.current.get(acceptedKey);
              if (acceptedCanvasRequest) {
                acceptedCanvasRequestsRef.current.delete(acceptedKey);
                if (
                  ev.event === "error" &&
                  isCanvasProviderImageError(ev.message)
                ) {
                  void restoreAcceptedCanvasForProviderError(
                    eventSession,
                    acceptedCanvasRequest,
                    ev.message
                  ).catch((error: unknown) => {
                    toast(
                      `Canvas retry could not be staged: ${String(error)}`,
                      "error"
                    );
                  });
                }
              }
            }
            if (markSessionStopped(eventSession, terminalRequestId)) {
              setPermissionQueue((previous) =>
                previous.filter((request) => request.session !== eventSession)
              );
            }
          } else if (
            ev.event === "error" &&
            ev.request_id != null &&
            ev.request_id === awaitingSessionRef.current
          ) {
            invalidatePendingCreation(
              awaitingCreationPane ?? focusedPaneRef.current
            );
          }
          void refreshSessions();
        }
        if (
          !shouldRenderSessionEvent(
            ev,
            activeSessionRef.current,
            awaitingCreationRequest,
            paneSessionsRef.current
          )
        )
          return;
        // The dock (terminal / file view) is a shared singleton that follows the focused pane, so
        // only the focused (or global) session may steer it — a background pane's tool call must
        // not hijack what the user is looking at.
        if (
          ev.event === "tool_call" &&
          (ev.session === null || ev.session === activeSessionRef.current)
        )
          handleDockFollow(ev);
        setPaneTurns(
          ev.session === null &&
            awaitingCreationPane != null &&
            awaitingCreationPane !== ""
            ? awaitingCreationPane
            : paneForSession(ev.session),
          (prev) => applyEvent(prev, ev, activeTurnRequestId ?? undefined)
        );
      });
      if (disposed) unlisten();
    })();

    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, [coreEventCallbacks, refreshSessions]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void onAutoSceneChanged((event) => {
      if (!componentEnabledRef.current("scenes.surface")) return;
      sceneBySessionRef.current.set(event.session, event.reference);
      autoSceneBySessionRef.current.set(event.session, true);
      if (event.session !== activeSessionRef.current) return;
      setActiveSceneName(event.reference);
      setAutoScene(true);
      setScenePendingFields(event.pending);
      memoryReadRef.current = event.memoryRead;
      memoryWriteRef.current = event.memoryWrite;
      setMemoryRead(event.memoryRead);
      setMemoryWrite(event.memoryWrite);
      toast(
        t("scene.autoSwitched", { scene: event.title, reason: event.reason }),
        "success"
      );
      void refreshSessions();
    }).then((dispose) => {
      unlisten = dispose;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, [refreshSessions, setTaskContext, t, toast]);

  // Rendered QA has no desktop event bridge in the Vite shell. This query-controlled fixture is
  // development-only and is replaced at build time, so production never gets a fake default.
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    const query = new URLSearchParams(window.location.search);
    if (query.get("mockContextWindow") !== "1") return;
    const usedTokens = Number(query.get("used") ?? "53000");
    const contextWindow = Number(query.get("size") ?? "200000");
    if (
      !Number.isSafeInteger(usedTokens) ||
      usedTokens < 0 ||
      !Number.isSafeInteger(contextWindow) ||
      contextWindow <= 0
    ) {
      return;
    }
    const session = query.get("session") ?? "dev-context-window";
    const requestedModels = [
      ...new Set(
        (query.get("mockModels") ?? "")
          .split(",")
          .map((id) => id.trim())
          .filter((id) => id.length > 0 && id.length <= 120)
      ),
    ].slice(0, 20);
    const mockModels =
      requestedModels.length > 0
        ? requestedModels.map((id) => ({ id, name: id, description: null }))
        : [{ id: "dev-model", name: "Context QA", description: null }];
    const mockModel = mockModels[0].id;
    const mockProvider = query.get("mockProviderSettings");
    if (mockProvider != null && mockProvider !== "") setProvider(mockProvider);
    activeSessionRef.current = session;
    knownModelsRef.current.set(session, mockModel);
    setActiveSession(session);
    setModels(mockModels);
    setCurrentModel(mockModel);
    setContextWindows((previous) => ({
      ...previous,
      [session]: { usedTokens, contextWindow, breakdown: null },
    }));
    if (query.get("mockCompactContext") === "1") {
      setInteractionCapabilities((previous) => ({
        ...previous,
        [session]: { steering: false, goal: null, compact_context: true },
      }));
    }
  }, []);

  const run = async (
    docOverride?: DocBlock[],
    newSessionTarget?: NewSessionRunTarget,
    paneId = focusedPaneRef.current,
    paneAppshots = activeAppshots
  ) => {
    const paneSession = paneContentsRef.current[paneId]?.sessionId ?? null;
    if (
      (transcriptStateByPaneRef.current[paneId] ?? EMPTY_PANE_TRANSCRIPT_STATE)
        .loading
    ) {
      toast(t("toast.sessionLoading"));
      return;
    }
    // The banner is the primary gate; this backstop catches the keyboard path (⌘⏎ and friends).
    if (
      paneSession !== null &&
      archivedSessions.some((session) => session.id === paneSession)
    ) {
      toast(t("archived.notice"));
      return;
    }
    const editorRefs = paneEditorRefsFor(paneId);
    const getBlocks = editorRefs.getBlocksRef.current;
    if (!getBlocks) return;
    const editorSnapshot = getBlocks();
    const editorRevision = editorRevisionByPaneRef.current.get(paneId) ?? 0;
    const promptAppshots = docOverride ? EMPTY_APPSHOTS : paneAppshots;
    const appshotIds = promptAppshots.map((capture) => capture.id);
    const clearEditor = docOverride === undefined;
    let doc: DocBlock[] = docOverride ?? [
      ...editorSnapshot,
      ...promptAppshots.map(privateImageBlock),
    ];
    // Running an empty document used to no-op in silence, which is indistinguishable from a broken
    // button. Say what's missing and put the caret where the fix goes.
    if (doc.length === 0) {
      toast(t("toast.emptyDoc"));
      editorRefs.focusRef.current?.();
      return;
    }
    if (
      (!newSessionTarget &&
        paneSession !== null &&
        runningSessionsRef.current.has(paneSession)) ||
      pendingCreationRef.current !== null
    ) {
      toast(t("toast.alreadyRunning"));
      return;
    }
    const targetSession = newSessionTarget ? null : paneSession;
    const creationWorktreeBase = newSessionTarget?.worktreeBase ?? worktreeBase;
    const stagedTask = activeBoardTaskRef.current;
    const temporary = temporarySessionRef.current;
    const projectPath = activeProjectRef.current;
    const submittedMemoryRead = memoryReadRef.current;
    const submittedMemoryWrite = memoryWriteRef.current;
    const worktreeBaseSha =
      newSessionTarget?.worktreeBaseSha ??
      (targetSession != null && targetSession !== ""
        ? null
        : sessionCreationBaselineSha(
            creationWorktreeBase,
            worktreeOptions,
            worktreeOptionsLoading
          ));
    if (worktreeBaseSha === undefined) {
      toast(
        t(
          worktreeOptionsLoading ? "worktree.resolving" : "worktree.unavailable"
        ),
        "error"
      );
      return;
    }
    // Freeze every live Canvas before creating the turn or submitting the prompt. The bridge owns
    // validation/export/CAS; any stale draft, missing pixels, or budget/provider failure aborts the
    // send with no optimistic turn left behind.
    try {
      if (editorRefs.freezeCanvasesRef.current) {
        doc = await editorRefs.freezeCanvasesRef.current(doc);
      }
    } catch (error) {
      toast(
        `Canvas could not be frozen: ${error instanceof Error ? error.message : String(error)}`,
        "error"
      );
      return;
    }
    const canvasRetryDoc = doc;
    const canvasIds = doc.flatMap((block) =>
      block.type === "canvas" ? [block.id] : []
    );
    const canvasRefs = doc.flatMap((block) =>
      block.type === "canvas"
        ? [{ id: block.id, revision: block.frozen_revision }]
        : []
    );
    // Ordinary blank drafts become Tasks on their first real run. Temporary sessions are the only
    // explicit opt-out, so no durable Session can silently fall through the Task board again.
    let boardTaskId = stagedTask?.id ?? null;
    if (
      (targetSession == null || targetSession === "") &&
      !stagedTask &&
      !temporary
    ) {
      const board = loadBoardSnapshot();
      if (board.warning != null && board.warning !== "")
        toast(board.warning, "error");
      const summary = summarizeDoc(doc).replaceAll(/\s+/g, " ").trim();
      const task = createBoardTask({
        title: summary.slice(0, 72) || "未命名任务",
        status: "todo",
        priority: "none",
        number: nextTaskNumber(board.tasks),
        order: board.tasks.filter((candidate) => candidate.status === "todo")
          .length,
      });
      const saved = saveBoardSnapshot([...board.tasks, task]);
      if (!saved.ok) {
        toast(saved.warning, "error");
        return;
      }
      boardTaskId = task.id;
      if (focusedPaneRef.current === paneId) setTaskContext(task, false);
    }
    const parallelTask =
      newSessionTarget?.parallelTask === true
        ? {
            taskId: activeBoardTaskRef.current!.id,
            goal: summarizeDoc(doc).replaceAll(/\s+/g, " ").trim(),
          }
        : null;
    const promptRequestId = globalThis.crypto.randomUUID();
    const creationRequestId =
      targetSession != null && targetSession !== "" ? null : promptRequestId;
    if (targetSession != null && targetSession !== "") {
      pendingPromptRequestsRef.current.set(targetSession, {
        requestId: promptRequestId,
        paneId,
        editorSnapshot,
        editorRevision,
        submittedDoc: canvasRetryDoc,
        canvasIds,
        canvasRefs,
        appshotIds,
        clearEditor,
      });
      updateRunningSession(targetSession, true);
    } else {
      awaitingSessionRef.current = creationRequestId;
      pendingCreationRef.current = {
        paneId,
        doc,
        canvasRetryDoc,
        promptRequestId,
        editorSnapshot,
        editorRevision,
        appshotIds,
        clearEditor,
        boardTaskId,
        autoScene: autoSceneRef.current,
        memoryRead: submittedMemoryRead,
        memoryWrite: submittedMemoryWrite,
        projectReasoningEffort:
          projects.find(
            (project) =>
              project.path === projectPath &&
              project.default_provider === provider
          )?.default_reasoning_effort ?? null,
      };
      setPendingSessionRunning(true);
      setPendingCreationPane(paneId);
    }
    setPaneTurns(paneId, (prev) => [
      ...prev,
      newTurn(
        summarizeDoc(doc),
        promptRequestId,
        promptImagesForTurn(promptAppshots)
      ),
    ]);
    try {
      if (targetSession != null && targetSession !== "") {
        if (componentEnabledRef.current("memory.settings")) {
          await setSessionMemoryPolicy(
            targetSession,
            submittedMemoryRead,
            submittedMemoryWrite
          );
        }
        await submitPrompt(targetSession, doc, promptRequestId);
        void refreshSessions();
      } else {
        await newSession(
          provider,
          newSessionTarget?.source ?? ((projectPath ?? cwd) || "."),
          creationWorktreeBase,
          creationRequestId!,
          worktreeBaseSha,
          { mode, sandbox },
          currentModel,
          false,
          pendingCreationRef.current?.projectReasoningEffort ?? null,
          parallelTask
        );
      }
    } catch (error) {
      const message = String(error);
      if (targetSession != null && targetSession !== "") {
        if (
          pendingPromptRequestsRef.current.get(targetSession)?.requestId ===
          promptRequestId
        ) {
          pendingPromptRequestsRef.current.delete(targetSession);
        }
        markSessionStopped(targetSession, promptRequestId);
        setPaneTurns(paneId, (previous) =>
          applyEvent(previous, {
            event: "error",
            session: targetSession,
            message,
            terminal: true,
            request_id: promptRequestId,
          })
        );
      } else {
        const stillOwned = awaitingSessionRef.current === creationRequestId;
        if (!stillOwned) return;
        invalidatePendingCreation(paneId);
        setPaneTurns(paneId, (previous) =>
          applyEvent(previous, {
            event: "error",
            session: null,
            message,
            terminal: true,
            request_id: promptRequestId,
          })
        );
      }
      if (isCanvasProviderImageError(message)) {
        editorRefs.canvasDeliveryErrorRef.current?.(
          doc,
          message,
          "provider_image"
        );
        toast(
          "Canvas images are unsupported by this provider. Choose Send structure only in each Canvas or switch provider, then retry.",
          "error"
        );
      } else {
        toast(t("toast.turnFailed", { error: message }), "error");
      }
    }
  };

  const sendDuringTurn = async (
    delivery: "queued" | "steer",
    docOverride?: DocBlock[],
    paneId = focusedPaneRef.current,
    paneAppshots = activeAppshots
  ) => {
    if (
      (transcriptStateByPaneRef.current[paneId] ?? EMPTY_PANE_TRANSCRIPT_STATE)
        .loading
    ) {
      toast(t("toast.sessionLoading"));
      return;
    }
    const session = paneContentsRef.current[paneId]?.sessionId ?? null;
    if (
      session == null ||
      session === "" ||
      !runningSessionsRef.current.has(session)
    ) {
      toast(t("toast.notRunning"), "error");
      return;
    }
    const editorRefs = paneEditorRefsFor(paneId);
    const getBlocks = editorRefs.getBlocksRef.current;
    if (!getBlocks) return;
    const editorSnapshot = getBlocks();
    const editorRevision = editorRevisionByPaneRef.current.get(paneId) ?? 0;
    const promptAppshots = docOverride ? EMPTY_APPSHOTS : paneAppshots;
    const appshotIds = promptAppshots.map((capture) => capture.id);
    let doc: DocBlock[] = docOverride ?? [
      ...editorSnapshot,
      ...promptAppshots.map(privateImageBlock),
    ];
    if (doc.length === 0) {
      toast(t("toast.emptyDoc"));
      editorRefs.focusRef.current?.();
      return;
    }
    if (delivery === "steer" && !interactionCapabilities[session]?.steering) {
      toast(t("toast.steerUnsupported"), "error");
      return;
    }
    try {
      if (editorRefs.freezeCanvasesRef.current) {
        doc = await editorRefs.freezeCanvasesRef.current(doc);
      }
    } catch (error) {
      toast(
        `Canvas could not be frozen: ${error instanceof Error ? error.message : String(error)}`,
        "error"
      );
      return;
    }
    const requestId = globalThis.crypto.randomUUID();
    const pending: PendingPromptRequest = {
      requestId,
      paneId,
      editorSnapshot,
      editorRevision,
      submittedDoc: doc,
      canvasIds: doc.flatMap((block) =>
        block.type === "canvas" ? [block.id] : []
      ),
      canvasRefs: doc.flatMap((block) =>
        block.type === "canvas"
          ? [{ id: block.id, revision: block.frozen_revision }]
          : []
      ),
      appshotIds,
      clearEditor: docOverride === undefined,
    };
    pendingDeferredPromptRequestsRef.current.set(requestId, pending);
    const optimistic = newTurn(
      summarizeDoc(doc),
      requestId,
      promptImagesForTurn(promptAppshots)
    );
    optimistic.delivery = delivery;
    optimistic.queuePosition = delivery === "queued" ? 1 : undefined;
    setPaneTurns(paneId, (previous) => [...previous, optimistic]);
    try {
      if (delivery === "queued") {
        await queuePrompt(session, doc, requestId);
      } else {
        await steerPrompt(session, doc, requestId);
      }
    } catch (error) {
      pendingDeferredPromptRequestsRef.current.delete(requestId);
      setPaneTurns(paneId, (previous) =>
        applyEvent(previous, {
          event: "error",
          session,
          message: String(error),
          terminal: false,
          request_id: requestId,
        })
      );
      toast(t("toast.turnFailed", { error: String(error) }), "error");
    }
  };

  // Composer model/config changes, parameterized by the session they act on so any pane's composer
  // can drive its own session. Both are optimistic: the engine echoes an authoritative `models` /
  // `config_options` event (or an `error`) that reconciles this state.
  const changeSessionModel = (session: string | null, id: string) => {
    if (session == null || session === "") {
      setCurrentModel(id);
      return;
    }
    if (runningSessionsRef.current.has(session)) {
      toast(t("toast.modelBusy"), "error");
      return;
    }
    if (id !== currentModelRef.current) {
      pendingModelChangesRef.current.add(session);
      knownModelsRef.current.set(session, id);
      setContextWindows((previous) => clearContextWindow(previous, session));
    }
    setCurrentModel(id);
    void setModel(session, id).catch((error: unknown) => {
      pendingModelChangesRef.current.delete(session);
      toast(t("toast.modelFailed", { error: String(error) }), "error");
    });
  };

  const changeSessionConfigOption = (
    session: string | null,
    configId: string,
    value: string
  ) => {
    if (session == null || session === "") return;
    const option = configOptions.find((item) => item.id === configId);
    if (
      (option?.category === "model" || configId === "model") &&
      runningSessionsRef.current.has(session)
    ) {
      toast(t("toast.modelBusy"), "error");
      return;
    }
    if (
      (option?.category === "model" || configId === "model") &&
      value !== currentModelRef.current
    ) {
      pendingModelChangesRef.current.add(session);
      knownModelsRef.current.set(session, value);
      setContextWindows((previous) => clearContextWindow(previous, session));
    }
    setConfigOptions((prev) =>
      prev.map((o) => (o.id === configId ? { ...o, current: value } : o))
    );
    void setConfigOption(session, configId, value).catch((error: unknown) => {
      if (option?.category === "model" || configId === "model") {
        pendingModelChangesRef.current.delete(session);
      }
      toast(t("toast.modelFailed", { error: String(error) }), "error");
    });
  };

  const createSession = (): string | null => {
    const currentSessionId = activeSessionRef.current;
    const storedSession =
      sessions.find((session) => session.id === currentSessionId) ??
      archivedSessions.find((session) => session.id === currentSessionId);
    const currentSession = sessionShellWithReceipt(
      currentSessionId,
      storedSession,
      activeSessionProvenanceRef.current
    );
    const source = sessionCreationSource(
      activeProjectRef.current,
      cwd,
      currentSession,
      currentSessionId !== null
    );
    if (source === null) {
      toast(t("toast.worktreeSourceUnknown"), "error");
      return null;
    }

    flushActiveComposerDraft();
    invalidatePendingCreation();
    setShowTaskBoard(false);
    setShowPullRequests(false);
    setShowDocker(false);
    setShowFeishu(false);
    const paneId = focusedPaneRef.current;
    sessionLoadSeqByPaneRef.current.set(
      paneId,
      (sessionLoadSeqByPaneRef.current.get(paneId) ?? 0) + 1
    );
    updatePaneTranscriptState(paneId, {
      loading: false,
      loadingEarlier: false,
      nextBefore: null,
    });
    setPendingSessionRunning(false);
    setPendingCreationPane(null);
    activeSessionRef.current = null;
    activeSessionProvenanceRef.current = null;
    setActiveSessionReceipt(null);
    setActiveSession(null);
    setFocusedTurns([]);
    setModels([]);
    setDefaultModel(null);
    setConfigOptions([]);
    memoryReadRef.current = "inherit";
    memoryWriteRef.current = "inherit";
    if (currentSessionId != null && currentSessionId !== "")
      memoryReceiptsBySessionRef.current.delete(currentSessionId);
    setMemoryRead("inherit");
    setMemoryWrite("inherit");
    // A plain new session starts sceneless; the restart-in-scene flow stages its scene first
    // and keeps it through this reset.
    if (pendingSceneRef.current === null) {
      setActiveSceneName(null);
      setScenePendingFields([]);
    }
    setCwd(source);
    // Pressing New while already on the unsent blank draft is a focus/reset no-op for workspace
    // selection: keep the explicit Composer choice. Leaving a durable session starts a new draft
    // and seeds that draft from the project preference or the prior session's baseline kind.
    if (currentSessionId !== null) {
      const project = projects.find(
        (item) => item.path === activeProjectRef.current
      );
      const projectMode = project?.default_worktree_mode ?? null;
      const baseline = nextSessionWorktreeBaseline(
        projectMode,
        sessionCreationBaseline(currentSession)
      );
      if (baseline !== undefined) setWorktreeBase(baseline);
      if (project?.default_provider != null && project?.default_provider !== "")
        setProvider(project.default_provider);
      setCurrentModel(
        project?.default_provider != null && project?.default_provider !== ""
          ? (project.default_model ?? null)
          : null
      );
      restoreComposerDraftScope({
        kind: "project",
        projectPath: (activeProjectRef.current ?? source) || ".",
      });
    }
    // Caret into the document; whichever mode you're in stays yours.
    setTimeout(() => focusEditorRef.current?.(), 0);
    // A New action opens a configurable blank draft. The first Run creates the durable session,
    // after the now-visible baseline picker has had a chance to tell the truth and be changed.
    return source;
  };

  const createTaskDraft = () => {
    setTaskContext(null, false);
    return createSession();
  };

  const forkTurnIntoTask = (turn: Turn) => {
    const sourceSession = activeSessionRef.current;
    const throughSeq = turn.transcriptStartSeq;
    const insertSession = insertSessionRef.current;
    if (
      sourceSession == null ||
      sourceSession === "" ||
      throughSeq === undefined ||
      !insertSession
    ) {
      toast(t("turn.forkFailed"), "error");
      return;
    }
    const sourceTitle = activeSessionTitle ?? sourceSession.slice(0, 8);
    const taskDraft = createTaskDraft();
    if (taskDraft == null) return;
    insertSession({
      id: sourceSession,
      title: sourceTitle,
      throughSeq,
    });
    toast(t("turn.forked"), "success");
  };

  const startBoardTask = (task: BoardTask) => {
    setTaskContext(task, false);
    createSession();
  };

  const startParallelTask = () => {
    const session = activeSessionRef.current;
    if (
      session == null ||
      session === "" ||
      !runningSessionsRef.current.has(session)
    ) {
      toast(t("toast.notRunning"), "error");
      return;
    }
    const worktreeBaseSha = sessionCreationBaselineSha(
      "current",
      worktreeOptions,
      worktreeOptionsLoading
    );
    if (worktreeBaseSha == null || worktreeBaseSha === "") {
      toast(t("toast.multitaskWorktreeUnavailable"), "error");
      return;
    }
    const copiedDoc = [
      ...(getBlocksRef.current?.() ?? []),
      ...activeAppshots.map(privateImageBlock),
    ];
    if (copiedDoc.length === 0) {
      toast(t("toast.emptyDoc"), "error");
      focusEditorRef.current?.();
      return;
    }
    const source = createSession();
    if (source == null || source === "") return;

    // A parallel send is a new user-owned Task, not an opaque provider subagent. The current
    // Session keeps running in the background while the new Session gets its own checkout.
    setTaskContext(null, false);
    setWorktreeBase("current");
    setProvider(provider);
    setCurrentModel(currentModel);
    void run(copiedDoc, {
      source,
      worktreeBase: "current",
      worktreeBaseSha,
      parallelTask: true,
    });
  };

  const appshotCapturedRef = useRef<(capture: AppshotCapture) => void>(() => {
    /* empty */
  });
  const appshotFailedRef = useRef<(failure: { message: string }) => void>(
    () => {
      /* empty */
    }
  );
  appshotCapturedRef.current = (capture) => {
    const needsNewDraft = capture.destination === "new" || activeArchived;
    if (needsNewDraft) createTaskDraft();
    const session = needsNewDraft ? null : activeSessionRef.current;
    const key = session ?? `draft:${(activeProjectRef.current ?? cwd) || "."}`;
    setPendingAppshots((current) => {
      const existing = current[key] ?? [];
      const next = {
        ...current,
        [key]: [
          ...existing.filter((candidate) => candidate.id !== capture.id),
          capture,
        ],
      };
      pendingAppshotsRef.current = next;
      return next;
    });
    setShowSettings(false);
    setCapturing(null);
    toast(t("toast.appshotReady", { app: capture.app_name }), "success");
    setTimeout(() => focusEditorRef.current?.(), 0);
  };
  appshotFailedRef.current = ({ message }) => {
    toast(t("toast.appshotFailed", { error: message }), "error");
  };

  const attachPromptImages = async (files: readonly File[]) => {
    const images = files.filter((file) => file.type.startsWith("image/"));
    if (images.length === 0) return;
    const session = activeSessionRef.current;
    const key = session ?? `draft:${(activeProjectRef.current ?? cwd) || "."}`;
    const results = await Promise.allSettled(
      images.map(
        async (file) =>
          await importPromptImage(
            new Uint8Array(await file.arrayBuffer()),
            file.type || null,
            file.name || "Image.png"
          )
      )
    );
    const captures = results.flatMap((result) =>
      result.status === "fulfilled" ? [result.value] : []
    );
    if (captures.length > 0) {
      setPendingAppshots((current) => {
        const next = {
          ...current,
          [key]: [...(current[key] ?? []), ...captures],
        };
        pendingAppshotsRef.current = next;
        return next;
      });
      setTimeout(() => focusEditorRef.current?.(), 0);
    }
    const failure = results.find(
      (result): result is PromiseRejectedResult => result.status === "rejected"
    );
    if (failure) {
      toast(
        t("toast.imageAttachFailed", { error: String(failure.reason) }),
        "error"
      );
    }
  };

  useEffect(() => {
    let removeCaptured: (() => void) | null = null;
    let removeFailed: (() => void) | null = null;
    let active = true;
    void onAppshotCaptured((capture) =>
      appshotCapturedRef.current(capture)
    ).then((dispose) => {
      if (active) removeCaptured = dispose;
      else dispose();
    });
    void onAppshotFailed((failure) => appshotFailedRef.current(failure)).then(
      (dispose) => {
        if (active) removeFailed = dispose;
        else dispose();
      }
    );
    return () => {
      active = false;
      removeCaptured?.();
      removeFailed?.();
    };
  }, []);

  const addSelectedText = (text: string) => {
    void insertMarkdownRef.current?.(
      selectedExcerptMarkdown(text),
      docEmpty ? "replace" : "append"
    );
    setDocMode(true);
    setTimeout(() => focusEditorRef.current?.(), 0);
  };

  const explainSelectedText = (text: string) => {
    const markdown = `${t("selection.moreDetailsPrompt")}\n\n${selectedExcerptMarkdown(text)}`;
    void insertMarkdownRef.current?.(markdown, docEmpty ? "replace" : "append");
    setDocMode(true);
    setTimeout(() => focusEditorRef.current?.(), 0);
  };

  const askSelectedTextInSideChat = (text: string) => {
    const markdown = `${t("selection.askInSideChatPrompt")}\n\n${selectedExcerptMarkdown(text)}`;
    setSideChatSeed({ id: globalThis.crypto.randomUUID(), text: markdown });
    manualDockTab("side-chat");
  };

  const readPullRequestTasks = (): BoardTask[] | null => {
    const board = loadBoardSnapshot();
    if (board.warning != null && board.warning !== "") {
      setPullRequestTasks([]);
      toast(board.warning, "error");
      return null;
    }
    setPullRequestTasks(board.tasks);
    return board.tasks;
  };

  const linkPullRequestToTask = (
    detail: GitHubPullRequestDetail,
    renderedTarget: PullRequestTaskLinkTarget | null
  ) => {
    const current = readPullRequestTasks();
    if (!current) return;
    const reference = githubPullRequestReference(detail);
    if (taskForPullRequest(current, reference)) {
      toast(t("pullRequests.taskLinkChanged"), "error");
      return;
    }

    let tasks = current;
    let target = renderedTarget
      ? (current.find((task) => task.id === renderedTarget.id) ?? null)
      : null;
    if (
      renderedTarget &&
      (!target ||
        target.pullRequestLinkRevision !== renderedTarget.revision ||
        target.pullRequest !== null)
    ) {
      toast(t("pullRequests.taskLinkChanged"), "error");
      return;
    }

    const created = target === null;
    if (!target) {
      target = createBoardTask({
        title: detail.title,
        description: detail.body.trim().slice(0, 600),
        status: "in_progress",
        priority: "none",
        labels: ["GitHub", "PR"],
        number: nextTaskNumber(current),
        order: current.filter((task) => task.status === "in_progress").length,
      });
      tasks = [...tasks, target];
    }
    const targetId = target.id;
    const associated = associateTaskPullRequest(tasks, targetId, reference);
    if (!associated) {
      toast(t("pullRequests.taskLinkChanged"), "error");
      return;
    }
    const linked = associated.find((task) => task.id === targetId);
    if (!linked) {
      toast(t("pullRequests.taskLinkChanged"), "error");
      return;
    }
    const saved = saveBoardSnapshot(associated);
    if (!saved.ok) {
      toast(saved.warning, "error");
      return;
    }
    setPullRequestTasks(associated);
    if (activeBoardTaskRef.current?.id === linked.id)
      setTaskContext(linked, false);
    toast(
      t(created ? "pullRequests.taskCreated" : "pullRequests.taskLinked", {
        title: linked.title,
      }),
      "success"
    );
  };

  const unlinkPullRequestFromTask = (
    detail: GitHubPullRequestDetail,
    renderedLink: PullRequestTaskLinkTarget
  ) => {
    const current = readPullRequestTasks();
    if (!current) return;
    const reference = githubPullRequestReference(detail);
    const task =
      current.find((candidate) => candidate.id === renderedLink.id) ?? null;
    const unlinked = unlinkTaskPullRequest(
      current,
      renderedLink.id,
      githubPullRequestIdentity(reference),
      renderedLink.revision
    );
    if (!task || !unlinked) {
      toast(t("pullRequests.taskLinkChanged"), "error");
      return;
    }
    const saved = saveBoardSnapshot(unlinked);
    if (!saved.ok) {
      toast(saved.warning, "error");
      return;
    }
    setPullRequestTasks(unlinked);
    if (activeBoardTaskRef.current?.id === task.id) {
      setTaskContext(
        unlinked.find((candidate) => candidate.id === task.id) ?? null,
        false
      );
    }
    toast(t("pullRequests.taskUnlinked", { title: task.title }), "success");
  };

  const chatAboutPullRequest = (detail: GitHubPullRequestDetail) => {
    const prompt = [
      t("pullRequests.chatPrompt"),
      `**${detail.repository.nameWithOwner} #${detail.number} — ${detail.title}**`,
      detail.url,
      detail.body,
    ]
      .filter(Boolean)
      .join("\n\n");
    const board = loadBoardSnapshot();
    if (board.warning == null || board.warning === "") {
      const linkedTask = taskForPullRequest(
        board.tasks,
        githubPullRequestReference(detail)
      );
      if (linkedTask) setTaskContext(linkedTask, false);
    }
    setShowPullRequests(false);
    createSession();
    clearEditorRef.current?.();
    setDocMode(true);
    setTimeout(() => {
      void insertMarkdownRef.current?.(prompt, "replace");
      focusEditorRef.current?.();
    }, 0);
  };

  const answer = async (optionId: string | null) => {
    if (permission == null) return;
    const accepted = await answerPermission(
      permission.session,
      permission.requestId,
      optionId
    );
    setPermissionQueue((previous) =>
      permissionQueueAfterAnswer(
        previous,
        permission.session,
        permission.requestId,
        accepted
      )
    );
  };

  const answerQuestion = async (value: ElicitationAnswer) => {
    if (permission == null) return;
    const accepted = await answerElicitation(
      permission.session,
      permission.requestId,
      value
    );
    setPermissionQueue((previous) =>
      permissionQueueAfterAnswer(
        previous,
        permission.session,
        permission.requestId,
        accepted
      )
    );
  };

  /**
   * The UI asks one question about permissions; the engine keeps two axes. This is where the one
   * becomes the two — both are always set together, so a session can't drift into a combination the
   * picker can't name (an "auto-edit" that a read-only sandbox silently vetoes).
   */
  const onSessionModeChange = (id: SessionMode): boolean => {
    const session = activeSessionRef.current;
    // `newSession` has already captured its first-turn policy, or this session is awaiting an
    // authoritative receipt. In both cases painting another choice would lie about what runs.
    if (
      executionPolicyChangeDisabled(
        pendingCreationRef.current !== null,
        session,
        pendingPolicyBySessionRef.current
      )
    )
      return false;
    const preset = SESSION_MODES.find((item) => item.id === id);
    if (!preset) return false;

    if (session != null && session !== "") {
      const requestId = globalThis.crypto.randomUUID();
      const authoritative = authoritativePoliciesRef.current.get(session) ?? {
        mode,
        sandbox,
      };
      pendingPolicyRequestsRef.current.set(requestId, {
        session,
        authoritative,
      });
      pendingPolicyBySessionRef.current.set(session, requestId);
      setPendingPolicySessions((current) => {
        if (current.has(session)) return current;
        const next = new Set(current);
        next.add(session);
        return next;
      });
      void setExecutionPolicy(
        session,
        preset.mode,
        preset.sandbox,
        requestId
      ).catch((error: unknown) => {
        const rejected = finishPolicyRequest(requestId);
        if (!rejected) return;
        restoreRejectedExecutionPolicy(rejected);
        toast(`Could not update execution policy: ${error}`, "error");
      });
    }

    setMode(preset.mode);
    setSandboxState(preset.sandbox);
    return true;
  };

  const onMemoryPolicyChange = (read: MemoryAccess, write: MemoryAccess) => {
    if (!componentEnabledRef.current("memory.settings")) return;
    const previousRead = memoryReadRef.current;
    const previousWrite = memoryWriteRef.current;
    memoryReadRef.current = read;
    memoryWriteRef.current = write;
    setMemoryRead(read);
    setMemoryWrite(write);
    const session = activeSessionRef.current;
    if (session != null && session !== "") {
      const update =
        (nextRead: MemoryAccess, nextWrite: MemoryAccess) =>
        (items: SessionInfo[]) =>
          items.map((item) =>
            item.id === session
              ? { ...item, memory_read: nextRead, memory_write: nextWrite }
              : item
          );
      setSessions(update(read, write));
      setArchivedSessions(update(read, write));
      void setSessionMemoryPolicy(session, read, write).catch(
        (error: unknown) => {
          memoryReadRef.current = previousRead;
          memoryWriteRef.current = previousWrite;
          setMemoryRead(previousRead);
          setMemoryWrite(previousWrite);
          setSessions(update(previousRead, previousWrite));
          setArchivedSessions(update(previousRead, previousWrite));
          toast(String(error), "error");
        }
      );
    }
  };

  const selectSession = async (
    id: string,
    requestedPaneId = focusedPaneRef.current,
    reloadTranscript = true,
    preserveFeaturePages = false
  ) => {
    flushActiveComposerDraft();
    const alreadyBoundPane =
      paneBoundToSession(paneContentsRef.current, id) ?? undefined;
    // Selecting a session already visible elsewhere focuses that pane instead of creating two
    // transcript consumers for one event stream.
    const paneId = alreadyBoundPane ?? requestedPaneId;
    if (focusedPaneRef.current !== paneId) {
      focusedPaneRef.current = paneId;
      setPaneLayout((layout) => focusPane(layout, paneId));
    }
    const shouldReloadTranscript =
      reloadTranscript &&
      (alreadyBoundPane === undefined || alreadyBoundPane === requestedPaneId);
    // Navigation replaces creation only in this pane. A draft creating in another pane keeps its
    // ownership and may finish while this session is focused.
    if (!preserveFeaturePages) {
      setShowTaskBoard(false);
      setShowPullRequests(false);
      setShowDocker(false);
      setShowFeishu(false);
    }
    invalidatePendingCreation(paneId);
    const stored =
      sessions.find((s) => s.id === id) ??
      archivedSessions.find((s) => s.id === id);
    if (stored) {
      setCwd(stored.cwd);
      setProvider(providerLabel(stored.provider));
      const policy = sessionExecutionPolicy(stored);
      if (policy) {
        setMode(policy.mode);
        setSandboxState(policy.sandbox);
      }
    }
    const storedProjectPath = stored ? sessionProjectPath(stored) : null;
    const projectPath =
      storedProjectPath != null && storedProjectPath !== ""
        ? (projects.find((project) => project.path === storedProjectPath)
            ?.path ?? null)
        : null;
    if (
      projectPath != null &&
      projectPath !== "" &&
      projectPath !== activeProjectRef.current
    ) {
      activeProjectRef.current = projectPath;
      setCallProjectPath(normalizePluginProjectPath(projectPath));
      setActiveProject(projectPath);
      void openProject(projectPath).then(refreshProjects);
    } else if (stored && (projectPath == null || projectPath === "")) {
      activeProjectRef.current = null;
      setCallProjectPath(null);
      setActiveProject(null);
    }

    const request = shouldReloadTranscript
      ? (sessionLoadSeqByPaneRef.current.get(paneId) ?? 0) + 1
      : (sessionLoadSeqByPaneRef.current.get(paneId) ?? 0);
    if (shouldReloadTranscript) {
      sessionLoadSeqByPaneRef.current.set(paneId, request);
      earlierLoadSeqByPaneRef.current.set(
        paneId,
        (earlierLoadSeqByPaneRef.current.get(paneId) ?? 0) + 1
      );
      earlierLoadRunningByPaneRef.current.delete(paneId);
      updatePaneTranscriptState(paneId, {
        loading: true,
        loadingEarlier: false,
        nextBefore: null,
      });
    }
    // R10: focus moved — release the dock-follow latch and drop the stale badge.
    followDockEvent({ kind: "session_switched" });
    activeSessionRef.current = id;
    if (stored?.model != null && stored?.model !== "")
      knownModelsRef.current.set(id, stored.model);
    else knownModelsRef.current.delete(id);
    const provenance = stored ? { session: id, shell: stored } : null;
    activeSessionProvenanceRef.current = provenance;
    setActiveSessionReceipt(provenance);
    setPaneSession(paneId, id);
    const draftScope: ComposerDraftScope = {
      kind: "session",
      sessionId: id,
      projectPath,
    };
    if (alreadyBoundPane === undefined) {
      restoreComposerDraftScope(draftScope, { restorePosture: false });
    } else {
      activeDraftScopeRef.current = draftScope;
      activeEditorDocRef.current =
        paneEditorRefsFor(paneId).getBlocksRef.current?.() ?? [];
    }
    const board = loadBoardSnapshot();
    const task =
      board.warning != null && board.warning !== ""
        ? null
        : taskForSession(board.tasks, id);
    setTaskContext(task, task === null);
    void prepareSession(id).catch(() => {
      /* empty */
    });
    {
      // Restore the concrete scene and Agent-owned routing independently: Auto can be enabled
      // before the Agent has selected the first scene.
      const remembered = sceneBySessionRef.current.get(id) ?? null;
      const rememberedAuto = autoSceneBySessionRef.current.get(id) ?? false;
      setActiveSceneName(remembered);
      setAutoScene(rememberedAuto);
      setScenePendingFields([]);
      if (componentEnabledRef.current("scenes.surface")) {
        void Promise.all([getSessionScene(id), getSessionAutoScene(id)]).then(
          ([state, enabled]) => {
            if (
              !componentEnabledRef.current("scenes.surface") ||
              activeSessionRef.current !== id
            ) {
              return;
            }
            autoSceneBySessionRef.current.set(id, enabled);
            setAutoScene(enabled);
            if (!state) return;
            sceneBySessionRef.current.set(id, state.reference);
            setActiveSceneName(state.reference);
            if (!state.resolved) toast(t("scene.unresolved"), "error");
          }
        );
      }
    }
    if (shouldReloadTranscript) setPaneTurns(paneId, []);
    // Models belong to a session. The agent only reports its own menu at session/new — which for
    // a session resumed from the store hasn't happened again yet — so start from the provider's
    // built-in list and let the agent's own options replace it when the next turn revives the
    // session.
    const forProvider = providers.find(
      (p) => p.id === providerLabel(stored?.provider ?? "")
    );
    setModels(modelsBySession[id] ?? forProvider?.models ?? []);
    setConfigOptions(configOptionsBySession[id] ?? []);
    setCurrentModel(currentModelBySession[id] ?? stored?.model ?? null);
    setDefaultModel(defaultModelBySession[id] ?? null);
    const nextRead = stored?.memory_read ?? "inherit";
    const nextWrite = stored?.memory_write ?? "inherit";
    memoryReadRef.current = nextRead;
    memoryWriteRef.current = nextWrite;
    if (shouldReloadTranscript) memoryReceiptsBySessionRef.current.delete(id);
    setMemoryRead(nextRead);
    setMemoryWrite(nextWrite);
    if (!shouldReloadTranscript) return;
    let observedTurnVersion = turnStartVersionsRef.current.get(id) ?? 0;
    try {
      let [page, receipts] = await Promise.all([
        getTranscriptPage(id),
        componentEnabledRef.current("memory.settings")
          ? listMemoryReceipts(id)
          : Promise.resolve([]),
      ]);
      // The core persists the prompt before broadcasting TurnStarted. If that boundary arrived
      // during this read, fetch once more so the persisted tail and live event buffer share an
      // explicit request identity instead of guessing from text content or array position.
      while (request === sessionLoadSeqByPaneRef.current.get(paneId)) {
        const currentTurnVersion = turnStartVersionsRef.current.get(id) ?? 0;
        if (currentTurnVersion === observedTurnVersion) break;
        observedTurnVersion = currentTurnVersion;
        page = await getTranscriptPage(id);
      }
      if (
        request !== sessionLoadSeqByPaneRef.current.get(paneId) ||
        paneContentsRef.current[paneId]?.sessionId !== id
      )
        return;
      memoryReceiptsBySessionRef.current.set(id, receipts);
      // Optimistic running state exists before TurnStarted/prompt persistence. It must not reopen
      // the previous persisted tail when the user re-selects this session during that window.
      const hasAuthoritativeTurn = runningPromptRequestsRef.current.has(id);
      const sessionIsRunning = runningSessionsRef.current.has(id);
      const tailState = transcriptTailState(
        sessionIsRunning,
        hasAuthoritativeTurn,
        latestTurnRequestIdsRef.current.get(id)
      );
      const loaded = turnsFromTranscript(
        page.entries,
        tailState.running,
        tailState.requestId,
        receipts
      );
      updateTranscriptCursor(paneId, page.next_before);
      setPaneTurns(paneId, (live) =>
        mergeLoadedTurns(
          loaded,
          live,
          runningSessionsRef.current.has(id) &&
            runningPromptRequestsRef.current.has(id)
        )
      );
    } catch (error) {
      if (request !== sessionLoadSeqByPaneRef.current.get(paneId)) return;
      memoryReceiptsBySessionRef.current.delete(id);
      setPaneTurns(paneId, []);
      updateTranscriptCursor(paneId, null);
      toast(t("toast.sessionLoadFailed", { error: String(error) }), "error");
    } finally {
      if (request === sessionLoadSeqByPaneRef.current.get(paneId)) {
        updatePaneTranscriptState(paneId, { loading: false });
      }
    }
  };

  useEffect(() => {
    let dispose: (() => void) | null = null;
    void onDesktopRevealSession(({ session }) => {
      void selectSession(session);
    }).then((unlisten) => {
      dispose = unlisten;
    });
    return () => dispose?.();
  }, [selectSession]);

  const activatePaneById = (paneId: string) => {
    const session = paneContentsRef.current[paneId]?.sessionId ?? null;
    if (session != null && session !== "") {
      void selectSession(session, paneId, false, true);
      return;
    }
    flushActiveComposerDraft();
    activeDraftScopeRef.current = {
      kind: "project",
      projectPath: (activeProjectRef.current ?? cwd) || ".",
    };
    activeEditorDocRef.current =
      paneEditorRefsFor(paneId).getBlocksRef.current?.() ?? [];
    activeSessionRef.current = null;
    activeSessionProvenanceRef.current = null;
    setActiveSessionReceipt(null);
    setTaskContext(null, false);
    setModels([]);
    setConfigOptions([]);
    setDefaultModel(null);
    memoryReadRef.current = "inherit";
    memoryWriteRef.current = "inherit";
    setMemoryRead("inherit");
    setMemoryWrite("inherit");
    setActiveSceneName(null);
    setAutoScene(false);
    setScenePendingFields([]);
    setPipelineDetail(null);
    updatePaneTranscriptState(paneId, { loading: false });
  };
  const activatedPaneRef = useRef(INITIAL_PANE_ID);
  const focusPaneById = (paneId: string) => {
    if (focusedPaneRef.current === paneId) return;
    focusedPaneRef.current = paneId;
    activatedPaneRef.current = paneId;
    // Set the authoritative ref before the click that follows this mouse-down can run an action.
    activeSessionRef.current =
      paneContentsRef.current[paneId]?.sessionId ?? null;
    setPaneLayout((layout) => focusPane(layout, paneId));
    activatePaneById(paneId);
  };
  useEffect(() => {
    if (activatedPaneRef.current === paneLayout.focused) return;
    activatedPaneRef.current = paneLayout.focused;
    activatePaneById(paneLayout.focused);
  }, [activatePaneById, paneLayout.focused]);

  useEffect(() => {
    let dispose: (() => void) | null = null;
    void onDeviceSyncChanged(() => {
      void refreshSessions();
      refreshProjects();
      const session = activeSessionRef.current;
      if (
        session != null &&
        session !== "" &&
        !runningSessionsRef.current.has(session)
      ) {
        void selectSession(session, focusedPaneRef.current, true, true);
      }
    }).then((unlisten) => {
      dispose = unlisten;
    });
    return () => dispose?.();
  }, [refreshProjects, refreshSessions, selectSession]);

  const loadEarlierTranscript = async (
    paneId: string,
    scroll?: TranscriptScrollController
  ) => {
    const session = paneContentsRef.current[paneId]?.sessionId ?? null;
    const before = (
      transcriptStateByPaneRef.current[paneId] ?? EMPTY_PANE_TRANSCRIPT_STATE
    ).nextBefore;
    if (
      session == null ||
      session === "" ||
      before === null ||
      earlierLoadRunningByPaneRef.current.has(paneId)
    )
      return;

    const sessionGeneration = sessionLoadSeqByPaneRef.current.get(paneId) ?? 0;
    const loadGeneration =
      (earlierLoadSeqByPaneRef.current.get(paneId) ?? 0) + 1;
    earlierLoadSeqByPaneRef.current.set(paneId, loadGeneration);
    const anchor = scroll?.capturePrependAnchor() ?? null;
    earlierLoadRunningByPaneRef.current.add(paneId);
    updatePaneTranscriptState(paneId, { loadingEarlier: true });
    try {
      const page = await getTranscriptPage(session, before);
      if (
        sessionGeneration !== sessionLoadSeqByPaneRef.current.get(paneId) ||
        loadGeneration !== earlierLoadSeqByPaneRef.current.get(paneId) ||
        session !== paneContentsRef.current[paneId]?.sessionId ||
        before !== transcriptStateByPaneRef.current[paneId]?.nextBefore
      ) {
        return;
      }
      const older = turnsFromTranscript(
        page.entries,
        false,
        undefined,
        memoryReceiptsBySessionRef.current.get(session) ?? []
      );
      if (older.length > 0) {
        scroll?.prepareForPrepend(anchor);
        setPaneTurns(paneId, (current) =>
          prependTranscriptTurns(current, older)
        );
      }
      updateTranscriptCursor(paneId, page.next_before);
    } catch (error) {
      if (
        sessionGeneration === sessionLoadSeqByPaneRef.current.get(paneId) &&
        loadGeneration === earlierLoadSeqByPaneRef.current.get(paneId)
      ) {
        toast(
          t("toast.transcriptEarlierFailed", { error: String(error) }),
          "error"
        );
      }
    } finally {
      if (loadGeneration === earlierLoadSeqByPaneRef.current.get(paneId)) {
        earlierLoadRunningByPaneRef.current.delete(paneId);
        updatePaneTranscriptState(paneId, { loadingEarlier: false });
      }
    }
  };

  const searchPaletteCommands = async (query: string): Promise<Command[]> => {
    const hits = await searchSessions(query, 12);
    return hits.map((hit) => {
      const stored =
        sessions.find((session) => session.id === hit.session_id) ??
        archivedSessions.find((session) => session.id === hit.session_id);
      const sourcePath = stored
        ? (sessionProjectPath(stored) ?? hit.cwd)
        : hit.cwd;
      const project =
        projects.find((item) => item.path === sourcePath)?.name ?? sourcePath;
      return {
        id: `conversation-${hit.session_id}-${hit.seq}`,
        identity: `session-${hit.session_id}`,
        category: "session",
        label: hit.title,
        detail: `${stored ? new Date(stored.created_at).toLocaleString(locale) : hit.session_id.slice(0, 8)} · ${t(hit.role === "user" ? "palette.you" : "palette.agent")}: ${hit.snippet}`,
        hint: hit.archived ? `${project} · ${t("palette.archived")}` : project,
        keywords: `${sourcePath} ${hit.cwd}`,
        run: () => void selectSession(hit.session_id),
      };
    });
  };

  // Skills depend on the workspace: harness skill directories (.claude/skills …) are rescanned
  // for the project the user is in, so the list refreshes on mount and on every project switch.
  const refreshSkills = useCallback(async () => {
    try {
      const next = await listSkills(cwd || ".");
      setSkills(next);
      return next;
    } catch {
      return [];
    }
  }, [cwd]);

  useEffect(() => {
    void refreshSkills();
  }, [refreshSkills]);

  const refreshManagedCatalogs = useCallback(
    async (scope: PluginManagerScope = pluginManagerScope) => {
      const projectPaths = new Set<string>();
      if (scope.kind === "project")
        projectPaths.add(normalizePluginProjectPath(scope.projectPath));
      if (activeProject != null && activeProject !== "")
        projectPaths.add(normalizePluginProjectPath(activeProject));
      return await refreshPluginsSnapshot([
        { kind: "user" },
        ...Array.from(projectPaths, (projectPath) => ({
          kind: "project" as const,
          projectPath,
        })),
      ]);
    },
    [activeProject, pluginManagerScope, refreshPluginsSnapshot]
  );
  const loadManagedCatalog = refreshManagedCatalogs;

  // Subscribe before the initial read so changes during bootstrap cannot be missed.
  useEffect(() => {
    let disposed = false;
    let unsubscribe = () => {};
    const refresh = () => {
      if (!disposed)
        void refreshManagedCatalogs().catch((error: unknown) => {
          console.warn("Could not refresh plugin snapshot", error);
        });
    };
    void onPluginsChanged(refresh)
      .then((stop) => {
        if (disposed) {
          stop();
          return;
        }
        unsubscribe = stop;
        refresh();
      })
      .catch((error: unknown) => {
        console.warn("Could not subscribe to plugin changes", error);
        refresh();
      });
    return () => {
      disposed = true;
      unsubscribe();
    };
  }, [refreshManagedCatalogs]);

  const pluginManagerProjects = (() => {
    const seen = new Set<string>();
    return projects.flatMap((project) => {
      const path = normalizePluginProjectPath(project.path);
      if (seen.has(path)) return [];
      seen.add(path);
      return [{ path, label: project.name }];
    });
  })();

  const selectedManagedCatalog =
    pluginManagerScope.kind === "user"
      ? managedUserCatalog
      : managedProjectCatalogs[
          normalizePluginProjectPath(pluginManagerScope.projectPath)
        ];
  const activeProjectCatalog =
    activeProject != null && activeProject !== ""
      ? managedProjectCatalogs[normalizePluginProjectPath(activeProject)]
      : undefined;
  const activeManagedCatalog =
    activeProject != null && activeProject !== ""
      ? (activeProjectCatalog ?? managedUserCatalog)
      : managedUserCatalog;
  const activeComponentPolicyReady =
    projectBootstrapComplete &&
    managedUserCatalog !== null &&
    (activeProject === null || activeProjectCatalog !== undefined);
  const pluginManagerModel = buildPluginManagerCatalog({
    catalog:
      selectedManagedCatalog ?? managedUserCatalog ?? EMPTY_MANAGED_CATALOG,
    userCatalog: managedUserCatalog ?? undefined,
    bundles: plugins,
    skills,
    market,
    localMarketplace: localPluginMarketplace,
    scope: pluginManagerScope,
  });
  const pluginManagerLabels = createPluginManagerLabels(t);
  const localizedPluginManagerModel = localizePluginManagerCatalog(
    pluginManagerModel,
    t
  );
  const activePluginModel = buildPluginManagerCatalog({
    catalog: activeManagedCatalog ?? EMPTY_MANAGED_CATALOG,
    userCatalog: managedUserCatalog ?? undefined,
    bundles: plugins,
    skills,
    market,
    scope:
      activeProject != null && activeProject !== ""
        ? {
            kind: "project",
            projectPath: normalizePluginProjectPath(activeProject),
          }
        : { kind: "user" },
  });
  const activeSkills = (() => {
    const enabledById = new Map(
      activePluginModel.components
        .filter((component) => component.skill)
        .map((component) => [component.id, component.state.effectiveEnabled])
    );
    return skills.filter(
      (skill) => enabledById.get(`skill:${skill.id}`) ?? true
    );
  })();
  const componentEnabled = (id: BuiltinUiComponentId) =>
    pluginManagerComponentEnabled(
      activePluginModel.components,
      id,
      activeComponentPolicyReady
    );
  componentEnabledRef.current = componentEnabled;
  const voiceComposerEnabled = componentEnabled("voice.composer");
  const memorySettingsEnabled = componentEnabled("memory.settings");
  const deviceSyncSettingsEnabled = componentEnabled("device-sync.settings");
  const scenesSurfaceEnabled = componentEnabled("scenes.surface");
  const lspRuntimeEnabled = componentEnabled("lsp.runtime");
  const lspPluginEnabled =
    activeComponentPolicyReady &&
    (activePluginModel.plugins.find((plugin) => plugin.id === "lsp")?.state
      .effectiveEnabled ??
      false);
  const lspProjectPath =
    activeProject != null && activeProject !== ""
      ? normalizePluginProjectPath(activeProject)
      : null;
  const pluginUiActions = (() => {
    const actions = activeComponentPolicyReady
      ? activePluginUiContributions(
          plugins,
          activePluginModel.plugins,
          activePluginModel.components
        )
      : activePluginUiContributions([], []);
    return actions;
  })();
  const dockerPlugin =
    plugins.find((plugin) => plugin.name === "docker-tools") ?? null;
  const dockerPluginReady =
    dockerPlugin?.enabled === true && dockerPlugin.trusted;
  useEffect(() => {
    if (showDocker && !dockerPlugin) setShowDocker(false);
  }, [dockerPlugin, showDocker]);
  const callDocker = useCallback<DockerCommandCaller>(
    async <T,>(name: string, args?: unknown) => await call<T>(name, args, null),
    []
  );
  const issueConnectors: IssueConnector[] = activePluginConnectorContributions(
    plugins,
    activePluginModel.plugins
  )
    .filter((connector) => connector.capabilities.includes("issues"))
    .map((connector) => ({
      plugin_id: connector.pluginId,
      connector_id: connector.id,
      name:
        plugins.find((plugin) => plugin.id === connector.pluginId)?.name ??
        connector.provider,
    }));
  const collaborationConnector =
    activePluginConnectorContributions(plugins, activePluginModel.plugins).find(
      (connector) => connector.provider === "feishu"
    ) ?? null;
  const callFeishu = async <T,>(operation: string, input?: unknown) => {
    if (!collaborationConnector)
      throw new Error("Collaboration connector is unavailable.");
    return await invokePluginConnector<T>(
      collaborationConnector.pluginId,
      collaborationConnector.id,
      operation,
      input ?? {},
      lspProjectPath
    );
  };
  const subscribeFeishuEvents: CollaborationConnectorSubscriber = async (
    callback
  ) => {
    if (!collaborationConnector)
      return () => {
        /* empty */
      };
    return await onPluginConnectorEvent((envelope) => {
      if (envelope.plugin_id !== collaborationConnector.pluginId) return;
      const event = asJsonObject(envelope.event);
      if (event == null) return;
      const eventKinds = [
        "message.created",
        "message.changed",
        "document.changed",
        "base.changed",
        "connection.changed",
      ] as const;
      if (
        event.connectorId !== collaborationConnector.id ||
        typeof event.eventId !== "string" ||
        !isOneOf(event.kind, eventKinds)
      )
        return;
      const next: CollaborationConnectorEvent = {
        connectorId: collaborationConnector.id,
        eventId: event.eventId,
        kind: event.kind,
        ...(typeof event.chatId === "string" ? { chatId: event.chatId } : {}),
        ...(typeof event.messageId === "string"
          ? { messageId: event.messageId }
          : {}),
        ...(typeof event.resourceId === "string"
          ? { resourceId: event.resourceId }
          : {}),
        ...(typeof event.resourceType === "string"
          ? { resourceType: event.resourceType }
          : {}),
        ...(typeof event.preview === "string"
          ? { preview: event.preview }
          : {}),
        ...(typeof event.createdAt === "string"
          ? { createdAt: event.createdAt }
          : {}),
        ...(typeof event.state === "string" ? { state: event.state } : {}),
        ...(typeof event.problem === "string"
          ? { problem: event.problem }
          : {}),
      };
      callback(next);
    });
  };
  const pluginLanguageServers = activeComponentPolicyReady
    ? activePluginLanguageServers(plugins, activePluginModel.plugins)
    : [];
  useLayoutEffect(() => {
    configurePluginLanguageServers(pluginLanguageServers);
  }, [pluginLanguageServers]);
  const invokePluginAction = async (
    contribution: ActivePluginUiContribution
  ) => {
    try {
      const result = await invokePluginUi(
        contribution.pluginId,
        contribution.id,
        {
          cwd: workspaceCwd,
          projectPath: lspProjectPath,
          sessionId: activeSession,
        },
        lspProjectPath
      );
      const resultRecord = asJsonObject(result);
      const message =
        typeof result === "string"
          ? result
          : (stringField(resultRecord, "message") ??
            `${contribution.label} completed.`);
      toast(message, "success");
    } catch (error) {
      toast(`${contribution.label} failed: ${String(error)}`, "error");
    }
  };
  // Close the renderer gate synchronously, then reopen it only after this project's backend has
  // resumed. This keeps mounted editor effects from racing a suspended project realm.
  useLayoutEffect(() => {
    let current = true;
    void synchronizeLspRuntimePolicy(
      {
        catalogReady: activeComponentPolicyReady,
        pluginEnabled: lspPluginEnabled,
        componentEnabled: lspRuntimeEnabled,
        projectPath: lspProjectPath,
        workspace: workspaceCwd,
      },
      async (enabled) => await lspSetRuntimeEnabled(enabled, lspProjectPath),
      () => current && componentEnabledRef.current("lsp.runtime")
    ).catch((error: unknown) =>
      console.warn("Could not update language-server runtime policy", error)
    );
    return () => {
      current = false;
    };
  }, [
    activeComponentPolicyReady,
    lspPluginEnabled,
    lspProjectPath,
    lspRuntimeEnabled,
  ]);
  const availableDockSurfaces: DockSurface[] = [
    "trajectory",
    ...(componentEnabled("browser.dock") ? ["browser" as const] : []),
    ...(componentEnabled("terminal.dock") ? ["terminal" as const] : []),
    "side-chat",
    ...(componentEnabled("files.surface") ? ["files" as const] : []),
    ...(componentEnabled("git.surface") ? ["git" as const] : []),
    ...(componentEnabled("git.surface") ? ["pull-request" as const] : []),
  ];

  // A live disable removes the surface immediately, including already-open dialogs. Runtime
  // cleanup is owned by the plugin scope; this closes only renderer projections of that scope.
  useEffect(() => {
    if (
      dockTab &&
      dockTab !== "home" &&
      !availableDockSurfaces.includes(dockTab)
    ) {
      manualDockTab(null);
    }
    if (!componentEnabled("files.surface")) setShowFiles(false);
    if (!componentEnabled("search.modal")) setShowWorkspaceSearch(false);
    if (!componentEnabled("issues.modal")) setShowIssues(false);
    if (!componentEnabled("git.surface")) setShowSourceControl(false);
    if (!componentEnabled("remote.modal")) setShowRemote(false);
    if (!componentEnabled("automation.page")) setShowAutomations(false);
    if (!scenesSurfaceEnabled) {
      setShowScenePicker(false);
      setShowSceneStudio(false);
      setSceneEditorRequest(null);
      setSceneEscalation(null);
      setSceneBanner(null);
      setPipelineDetail(null);
      pendingSceneRef.current = null;
      pendingPipelineBindRef.current = null;
      pendingDelegationRef.current = null;
      pendingIssueInsertRef.current = null;
    }
  }, [
    availableDockSurfaces,
    componentEnabled,
    dockTab,
    manualDockTab,
    scenesSurfaceEnabled,
  ]);

  const refreshScenes = useStableCallback(async () => {
    if (!scenesSurfaceEnabled) {
      scenesRef.current = [];
      setScenes([]);
      return [];
    }
    const next = await listScenes(cwd || ".");
    if (!componentEnabledRef.current("scenes.surface")) return [];
    scenesRef.current = next;
    setScenes(next);
    return next;
  });

  // Scenes rescan with the workspace, same contract as skills. Degrades to [] on an older core.
  useEffect(() => {
    if (!scenesSurfaceEnabled) {
      setPipelines([]);
      return;
    }
    void refreshScenes();
    // Pipelines resolve from the same library; the palette's "start pipeline" commands feed here.
    void listPipelines().then((next) => {
      if (componentEnabledRef.current("scenes.surface")) setPipelines(next);
    });
  }, [cwd, refreshScenes, scenesSurfaceEnabled]);

  // The stage track follows the active session's pipeline binding (R9). Refetched at turn
  // boundaries and on banner changes — auto advances, loop re-entries, and artifact captures all
  // land there. Degrades to hidden (null) on an older core or an unbound session.
  useEffect(() => {
    const session = activeSession;
    if (session == null || session === "" || !scenesSurfaceEnabled) {
      setPipelineDetail(null);
      return;
    }
    let cancelled = false;
    void sessionPipeline(session).then((binding) => {
      if (cancelled) return;
      if (!binding) {
        setPipelineDetail(null);
        return;
      }
      void getPipelineInstance(binding.instance_id).then((detail) => {
        if (!cancelled) setPipelineDetail(detail);
      });
    });
    return () => {
      cancelled = true;
    };
  }, [activeSession, turns.length, sceneBanner, scenesSurfaceEnabled]);

  const saveDraft = async () => {
    if (!skillDraft || skillDraft.name.trim().length === 0) return;
    await saveSkill({
      id: slug(skillDraft.name),
      name: skillDraft.name.trim(),
      description: "",
      icon: null,
      payload: { kind: "fragment", text: skillDraft.text },
    });
    setSkillDraft(null);
    void refreshSkills();
  };

  // Mirrors `cwd` so an in-flight git fetch can tell it's stale. Without this, the mount-time
  // fetch for the engine's own cwd (".") could resolve *after* the fetch for the project you
  // switched to and paint another repo's branch and diff into the rail.
  const cwdRef = useRef(cwd);
  cwdRef.current = cwd;

  useEffect(() => {
    gitRefreshSeq.current += 1;
    checkpointRefreshSeq.current += 1;
  }, [cwd]);

  const refreshGit = useCallback(() => {
    const target = cwd || ".";
    const request = (gitRefreshSeq.current += 1);
    const fresh = () =>
      gitRefreshSeq.current === request && (cwdRef.current || ".") === target;
    setGitWorkspace({ cwd: target, loading: true, value: EMPTY_GIT_WORKSPACE });
    void (async () => {
      const [statusResult, sourceControlResult] = await Promise.allSettled([
        gitStatus(target),
        gitSourceControlInfo(target),
      ]);
      if (!fresh()) return;
      if (statusResult.status === "rejected") {
        setGitWorkspace({
          cwd: target,
          loading: false,
          value: EMPTY_GIT_WORKSPACE,
        });
        return;
      }

      const status = statusResult.value;
      const sourceControl =
        sourceControlResult.status === "fulfilled"
          ? sourceControlResult.value
          : null;
      let forgeError =
        sourceControlResult.status === "rejected"
          ? String(sourceControlResult.reason)
          : null;
      let pullRequest: GitHubPullRequest | null = null;
      if (status.is_repo && sourceControl?.provider === "github") {
        if (
          sourceControl.required_cli != null &&
          sourceControl.required_cli !== "" &&
          !sourceControl.required_cli_available
        ) {
          forgeError = `${sourceControl.required_cli} is unavailable`;
        } else {
          try {
            pullRequest = await githubCurrentPullRequest(target);
          } catch (error) {
            forgeError = String(error);
          }
        }
      }

      if (!fresh()) return;
      setGitWorkspace({
        cwd: target,
        loading: false,
        value: {
          status,
          diffStat: EMPTY_DIFF_STAT,
          sourceControl,
          pullRequest,
          forgeError,
        },
      });
      if (status.is_repo && status.files.length > 0) {
        gitDiffStat(target)
          .then((stat) => {
            if (!fresh()) return;
            setGitWorkspace((current) =>
              current.cwd === target
                ? {
                    ...current,
                    value: {
                      ...current.value,
                      diffStat: {
                        added: stat.added,
                        deleted: stat.deleted,
                        truncated: stat.truncated,
                      },
                    },
                  }
                : current
            );
          })
          .catch(() => {
            /* empty */
          });
      }
    })().catch(() => {
      if (fresh()) {
        setGitWorkspace({
          cwd: target,
          loading: false,
          value: EMPTY_GIT_WORKSPACE,
        });
      }
    });
  }, [cwd]);

  const refreshCheckpoints = useCallback(() => {
    const target = cwd || ".";
    // A callback captured before a project switch must not invalidate the current project's load.
    if ((cwdRef.current || ".") !== target) return;
    const request = (checkpointRefreshSeq.current += 1);
    setCheckpointWorkspace({
      cwd: target,
      loading: true,
      value: EMPTY_CHECKPOINTS,
    });
    gitCheckpoints(target)
      .then((next) => {
        if (
          checkpointRefreshSeq.current === request &&
          (cwdRef.current || ".") === target
        ) {
          setCheckpointWorkspace({ cwd: target, loading: false, value: next });
        }
      })
      .catch(() => {
        if (
          checkpointRefreshSeq.current === request &&
          (cwdRef.current || ".") === target
        ) {
          setCheckpointWorkspace({
            cwd: target,
            loading: false,
            value: EMPTY_CHECKPOINTS,
          });
        }
      });
  }, [cwd]);

  const openPluginManagerFor = (pluginId: string | null) => {
    const normalizedActiveProject =
      activeProject != null && activeProject !== ""
        ? normalizePluginProjectPath(activeProject)
        : null;
    const scope: PluginManagerScope =
      normalizedActiveProject != null &&
      normalizedActiveProject !== "" &&
      pluginManagerProjects.some(
        (project) => project.path === normalizedActiveProject
      )
        ? { kind: "project", projectPath: normalizedActiveProject }
        : { kind: "user" };
    setPluginManagerScope(scope);
    setLocalPluginMarketplace(null);
    marketCatalog()
      .then(setMarket)
      .catch(() => {});
    void refreshManagedCatalogs(scope).catch(() => {});
    void refreshSkills();
    setPluginManagerInitialPluginId(pluginId);
    setShowAutomations(false);
    setShowTaskBoard(false);
    setShowPullRequests(false);
    setShowDocker(false);
    setShowFeishu(false);
    setShowPluginManager(true);
    if (railOverlay) setNarrowRailOpen(false);
  };
  const openPluginManager = () => {
    openPluginManagerFor(null);
  };
  const openFeishuPluginSettings = () => {
    if (!collaborationConnector) return;
    openPluginManagerFor(`bundle:${collaborationConnector.pluginId}`);
  };

  const refreshPluginManagerData = useCallback(
    async (scope: PluginManagerScope = pluginManagerScope) => {
      const [nextMarket] = await Promise.all([
        marketCatalog(),
        refreshSkills(),
        refreshManagedCatalogs(scope),
      ]);
      setMarket(nextMarket);
    },
    [pluginManagerScope, refreshManagedCatalogs, refreshSkills]
  );

  const planManagerChange = async (
    request: PluginManagerChangeRequest
  ): Promise<PluginManagerChangePlan> => {
    const plan = await planPluginManagerChange({
      request,
      plugins: localizedPluginManagerModel.plugins,
      components: localizedPluginManagerModel.components,
      planChange: planPluginChange,
    });
    return {
      ...plan,
      summary: pluginManagerLabels.changeSummary(
        request.targetKind,
        request.targetName,
        request.desiredState
      ),
    };
  };

  const applyManagerChange = async (plan: PluginManagerChangePlan) => {
    await applyPluginManagerChange(plan, applyPluginChange);
    await refreshPluginManagerData(plan.request.scope);
    toast(
      pluginManagerLabels.changeApplied(
        plan.request.targetName,
        plan.request.desiredState
      ),
      "success"
    );
  };

  const saveManagerConfig = async ({
    pluginId,
    scope,
    config,
  }: {
    pluginId: string;
    scope: PluginManagerScope;
    config: unknown;
  }) => {
    const plugin = pluginManagerModel.plugins.find(
      (item) => item.id === pluginId
    );
    if (!plugin || plugin.source === "bundle") {
      throw new Error(
        "This bundle does not expose a host-validated configuration schema."
      );
    }
    const plan = await planPluginChange({
      plugin: plugin.id,
      scope: toManagedPluginScope(scope),
      config,
    });
    await applyPluginChange(plan.id);
    await refreshPluginManagerData(scope);
    toast(`${plugin.name} configuration saved and reloaded.`, "success");
  };

  const openAutomations = () => {
    if (!componentEnabled("automation.page")) {
      toast("Automations are disabled in Plugins.", "info");
      return;
    }
    setShowTaskBoard(false);
    setShowPluginManager(false);
    setShowPullRequests(false);
    setShowDocker(false);
    setShowFeishu(false);
    setShowAutomations(true);
    if (railOverlay) setNarrowRailOpen(false);
  };

  const openPullRequests = () => {
    if (!componentEnabled("git.surface")) {
      toast("Source control is disabled in Plugins.", "info");
      return;
    }
    setShowAutomations(false);
    setShowPluginManager(false);
    setShowTaskBoard(false);
    setShowDocker(false);
    setShowFeishu(false);
    readPullRequestTasks();
    setShowPullRequests(true);
    if (railOverlay) setNarrowRailOpen(false);
  };

  const openDocker = () => {
    setShowAutomations(false);
    setShowPluginManager(false);
    setShowPullRequests(false);
    setShowTaskBoard(false);
    setShowFeishu(false);
    setShowDocker(true);
    if (railOverlay) setNarrowRailOpen(false);
  };

  const openFeishu = () => {
    setShowAutomations(false);
    setShowPluginManager(false);
    setShowPullRequests(false);
    setShowTaskBoard(false);
    setShowDocker(false);
    setShowFeishu(true);
    if (railOverlay) setNarrowRailOpen(false);
  };

  const openSourceControl = () => {
    if (!componentEnabled("git.surface")) {
      toast("Source control is disabled in Plugins.", "info");
      return;
    }
    setShowSourceControl(true);
  };

  const openWorkingDirectory = async (target: WorkspaceOpenTarget) => {
    const application =
      target === "cursor"
        ? t("header.cursor")
        : target === "antigravity"
          ? t("header.antigravity")
          : fileManagerLabel;
    try {
      if (!(await openWorkspace(cwd || ".", target))) {
        throw new Error("Workspace launcher unavailable");
      }
    } catch {
      toast(t("header.openFailed", { application }), "error");
    }
  };

  const doCheckpoint = async () => {
    try {
      const cp = await gitCheckpoint(cwd || ".", "manual checkpoint");
      toast(
        cp ? "Checkpoint saved." : "Nothing to checkpoint.",
        cp ? "success" : "info"
      );
    } catch (error) {
      toast(`Checkpoint failed: ${error}`, "error");
    }
    refreshCheckpoints();
  };

  const doPush = async () => {
    try {
      await gitPush(cwd || ".");
      toast("Pushed.", "success");
    } catch (error) {
      toast(`Push failed: ${error}`, "error");
      throw error;
    } finally {
      refreshGit();
    }
  };

  const doPreview = async () => {
    const getBlocks = getBlocksRef.current;
    if (!getBlocks) return;
    try {
      const current = getBlocks();
      const frozen = freezeCanvasesRef.current
        ? await freezeCanvasesRef.current(current)
        : current;
      setPreview(await compileDoc(frozen, cwd || "."));
    } catch (error) {
      toast(`Could not compile the document: ${error}`, "error");
    }
  };

  /* One card per annotation. The core renders the markdown the agent will see; the editor shows
     it as a dedicated block — host, element, note, style edits — instead of a wall of text. */
  const annotate = async (notes: Annotation[]) => {
    for (const a of notes) {
      const ctx = await browserContext(a);
      insertAnnotationRef.current?.(a, ctx);
    }
  };

  const insertIssue = async (issue: Issue) => {
    const ctx = await issueContext(issue);
    insertIssueRef.current?.(issue, ctx);
    setShowIssues(false);
  };

  const toggleDock = (t: DockSurface) => {
    const component: Partial<Record<DockSurface, BuiltinUiComponentId>> = {
      browser: "browser.dock",
      terminal: "terminal.dock",
      files: "files.surface",
      git: "git.surface",
      "pull-request": "git.surface",
    };
    const componentId = component[t];
    if (componentId && !componentEnabled(componentId)) {
      toast(
        `${t[0]?.toUpperCase()}${t.slice(1)} is disabled in Plugins.`,
        "info"
      );
      return;
    }
    // A manual dock choice, so it routes through the follow reducer and latches auto-follow.
    manualDockTab(dockTabRef.current === t ? null : t);
    setTimeout(() => window.dispatchEvent(new Event("resize")), 0);
  };

  const toggleSidePanel = () => {
    manualDockTab(dockTabRef.current === null ? "home" : null);
    setTimeout(() => window.dispatchEvent(new Event("resize")), 0);
  };

  const runProjectAction = (script: ProjectScript) => {
    if (script.kind === "prompt") {
      const doc: DocBlock[] = [{ type: "text", text: script.prompt }];
      const session = activeSessionRef.current;
      if (
        session != null &&
        session !== "" &&
        runningSessionsRef.current.has(session)
      ) {
        void sendDuringTurn("queued", doc);
      } else {
        void run(doc);
      }
      return;
    }
    const name = script.name || script.id;
    if (script.preview_url && script.open_preview) {
      if (componentEnabled("browser.dock")) {
        setBrowserUrl(script.preview_url);
        void browserRegistryCreate(script.preview_url).catch(
          (error: unknown) => {
            toast(t("actionDialog.failed", { error: String(error) }), "error");
          }
        );
        manualDockTab("browser");
        setTimeout(() => window.dispatchEvent(new Event("resize")), 0);
      } else {
        toast("Browser preview is disabled in Plugins.", "info");
      }
    }
    toast(t("actionDialog.running", { name }));
    void runProjectScript(cwd || ".", script.id)
      .then((output) => {
        const message = output.trim()
          ? output.trim().slice(-300)
          : t("actionDialog.finished", { name });
        toast(message, "success");
      })
      .catch((error: unknown) =>
        toast(t("actionDialog.failed", { error: String(error) }), "error")
      );
  };

  const saveProjectAction = async (script: ProjectScript) => {
    const saved = await saveProjectScript(cwd || ".", script);
    setScripts((current) => {
      const index = current.findIndex((candidate) => candidate.id === saved.id);
      if (index === -1) return [...current, saved];
      return current.map((candidate, candidateIndex) =>
        candidateIndex === index ? saved : candidate
      );
    });
    toast(t("actionDialog.saved", { name: saved.name || saved.id }), "success");
  };

  // Expanding hands the whole column to the document; focus follows so you can just start writing.
  const toggleDocMode = (v: boolean) => {
    const focusDocument = () => {
      if (v) setTimeout(() => focusEditorRef.current?.(), 0);
    };
    const transitionDocument = document as Document & {
      startViewTransition?: (update: () => void) => {
        finished: Promise<void>;
      };
    };
    const startViewTransition =
      transitionDocument.startViewTransition?.bind(document);
    const reducedMotion = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)"
    ).matches;

    if (startViewTransition == null || reducedMotion) {
      setDocMode(v);
      focusDocument();
      return;
    }

    document.documentElement.dataset.composerModeTransition = "";
    try {
      const transition = startViewTransition(() => {
        flushSync(() => setDocMode(v));
      });
      void transition.finished.finally(() => {
        delete document.documentElement.dataset.composerModeTransition;
        focusDocument();
      });
    } catch {
      delete document.documentElement.dataset.composerModeTransition;
      setDocMode(v);
      focusDocument();
    }
  };

  const getCanvasAssets = (id: string): readonly CanvasStaticAsset[] => {
    const current = canvasAssetsRef.current.get(id);
    if (current) return [...current.values()];
    const draft = canvasDraftsRef.current.get(id);
    return draft?.assets ?? [];
  };

  const rememberCanvasDraft = (draft: CanvasDraft) => {
    canvasDraftsRef.current.set(draft.id, draft);
    const assets = new Map(draft.assets.map((asset) => [asset.id, asset]));
    canvasAssetsRef.current.set(draft.id, assets);
  };

  const normalizeCanvasMedia = async (
    canvasId: string,
    input: import("./canvas/media").CanvasMediaInput
  ) => {
    const bytes =
      input.bytes instanceof Uint8Array
        ? input.bytes
        : input.bytes instanceof ArrayBuffer
          ? new Uint8Array(input.bytes)
          : new Uint8Array(await input.bytes.arrayBuffer());
    const normalized = await canvasNormalizeMedia(bytes, input.mimeType);
    const media = {
      ref: normalized.id,
      bytes: new Uint8Array(normalized.bytes),
      mimeType: normalized.mimeType,
      name: input.name,
      width: normalized.width,
      height: normalized.height,
    } as const;
    const existing =
      canvasAssetsRef.current.get(canvasId) ??
      new Map<string, CanvasStaticAsset>();
    existing.set(normalized.id, normalized);
    canvasAssetsRef.current.set(canvasId, existing);
    return media;
  };

  const resolveCanvasAsset = async (
    canvasId: string,
    asset: {
      ref: string;
      fileId: string;
      mimeType: "image/png" | "image/webp";
    }
  ) => {
    const stored =
      canvasAssetsRef.current.get(canvasId)?.get(asset.ref) ??
      canvasAssetsRef.current.get(canvasId)?.get(asset.fileId);
    if (stored) {
      return {
        ref: stored.id,
        fileId: stored.id,
        mimeType: stored.mimeType,
        bytes: new Uint8Array(stored.bytes),
      };
    }
    return null;
  };

  const saveCanvasDraft = async (
    canvasId: string,
    envelope: LocalCanvasEnvelope,
    assets: readonly CanvasStaticAsset[]
  ) => {
    const current = canvasDraftsRef.current.get(canvasId);
    const update = {
      title: current?.title ?? "Canvas",
      theme: envelope.theme,
      envelope: localCanvasScene(envelope, assets),
      manifest: localCanvasManifest(envelope),
      assets: [...assets],
    };
    const saved = await canvasUpdateDraft(canvasId, envelope.revision, update);
    rememberCanvasDraft(saved);
    return saved;
  };

  const freezeCanvasDraft = async (
    canvasId: string,
    envelope: LocalCanvasEnvelope,
    assets: readonly CanvasStaticAsset[],
    exports: readonly CanvasExport[],
    _pixelPolicy: CanvasPixelPolicy
  ): Promise<CanvasSnapshot> => {
    const current = canvasDraftsRef.current.get(canvasId);
    const frozen = await canvasFreeze(canvasId, envelope.revision, {
      title: current?.title ?? "Canvas",
      theme: envelope.theme,
      envelope: localCanvasScene(envelope, assets),
      manifest: localCanvasManifest(envelope),
      assets: [...assets],
      // Keep validated PNG exports in the immutable revision for history and later provider
      // retries. The pixel policy is applied by core lowering, not by dropping evidence here.
      exports: [...exports],
    });
    return frozen;
  };

  const forgetCanvasHead = (canvasId: string) => {
    canvasDraftsRef.current.delete(canvasId);
    canvasAssetsRef.current.delete(canvasId);
    canvasFrozenRef.current.delete(canvasId);
  };

  const purgeCanvasHead = async (canvasId: string) => {
    const hasMutableHead =
      canvasDraftsRef.current.has(canvasId) ||
      canvasAssetsRef.current.has(canvasId);
    const plan = canvasUnmountPlan(
      hasMutableHead,
      canvasTombstonesRef.current.has(canvasId)
    );
    if (!plan.purge) return;
    canvasPurgeRequestedRef.current.add(canvasId);
    if (plan.tombstone) {
      canvasTombstonesRef.current.add(canvasId);
      await canvasTombstone(canvasId);
    }
    await canvasPurge(canvasId);
    canvasTombstonesRef.current.delete(canvasId);
    canvasPurgeRequestedRef.current.delete(canvasId);
    forgetCanvasHead(canvasId);
  };

  const removeCanvasDraft = (canvasId: string, nonEmpty: boolean) => {
    canvasTombstonesRef.current.add(canvasId);
    if (!nonEmpty) {
      canvasPurgeRequestedRef.current.delete(canvasId);
      void canvasPurge(canvasId)
        .then(() => forgetCanvasHead(canvasId))
        .catch(() => {
          /* empty */
        });
      return;
    }
    void canvasTombstone(canvasId)
      .then(async () => {
        if (!canvasPurgeRequestedRef.current.has(canvasId)) return;
        return await purgeCanvasHead(canvasId);
      })
      .catch((error: unknown) => {
        toast(
          `Canvas removal could not be recorded: ${String(error)}`,
          "error"
        );
      });
  };

  const restoreCanvasDraft = (canvasId: string) => {
    if (!canvasTombstonesRef.current.has(canvasId)) return;
    canvasTombstonesRef.current.delete(canvasId);
    void canvasRestore(canvasId).catch((error: unknown) => {
      toast(`Canvas restore failed: ${String(error)}`, "error");
    });
  };

  const purgeCanvasOnUnmount = (canvasId: string) => {
    void purgeCanvasHead(canvasId).catch(() => {
      /* empty */
    });
  };

  const canvasUiEnabled =
    canvasFeature.enabled && componentEnabled("canvas.editor");
  const canvasRuntime: CanvasBlockRuntime | null = {
    enabled: canvasUiEnabled,
    normalizeMedia: normalizeCanvasMedia,
    resolveAsset: resolveCanvasAsset,
    getAssets: getCanvasAssets,
    onAsset: (canvasId, asset) => {
      const assets =
        canvasAssetsRef.current.get(canvasId) ??
        new Map<string, CanvasStaticAsset>();
      assets.set(asset.id, asset);
      canvasAssetsRef.current.set(canvasId, assets);
    },
    onCanvasActivity: () => {
      /* empty */
    },
    saveDraft: saveCanvasDraft,
    freezeDraft: freezeCanvasDraft,
    onCanvasRemoved: removeCanvasDraft,
    onCanvasRestored: restoreCanvasDraft,
    onCanvasUnmount: (canvasId) => purgeCanvasOnUnmount(canvasId),
    onCanvasFrozen: (canvasId) => canvasFrozenRef.current.add(canvasId),
    onCanvasDeliveryError: (_canvasId, message) => toast(message, "error"),
    register: () => () => {
      /* empty */
    },
  };

  const createCanvas = async () => {
    if (!canvasUiEnabled) {
      const error = new Error(
        componentEnabled("canvas.editor")
          ? canvasFeature.status
          : "Canvas is disabled in Plugins."
      );
      toast(error.message, "error");
      throw error;
    }
    const draft = await canvasCreateDraft("Canvas");
    rememberCanvasDraft(draft);
    return draft;
  };

  useEffect(() => {
    const onDuplicate = (event: Event) => {
      if (!(event instanceof CustomEvent)) return;
      const detail = asJsonObject(event.detail);
      const id = stringField(detail, "id");
      const revision = detail?.revision;
      if (
        id == null ||
        id === "" ||
        typeof revision !== "number" ||
        !Number.isFinite(revision)
      )
        return;
      if (!canvasUiEnabled) {
        toast(
          componentEnabled("canvas.editor")
            ? canvasFeature.status
            : "Canvas is disabled in Plugins.",
          "error"
        );
        return;
      }
      void canvasDuplicate(id, revision)
        .then((draft) => {
          rememberCanvasDraft(draft);
          insertCanvasDraftRef.current?.(draft);
        })
        .catch((error: unknown) =>
          toast(`Canvas duplicate failed: ${String(error)}`, "error")
        );
    };
    window.addEventListener("codetwo-canvas-duplicate", onDuplicate);
    return () =>
      window.removeEventListener("codetwo-canvas-duplicate", onDuplicate);
  }, [
    canvasFeature.status,
    canvasUiEnabled,
    componentEnabled,
    rememberCanvasDraft,
    toast,
  ]);

  useEffect(() => {
    let confirmationOpen = false;
    const onVisualizeFollowUp = (event: Event) => {
      if (!(event instanceof CustomEvent)) return;
      const detail = asJsonObject(event.detail);
      const prompt = stringField(detail, "prompt")?.trim();
      if (
        prompt == null ||
        prompt === "" ||
        !insertTextRef.current ||
        confirmationOpen
      )
        return;
      confirmationOpen = true;
      void confirmNative(
        t("visualization.followUp", { prompt }),
        stringField(detail, "title") ?? t("visualization.title")
      )
        .then((accepted) => {
          if (!accepted) return;
          insertTextRef.current?.(prompt);
          setTimeout(() => focusEditorRef.current?.(), 0);
        })
        .finally(() => {
          confirmationOpen = false;
        });
    };
    window.addEventListener("codetwo-visualize-follow-up", onVisualizeFollowUp);
    return () =>
      window.removeEventListener(
        "codetwo-visualize-follow-up",
        onVisualizeFollowUp
      );
  }, [t]);

  /** Open a file as a tab in the right panel's editor, and bring that panel to the front. */
  const openFileTab = (
    p: string,
    position?: Pick<WorkspaceContentMatch, "line" | "column">
  ) => {
    if (!componentEnabled("files.surface")) {
      toast("Files are disabled in Plugins.", "info");
      return;
    }
    setOpenFiles((prev) => (prev.includes(p) ? prev : [...prev, p]));
    setActiveFile(p);
    setFileReveal(
      position
        ? {
            path: p,
            line: position.line,
            column: position.column,
            requestId: (fileRevealRequestRef.current += 1),
          }
        : null
    );
    setDockTab("files");
    // The files surface is an editor *and* a tree; at the dock's chat-sized default the code
    // column is a sliver. Take the room the document can spare, up to a readable measure.
    if (dockWidth < 640)
      setDockWidth(Math.min(Math.max(300, window.innerWidth - 620), 800));
    setTimeout(() => window.dispatchEvent(new Event("resize")), 0);
  };

  const openBuiltinWebLink = (url: string) => {
    if (!componentEnabled("browser.dock")) {
      toast("Browser is disabled in Plugins.", "info");
      return;
    }
    setBrowserUrl(url);
    void browserRegistryCreate(url).catch((error: unknown) => {
      toast(t("actionDialog.failed", { error: String(error) }), "error");
    });
    manualDockTab("browser");
    setTimeout(() => window.dispatchEvent(new Event("resize")), 0);
  };

  const openBuiltinFileLink = (
    target: Extract<BuiltinLinkTarget, { kind: "file" }>
  ) => {
    if (!componentEnabled("files.surface")) {
      toast("Files are disabled in Plugins.", "info");
      return;
    }
    const path = workspaceRelativeLinkPath(target.path, cwd || ".");
    if (path == null || path === "") return;
    openFileTab(
      path,
      target.line == null
        ? undefined
        : { line: target.line, column: target.column ?? 1 }
    );
  };

  const builtinLinkActions = {
    workspaceRoot: cwd || ".",
    openWebLink: componentEnabled("browser.dock")
      ? openBuiltinWebLink
      : undefined,
    openFileLink: componentEnabled("files.surface")
      ? openBuiltinFileLink
      : undefined,
  };

  const closeFileTab = async (p: string) => {
    // Unsaved edits die with the tab — say so first, like any editor would.
    if (isFileDirty(dirtyKey(cwd, p))) {
      const name = p.split("/").pop() ?? p;
      if (!(await confirmNative(t("files.confirmClose", { name })))) return;
      markDirty(dirtyKey(cwd, p), false);
    }
    const at = openFiles.indexOf(p);
    const next = openFiles.filter((x) => x !== p);
    setOpenFiles(next);
    // Closing the visible tab lands on its neighbour, not on an empty pane, VS Code-style.
    if (activeFile === p) {
      setActiveFile(next[Math.min(Math.max(at, 0), next.length - 1)] ?? null);
    }
  };

  const stepSession = (delta: number) => {
    if (sessions.length === 0) return;
    const at = sessions.findIndex((s) => s.id === activeSession);
    const next = sessions[(at + delta + sessions.length) % sessions.length];
    if (next != null) void selectSession(next.id);
  };

  /**
   * Every action in the keymap must land here. An action with no arm is a key that silently does
   * nothing — `open_skill_picker` and `focus_editor` were exactly that.
   */
  /**
   * Soft-apply a scene (or clear with null). Tightening applies silently; loosening stops here
   * and raises the escalation dialog — the rule is absolute, so this is the only UI path in.
   */
  const applySceneChoice = (
    reference: string | null,
    opts?: { confirmed?: boolean }
  ) => {
    if (!componentEnabledRef.current("scenes.surface")) return;
    const session = activeSessionRef.current;
    autoSceneRef.current = false;
    setAutoScene(false);
    if (session != null && session !== "") {
      autoSceneBySessionRef.current.set(session, false);
      void setSessionAutoScene(session, false);
    }
    if (reference === null) {
      setActiveSceneName(null);
      setScenePendingFields([]);
      pendingSceneRef.current = null;
      if (session != null && session !== "") {
        sceneBySessionRef.current.delete(session);
        void setSessionScene(session, null, false);
      }
      return;
    }
    const scene = scenesRef.current.find((s) => s.reference === reference);
    if (!scene) return;
    const confirmed = opts?.confirmed ?? false;
    const escalation = escalationNeeded(scene, sessionMode(mode, sandbox));
    if (escalation && !confirmed) {
      setSceneEscalation({ reference, kind: "soft", ...escalation });
      return;
    }
    const live = {
      mode: sessionMode(mode, sandbox),
      memoryRead,
      memoryWrite,
      provider,
      model: currentModel,
    };
    const { execution } = scene;
    if (execution?.session_mode) onSessionModeChange(execution.session_mode);
    if (execution?.memory_preset) {
      const preset = MEMORY_PRESET_POLICY[execution.memory_preset];
      onMemoryPolicyChange(preset.read, preset.write);
    }
    const pending = softApplyPending(scene, live);
    setActiveSceneName(reference);
    setScenePendingFields(pending);
    if (session != null && session !== "") {
      sceneBySessionRef.current.set(session, reference);
      void applySceneToSession(session, reference, confirmed).then(
        (outcome) => {
          if (!componentEnabledRef.current("scenes.surface")) return;
          // The core re-checks against the persisted policy; if it disagrees, nothing was
          // applied there — surface the same dialog instead of drifting.
          if (outcome?.escalation) {
            setSceneEscalation({
              reference,
              kind: "soft",
              from: toSessionMode(outcome.escalation.from),
              to: toSessionMode(outcome.escalation.to),
            });
          }
        }
      );
    } else {
      pendingSceneRef.current = reference;
    }
    toast(t("scene.switched", { scene: scene.title }));
  };

  const setAutoSceneChoice = (enabled: boolean) => {
    if (!componentEnabledRef.current("scenes.surface")) return;
    const session = activeSessionRef.current;
    autoSceneRef.current = enabled;
    setAutoScene(enabled);
    if (session != null && session !== "") {
      autoSceneBySessionRef.current.set(session, enabled);
      void setSessionAutoScene(session, enabled);
    }
  };

  /** Full-apply: a fresh session in the active scene, closing the soft-apply gap. */
  const restartInScene = async (confirmed = false) => {
    if (!componentEnabledRef.current("scenes.surface")) return;
    const reference = activeSceneNameRef.current;
    if (reference == null || reference === "") return;
    const plan = await sceneSessionPlan(reference, confirmed);
    if (!componentEnabledRef.current("scenes.surface") || !plan) return;
    if (plan.escalation) {
      setSceneEscalation({
        reference,
        kind: "restart",
        from: toSessionMode(plan.escalation.from),
        to: toSessionMode(plan.escalation.to),
      });
      return;
    }
    pendingSceneRef.current = reference;
    createSession();
    const { params } = plan;
    if (params?.provider != null && params?.provider !== "")
      setProvider(params.provider);
    if (params?.use_worktree === false) setWorktreeBase(null);
    else if (params?.worktree_base) setWorktreeBase(params.worktree_base);
    // The draft reset cleared the posture; re-apply the scene's fields to the new draft.
    applySceneChoice(reference, { confirmed: true });
    pendingSceneRef.current = reference;
  };

  /** The pinned scene reference a pipeline stage's bare scene name resolves to, when installed. */
  const resolveStageScene = (target: string) => {
    return (
      scenesRef.current.find((s) => s.reference === target || s.name === target)
        ?.reference ?? target
    );
  };

  /** Refresh the local scene chip state after the core soft-applied a stage's scene (R9). */
  const syncSessionScene = async (session: string) => {
    if (!componentEnabledRef.current("scenes.surface")) return;
    const state = await getSessionScene(session);
    if (!componentEnabledRef.current("scenes.surface") || !state) return;
    sceneBySessionRef.current.set(session, state.reference);
    setActiveSceneName(state.reference);
    setScenePendingFields([]);
  };

  /**
   * Advance a pipeline instance for the active session (R9). The command re-checks escalation
   * (refuse-and-report); a report raises the shared SceneEscalationDialog, and confirming
   * re-calls with confirm=true.
   */
  const advancePipelineChoice = async (
    instanceId: string,
    toStage: string,
    confirmed = false
  ) => {
    if (!componentEnabledRef.current("scenes.surface")) return;
    const session = activeSessionRef.current;
    const outcome = await advancePipeline(
      instanceId,
      toStage,
      session,
      confirmed
    );
    if (!componentEnabledRef.current("scenes.surface") || !outcome) return;
    const escalation =
      outcome.escalation ?? outcome.applied_scene?.escalation ?? null;
    if (escalation) {
      const stage = pipelineDetail?.stages.find((s) => s.id === toStage);
      setSceneEscalation({
        reference: resolveStageScene(stage?.scene_ref ?? toStage),
        kind: "pipeline",
        from: toSessionMode(escalation.from),
        to: toSessionMode(escalation.to),
        pipeline: { instanceId, toStage },
      });
      return;
    }
    if (session != null && session !== "") await syncSessionScene(session);
    if (!componentEnabledRef.current("scenes.surface")) return;
    const detail = await getPipelineInstance(instanceId);
    if (!componentEnabledRef.current("scenes.surface")) return;
    setPipelineDetail(detail);
    const stage = detail?.stages.find((s) => s.id === toStage);
    toast(t("stage.advanced", { stage: stage?.title ?? toStage }));
  };

  /**
   * Advance into the next stage in a FRESH session (full-apply): the backend records the
   * transition and returns the stage scene's session plan; the created session is bound to the
   * stage via pendingPipelineBindRef once `session_created` arrives.
   */
  const advancePipelineInNewSession = async (
    instanceId: string,
    toStage: string,
    confirmed = false
  ) => {
    if (!componentEnabledRef.current("scenes.surface")) return;
    const outcome = await advancePipeline(instanceId, toStage, null, confirmed);
    if (!componentEnabledRef.current("scenes.surface") || !outcome) return;
    if (outcome.escalation) {
      const stage = pipelineDetail?.stages.find((s) => s.id === toStage);
      setSceneEscalation({
        reference: resolveStageScene(stage?.scene_ref ?? toStage),
        kind: "pipeline_new",
        from: toSessionMode(outcome.escalation.from),
        to: toSessionMode(outcome.escalation.to),
        pipeline: { instanceId, toStage },
      });
      return;
    }
    const detail = await getPipelineInstance(instanceId);
    if (!componentEnabledRef.current("scenes.surface")) return;
    setPipelineDetail(detail);
    const stage = detail?.stages.find((s) => s.id === toStage);
    const reference = resolveStageScene(stage?.scene_ref ?? toStage);
    pendingSceneRef.current = reference;
    pendingPipelineBindRef.current = { instanceId, stageId: toStage };
    createSession();
    const params = outcome.session_plan;
    if (params?.provider != null && params?.provider !== "")
      setProvider(params.provider);
    if (params?.use_worktree === false) setWorktreeBase(null);
    else if (params?.worktree_base) setWorktreeBase(params.worktree_base);
    applySceneChoice(reference, { confirmed: true });
    pendingSceneRef.current = reference;
    toast(t("stage.advancedNew", { stage: stage?.title ?? toStage }));
  };

  /** Start a pipeline in the current project, binding the active session to its entry stage. */
  const startPipelineChoice = async (reference: string) => {
    if (!componentEnabledRef.current("scenes.surface")) return;
    const session = activeSessionRef.current;
    const outcome = await startPipeline(reference, cwd || ".", session);
    if (!componentEnabledRef.current("scenes.surface") || !outcome) return;
    setPipelineDetail(outcome.detail);
    if (session != null && session !== "") await syncSessionScene(session);
    if (!componentEnabledRef.current("scenes.surface")) return;
    // An entry scene looser than the session's posture applied nothing (refuse-and-report);
    // the binding landed, so confirming just soft-applies the entry scene.
    const escalation = outcome.applied_scene?.escalation;
    const entry = outcome.detail.stages.find((s) => s.state === "current");
    if (escalation && entry) {
      setSceneEscalation({
        reference: resolveStageScene(entry.scene_ref),
        kind: "soft",
        from: toSessionMode(escalation.from),
        to: toSessionMode(escalation.to),
      });
    }
    const title =
      pipelines.find((p) => p.reference === reference)?.title ?? reference;
    toast(t("stage.started", { pipeline: title }));
  };

  /**
   * Delegate an issue into a scene (R12): a fresh draft fully applied to that scene, opened with
   * the issue as a provenance-carrying block. Mirrors `restartInScene` rather than reusing it —
   * that flow is bound to the *active* scene and has no post-create insert seam.
   */
  const onDelegateIssue = async (issue: Issue, sceneReference: string) => {
    if (!componentEnabledRef.current("scenes.surface")) return;
    const scene = scenesRef.current.find((s) => s.reference === sceneReference);
    if (!scene) return;
    const ctx = await issueContext(issue);
    if (!componentEnabledRef.current("scenes.surface")) return;
    const plan = await sceneSessionPlan(sceneReference, false);
    if (!componentEnabledRef.current("scenes.surface") || !plan) return;
    if (plan.escalation) {
      // Delegation never loosens the sandbox silently: same chokepoint dialog, same rule.
      // Confirming soft-applies the scene; delegation itself stays a deliberate re-run.
      setSceneEscalation({
        reference: sceneReference,
        kind: "soft",
        from: toSessionMode(plan.escalation.from),
        to: toSessionMode(plan.escalation.to),
      });
      return;
    }
    // Accountability trail: record the delegation now (session id lands on session_created).
    const delegationId = await recordIssueDelegation(
      issue.source,
      issue.id,
      issue.title,
      sceneReference,
      scene.title
    );
    if (!componentEnabledRef.current("scenes.surface")) return;
    pendingSceneRef.current = sceneReference;
    pendingIssueInsertRef.current = {
      issue,
      context: ctx,
      delegatedScene: scene.title,
    };
    if (delegationId !== null) pendingDelegationRef.current = delegationId;
    createSession();
    const { params } = plan;
    if (params?.provider != null && params?.provider !== "")
      setProvider(params.provider);
    if (params?.use_worktree === false) setWorktreeBase(null);
    else if (params?.worktree_base) setWorktreeBase(params.worktree_base);
    applySceneChoice(sceneReference, { confirmed: true });
    pendingSceneRef.current = sceneReference;
    // Consume after createSession's synchronous reset settles — the same tick ordering its own
    // focus timeout uses; the mounted editor survives New, so the ref is live by then.
    setTimeout(() => {
      const pending = pendingIssueInsertRef.current;
      pendingIssueInsertRef.current = null;
      if (pending) {
        insertIssueRef.current?.(
          pending.issue,
          pending.context,
          pending.delegatedScene
        );
      }
    }, 0);
    setShowIssues(false);
    toast(
      t("issueDeleg.toast", { id: issue.id, scene: scene.title }),
      "success"
    );
    // Best-effort attribution on the tracker, fire-and-forget. GitHub only: Linear's comment
    // call needs the caller-held API token `listLinearIssues` uses, which this surface does
    // not hold — skipped rather than failing.
    if (issue.source === "github") {
      void commentIssue(
        cwd || ".",
        issue.source,
        issue.id,
        t("issueDeleg.commentBody", { scene: scene.title })
      ).then((url) => {
        if (url != null && url !== "" && delegationId !== null)
          void setIssueDelegationComment(delegationId, url);
      });
    }
  };

  const dispatchAction = (action: string) => {
    switch (action) {
      case "run": {
        void run(undefined, undefined, focusedPaneRef.current, activeAppshots);
        break;
      }
      case "new_session": {
        setShowTaskBoard(false);
        setShowPluginManager(false);
        setShowAutomations(false);
        createTaskDraft();
        break;
      }
      case "cancel": {
        if (
          activeSessionRef.current != null &&
          activeSessionRef.current !== "" &&
          running
        )
          void cancelTurn(activeSessionRef.current);
        else toast(t("toast.nothingRunning"));
        break;
      }
      case "toggle_terminal": {
        toggleDock("terminal");
        break;
      }
      case "toggle_browser": {
        toggleDock("browser");
        break;
      }
      case "toggle_git": {
        toggleDock("git");
        break;
      }
      case "close_panel": {
        manualDockTab(null);
        break;
      }
      case "split_pane_right": {
        splitPaneById(focusedPaneRef.current, "right");
        break;
      }
      case "split_pane_down": {
        splitPaneById(focusedPaneRef.current, "bottom");
        break;
      }
      case "toggle_side_panel": {
        toggleSidePanel();
        break;
      }
      case "open_skill_picker": {
        openSkillPickerRef.current?.();
        break;
      }
      case "focus_editor": {
        focusEditorRef.current?.();
        break;
      }
      case "toggle_doc_mode": {
        toggleDocMode(!docMode);
        break;
      }
      case "open_settings": {
        setShowTaskBoard(false);
        setShowPluginManager(false);
        setShowAutomations(false);
        setShowPullRequests(false);
        setShowDocker(false);
        setShowFeishu(false);
        setSettingsInitialTab("general");
        setShowSettings(true);
        break;
      }
      case "open_command_palette": {
        setShowPalette(true);
        break;
      }
      case "open_source_control": {
        openSourceControl();
        break;
      }
      case "open_market": {
        openPluginManager();
        break;
      }
      case "open_usage": {
        if (!componentEnabled("usage.settings")) {
          toast("Usage is disabled in Plugins.", "info");
          break;
        }
        setShowTaskBoard(false);
        setShowPluginManager(false);
        setShowAutomations(false);
        setShowPullRequests(false);
        setShowDocker(false);
        setShowFeishu(false);
        setSettingsInitialTab("usage");
        setShowSettings(true);
        break;
      }
      case "open_files": {
        if (componentEnabled("files.surface")) setShowFiles(true);
        else toast("Files are disabled in Plugins.", "info");
        break;
      }
      case "open_finder": {
        void openWorkingDirectory("finder");
        break;
      }
      case "search_workspace": {
        if (componentEnabled("search.modal")) setShowWorkspaceSearch(true);
        else toast("Workspace search is disabled in Plugins.", "info");
        break;
      }
      case "open_issues": {
        if (componentEnabled("issues.modal")) setShowIssues(true);
        else toast("Issues are disabled in Plugins.", "info");
        break;
      }
      case "prev_session": {
        stepSession(-1);
        break;
      }
      case "next_session": {
        stepSession(1);
        break;
      }
      case "cycle_permission_mode": {
        if (policyChangeDisabled) break;
        const next = nextSessionMode(sessionMode(mode, sandbox));
        if (onSessionModeChange(next)) {
          toast(`Mode: ${t(`mode.${next}`)}`);
        }
        break;
      }
      case "refresh_git": {
        refreshGit();
        break;
      }
      case "cycle_scene": {
        if (!componentEnabled("scenes.surface")) {
          toast("Scenes are disabled in Plugins.", "info");
          break;
        }
        const next = nextSceneInRing(
          [],
          scenesRef.current,
          activeSceneNameRef.current
        );
        if (next != null && next !== "") applySceneChoice(next);
        break;
      }
      case "open_scene_picker": {
        if (componentEnabled("scenes.surface")) setShowScenePicker(true);
        else toast("Scenes are disabled in Plugins.", "info");
        break;
      }
      case "open_mission_control": {
        setShowMissionControl(true);
        break;
      }
      default: {
        // A binding pointing at an action this frontend doesn't implement.
        toast(`No handler for "${action}".`, "error");
        break;
      }
    }
  };

  // Hints come from the live keymap, so a rebind is reflected everywhere without touching labels.
  const hint = (action: string) => keyHint(bindings, action);
  const effectiveBindings = [...bindings, ...projectActionBindings(scripts)];

  const paletteCommands: Command[] = [
    {
      id: "run",
      label: "Run prompt",
      hint: hint("run"),
      run: () =>
        void run(undefined, undefined, focusedPaneRef.current, activeAppshots),
    },
    {
      id: "new",
      label: "New task",
      hint: hint("new_session"),
      run: () => {
        setShowTaskBoard(false);
        setShowPluginManager(false);
        setShowAutomations(false);
        createTaskDraft();
      },
    },
    {
      id: "sc",
      label: "Source control",
      hint: hint("open_source_control"),
      run: openSourceControl,
    },
    {
      id: "pull-requests",
      label: t("pullRequests.title"),
      run: openPullRequests,
    },
    {
      id: "checkpoint",
      label: "Checkpoint now",
      run: () => void doCheckpoint(),
    },
    {
      id: "market",
      label: "Open Plugin Hub",
      hint: hint("open_market"),
      run: openPluginManager,
    },
    { id: "automations", label: t("automations.title"), run: openAutomations },
    { id: "taskboard", label: t("taskboard.open"), run: openTaskBoard },
    {
      id: "issues",
      label: "GitHub / Linear issues",
      hint: hint("open_issues"),
      run: () => setShowIssues(true),
    },
    {
      id: "files",
      label: "Browse workspace files",
      hint: hint("open_files"),
      run: () => setShowFiles(true),
    },
    {
      id: "finder",
      label: t("action.open_finder"),
      hint: hint("open_finder"),
      run: () => void openWorkingDirectory("finder"),
    },
    {
      id: "search",
      label: "Search workspace contents",
      hint: hint("search_workspace"),
      run: () => setShowWorkspaceSearch(true),
    },
    {
      id: "usage",
      category: "setting" as const,
      label: "Usage (5h / week / month)",
      hint: hint("open_usage"),
      run: () => {
        setShowTaskBoard(false);
        setShowPluginManager(false);
        setShowAutomations(false);
        setShowPullRequests(false);
        setShowDocker(false);
        setShowFeishu(false);
        setSettingsInitialTab("usage");
        setShowSettings(true);
      },
    },
    {
      id: "preview",
      label: "Preview compiled prompt",
      run: () => void doPreview(),
    },
    {
      id: "docmode",
      label: docMode
        ? "Collapse the document"
        : "Expand the document to full height",
      hint: hint("toggle_doc_mode"),
      run: () => toggleDocMode(!docMode),
    },
    {
      id: "skills",
      label: "Insert a skill",
      hint: hint("open_skill_picker"),
      run: () => openSkillPickerRef.current?.(),
    },
    {
      id: "template-from-last",
      label: t("templateFrom.palette"),
      // No-op with no history: the most recent user turn's prompt is the source.
      run: () => {
        const last = [...turns]
          .toReversed()
          .find((turn) => turn.prompt.trim().length > 0);
        if (last) setTemplateDraft(last.prompt);
      },
    },
    {
      id: "rail",
      label: displayedRailCollapsed
        ? "Expand the sidebar"
        : "Collapse the sidebar",
      run: toggleDisplayedRail,
    },
    {
      id: "remote",
      label: t("rail.deviceConnections"),
      run: () => setShowRemote(true),
    },
    {
      id: "settings",
      category: "setting" as const,
      label: "Open settings",
      hint: hint("open_settings"),
      run: () => {
        setShowPullRequests(false);
        setShowDocker(false);
        setShowFeishu(false);
        setSettingsInitialTab("general");
        setShowSettings(true);
      },
    },
    {
      id: "terminal",
      label: "Toggle terminal",
      hint: hint("toggle_terminal"),
      run: () => toggleDock("terminal"),
    },
    {
      id: "browser",
      label: "Toggle browser",
      hint: hint("toggle_browser"),
      run: () => toggleDock("browser"),
    },
    {
      id: "filespanel",
      label: "Toggle file tree",
      run: () => toggleDock("files"),
    },
    {
      id: "gitpanel",
      label: "Toggle git panel",
      hint: hint("toggle_git"),
      run: () => toggleDock("git"),
    },
    {
      id: "git",
      label: "Refresh git status",
      hint: hint("refresh_git"),
      run: refreshGit,
    },
    {
      id: "perm",
      label: "Cycle approval mode",
      hint: hint("cycle_permission_mode"),
      run: () => dispatchAction("cycle_permission_mode"),
    },
    {
      id: "scene",
      label: t("scene.pickerTitle"),
      hint: hint("cycle_scene"),
      run: () => setShowScenePicker(true),
    },
    {
      id: "scene-studio",
      label: t("sceneEditor.manage"),
      run: () => {
        setSceneEditorRequest(null);
        setShowSceneStudio(true);
      },
    },
    {
      id: "mission",
      label: t("action.open_mission_control"),
      hint: hint("open_mission_control"),
      run: () => setShowMissionControl(true),
    },
    ...scenes.map((s) => ({
      id: `scene-${s.reference}`,
      label: `${t("scene.chip")}: ${s.title}`,
      run: () => applySceneChoice(s.reference),
    })),
    ...pipelines.map((p) => ({
      id: `pipeline-${p.reference}`,
      label: t("stage.startPipeline", { pipeline: p.title }),
      run: () => void startPipelineChoice(p.reference),
    })),
    ...scripts.map((s) => ({
      id: `script-${s.id}`,
      label:
        s.kind === "prompt"
          ? `Send prompt: ${s.name || s.id}`
          : `Run script: ${s.name || s.id}`,
      hint: s.kind === "prompt" ? s.prompt : s.command,
      run: () => runProjectAction(s),
    })),
    ...sessions.map((s) => ({
      id: `sess-${s.id}`,
      identity: `session-${s.id}`,
      category: "session" as const,
      label: s.title,
      hint: displayProvider(s.provider),
      detail: `${s.project_path ?? s.cwd ?? "—"} · ${new Date(s.created_at).toLocaleString(locale)}`,
      run: () => void selectSession(s.id),
    })),
  ].filter((command) => {
    if (["sc", "checkpoint", "gitpanel", "git"].includes(command.id)) {
      return componentEnabled("git.surface");
    }
    if (command.id === "automations")
      return componentEnabled("automation.page");
    if (command.id === "issues") return componentEnabled("issues.modal");
    if (["files", "filespanel"].includes(command.id))
      return componentEnabled("files.surface");
    if (command.id === "search") return componentEnabled("search.modal");
    if (command.id === "usage") return componentEnabled("usage.settings");
    if (command.id === "remote") return componentEnabled("remote.modal");
    if (command.id === "terminal") return componentEnabled("terminal.dock");
    if (command.id === "browser") return componentEnabled("browser.dock");
    if (
      command.id === "scene" ||
      command.id === "scene-studio" ||
      command.id.startsWith("scene-") ||
      command.id.startsWith("pipeline-")
    ) {
      return componentEnabled("scenes.surface");
    }
    return true;
  });

  useEffect(() => {
    canvasFeatureState()
      .then((state) => setCanvasFeature(state))
      .catch(() => {
        setCanvasFeature({
          feature: "CODETWO_CANVAS_INPUT_V1",
          enabled: false,
          status: "not production-enabled",
        });
      });
    getKeymap()
      .then(setBindings)
      .catch(() => {
        /* empty */
      });
    // Open on the project used last. Failing that, register the directory the app started in, so
    // the picker is never empty and the first session has somewhere real to run.
    setCallProjectPath(null);
    listProjects()
      .then(async (list) => {
        setProjects(list);
        if (list.length > 0) {
          // The list is in a fixed order, so "used last" is a property of the rows, not their
          // position — read it off `last_opened_at` rather than taking the first one.
          const last = list.reduce((a, b) =>
            b.last_opened_at > a.last_opened_at ? b : a
          );
          activeProjectRef.current = last.path;
          setCallProjectPath(normalizePluginProjectPath(last.path));
          setActiveProject(last.path);
          setCwd(last.path);
          if (last.default_provider != null && last.default_provider !== "")
            setProvider(last.default_provider);
          setCurrentModel(
            last.default_provider != null && last.default_provider !== ""
              ? (last.default_model ?? null)
              : null
          );
          setWorktreeBase(
            projectSwitchWorktreeBaseline(last.default_worktree_mode)
          );
          setProjectBootstrapComplete(true);
          return;
        }
        const here = await defaultCwd();
        setCwd(here);
        const resolved = await addProject(here).catch(() => null);
        if (resolved != null && resolved !== "") {
          activeProjectRef.current = resolved;
          setCallProjectPath(normalizePluginProjectPath(resolved));
          setActiveProject(resolved);
          listProjects()
            .then(setProjects)
            .catch(() => {
              /* empty */
            });
        }
        setProjectBootstrapComplete(true);
      })
      .catch(() => {
        setProjectBootstrapComplete(true);
        defaultCwd()
          .then(setCwd)
          .catch(() => {
            /* empty */
          });
      });
    // The app opens on a blank page, so put the caret in it. Deferred one tick: the editor installs
    // its focus handle in its own mount effect.
    setTimeout(() => focusEditorRef.current?.(), 0);
  }, []);

  useEffect(() => {
    refreshGit();
    if (showSourceControl) refreshCheckpoints();
  }, [refreshGit, refreshCheckpoints, activeSession, showSourceControl]);

  useEffect(() => {
    listProjectScripts(cwd || ".")
      .then(setScripts)
      .catch(() => setScripts([]));
  }, [cwd]);

  useEffect(() => {
    const request = (worktreeOptionsRequestRef.current += 1);
    const source = (activeProject ?? cwd) || ".";
    setWorktreeOptions([]);
    setWorktreeOptionsLoading(true);
    void listWorktreeBaselines(source)
      .then((options) => {
        if (request === worktreeOptionsRequestRef.current)
          setWorktreeOptions(options);
      })
      .catch(() => {
        if (request === worktreeOptionsRequestRef.current)
          setWorktreeOptions([]);
      })
      .finally(() => {
        if (request === worktreeOptionsRequestRef.current)
          setWorktreeOptionsLoading(false);
      });
  }, [activeProject, cwd]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (capturing != null && capturing !== "") {
        if (isModifierOnly(e)) return;
        e.preventDefault();
        // Escape aborts the capture rather than binding Escape to this action.
        if (e.key === "Escape") {
          setCapturing(null);
          return;
        }
        void setKeymap(capturing, comboFromEvent(e))
          .then(async () => await getKeymap().then(setBindings))
          .catch((error: unknown) =>
            toast(`Could not save shortcut: ${error}`, "error")
          );
        setCapturing(null);
        return;
      }
      const action = actionForEvent(e, effectiveBindings);
      if (action == null || action === "") return;
      // Escape is also how dialogs and the suggestion menu close; let those win when one is open.
      if (
        e.key === "Escape" &&
        document.querySelector('[role="dialog"],.bn-suggestion-menu')
      )
        return;
      e.preventDefault();
      if (action.startsWith("project_action:")) {
        const script = scripts.find(
          (candidate) => `project_action:${candidate.id}` === action
        );
        if (script) runProjectAction(script);
        return;
      }
      dispatchAction(action);
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [
    capturing,
    dispatchAction,
    effectiveBindings,
    runProjectAction,
    scripts,
    toast,
  ]);

  // Restore one shortcut to its shipped default.
  const resetBinding = (action: string) => {
    const def = DEFAULT_KEYMAP.find(([a]) => a === action);
    if (!def) return;
    void setKeymap(action, def[1])
      .then(async () => await getKeymap().then(setBindings))
      .catch((error: unknown) =>
        toast(`Could not reset shortcut: ${error}`, "error")
      );
  };

  // Restore every shortcut — settings' "Restore defaults" on the keybindings tab.
  const resetAllBindings = () => {
    void Promise.all(
      DEFAULT_KEYMAP.map(async ([a, key]) => await setKeymap(a, key))
    )
      .then(async () => await getKeymap().then(setBindings))
      .catch((error: unknown) =>
        toast(`Could not reset shortcuts: ${error}`, "error")
      );
  };

  // Discarding a worktree deletes uncommitted work, so it confirms natively first. Success
  // feedback arrives through the broadcast `worktree_discarded` event, not from this call.
  const discardWorktreeForSession = async (session: SessionInfo) => {
    const path = session.worktree_path;
    if (path == null || path === "" || session.worktree_discarded === true)
      return;
    if (!(await confirmNative(t("worktree.discardConfirm", { path })))) return;
    try {
      await discardSessionWorktree(session.id);
      void refreshSessions();
    } catch (error) {
      toast(t("worktree.discardFailed", { error: String(error) }), "error");
    }
  };

  const changeConversationProvider = (
    sessionId: string | null,
    next: string,
    nextModel: string | null = null
  ) => {
    providerPinned.current = true;
    if (sessionId === null) {
      setProvider(next);
      setModels(
        providers.find((candidate) => candidate.id === next)?.models ?? []
      );
      setCurrentModel(nextModel);
      setDefaultModel(null);
      setConfigOptions([]);
      return;
    }
    const stored = [...sessions, ...archivedSessions].find(
      (candidate) => candidate.id === sessionId
    );
    if (stored && providerLabel(stored.provider) === next) return;
    if (runningSessionsRef.current.has(sessionId)) {
      toast(t("toast.providerSwitchBusy"), "error");
      return;
    }
    if (providerSwitchRequestsRef.current.has(sessionId)) return;
    providerSwitchRequestsRef.current.add(sessionId);
    setProviderSwitchingSessions((current) => new Set(current).add(sessionId));
    void switchProvider(sessionId, next, nextModel)
      .catch((error: unknown) => {
        toast(
          t("toast.providerSwitchFailed", { error: String(error) }),
          "error"
        );
      })
      .finally(() => {
        providerSwitchRequestsRef.current.delete(sessionId);
        setProviderSwitchingSessions((current) => {
          if (!current.has(sessionId)) return current;
          const remaining = new Set(current);
          remaining.delete(sessionId);
          return remaining;
        });
      });
  };

  const sessionConfig: SessionConfig = {
    providers,
    providersStatus,
    provider,
    onProvider: (next) =>
      changeConversationProvider(activeSessionRef.current, next, null),
    onProviderModel: (nextProvider, nextModel) =>
      changeConversationProvider(
        activeSessionRef.current,
        nextProvider,
        nextModel
      ),
    providerSwitching:
      activeSession !== null && providerSwitchingSessions.has(activeSession),
    providerChangeDisabled:
      activeSession !== null &&
      (runningSessions.has(activeSession) ||
        providerSwitchingSessions.has(activeSession)),
    onReloadProviders: () => {
      void refreshProviders().catch(() => {
        /* empty */
      });
    },
    mode,
    sandbox,
    modeChangeDisabled: policyChangeDisabled,
    onSessionMode: onSessionModeChange,
    worktreeBase,
    activeWorktreeBaseline,
    activeWorktreeUnknown,
    worktreeOptions,
    worktreeOptionsLoading,
    onWorktreeBase: setWorktreeBase,

    memoryRead,
    memoryWrite,
    memoryEnabled: memorySettingsEnabled,
    onMemoryPolicy: onMemoryPolicyChange,
    hasSession: activeSession !== null,
    scenesEnabled: scenesSurfaceEnabled,
    scenes: scenesSurfaceEnabled ? scenes : [],
    activeScene: scenesSurfaceEnabled
      ? (scenes.find((s) => s.reference === activeSceneName) ?? null)
      : null,
    autoScene: scenesSurfaceEnabled && autoScene,
    onAutoScene: setAutoSceneChoice,
    onScene: (reference, strength) => {
      if (strength === "full") {
        if (reference !== null) {
          setActiveSceneName(reference);
          void restartInScene();
        }
      } else {
        applySceneChoice(reference);
      }
    },
    onManageScenes: () => {
      setSceneEditorRequest(null);
      setShowSceneStudio(true);
    },
    sceneCustomized:
      scenesSurfaceEnabled &&
      (() => {
        const scene = scenes.find((s) => s.reference === activeSceneName);
        if (!scene) return false;
        return sceneCustomized(scene, {
          mode: sessionMode(mode, sandbox),
          memoryRead,
          memoryWrite,
          provider,
          model: currentModel,
        });
      })(),
    scenePendingFields: scenesSurfaceEnabled ? scenePendingFields : [],
    onRestartInScene: () => void restartInScene(),
  };

  const handleSceneSaved = (saved: SceneInfo) => {
    if (!componentEnabledRef.current("scenes.surface")) return;
    const previous =
      sceneEditorRequest?.kind === "edit"
        ? sceneEditorRequest.scene.reference
        : null;
    if (
      previous != null &&
      previous !== "" &&
      previous === activeSceneNameRef.current &&
      previous !== saved.reference
    ) {
      activeSceneNameRef.current = saved.reference;
      setActiveSceneName(saved.reference);
      if (activeSession != null && activeSession !== "") {
        sceneBySessionRef.current.set(activeSession, saved.reference);
        void setSessionScene(activeSession, saved.reference, false);
      }
    }
    void refreshScenes();
    setSceneEditorRequest(null);
  };

  const handleSceneDeleted = (reference: string) => {
    if (!componentEnabledRef.current("scenes.surface")) return;
    if (reference === activeSceneNameRef.current) {
      activeSceneNameRef.current = null;
      setActiveSceneName(null);
      setScenePendingFields([]);
      if (activeSession != null && activeSession !== "") {
        sceneBySessionRef.current.delete(activeSession);
        void setSessionScene(activeSession, null, false);
      }
    }
    void refreshScenes();
    setSceneEditorRequest(null);
  };

  return (
    <div className="app-shell text-foreground flex h-screen flex-col overflow-hidden">
      <DesktopPetBridge
        animation={petAnimation}
        bubble={petConversationBubble}
      />
      {!showSettings && !(showSceneStudio && scenesSurfaceEnabled) && (
        <div
          data-workspace-titlebar
          className="workspace-titlebar bg-sidebar electrobun-webkit-app-region-drag flex min-w-0 shrink-0 items-center"
        >
          <div
            data-workspace-navigation
            className="flex shrink-0 items-center justify-end gap-1 pr-2"
            style={{ width: Math.max(168, APP_NAV_WIDTH + railInlineWidth) }}
          >
            <IconAction
              icon={Search}
              label={t("rail.searchChats")}
              hint={hint("open_command_palette")}
              onClick={() => setShowPalette(true)}
            />
            <IconAction
              icon={PanelLeft}
              label={t(
                displayedRailCollapsed ? "rail.expand" : "rail.collapse"
              )}
              onClick={toggleDisplayedRail}
            />
          </div>
          <div className="electrobun-webkit-app-region-drag flex min-w-0 flex-1 justify-center px-4">
            <span className="electrobun-webkit-app-region-drag text-metadata text-muted-foreground max-w-lg truncate font-medium">
              {showTaskBoard ||
              showPluginManager ||
              showAutomations ||
              showPullRequests ||
              showDocker ||
              showFeishu
                ? null
                : activeTitle}
            </span>
          </div>
          <div
            ref={setDockTitlebarHost}
            data-dock-titlebar-host
            className="contents"
          />
        </div>
      )}
      {/* Settings takes the whole window — its own nav rail replaces the session rail, and the
          Back row at its foot is the way home. */}
      {showSettings ? (
        <SettingsPage
          onSelectProject={selectProject}
          onOpenDevices={
            componentEnabled("remote.modal")
              ? () => setShowRemote(true)
              : undefined
          }
          sidebarWidth={railWidth}
          initialTab={settingsInitialTab}
          bindings={bindings}
          capturing={capturing}
          onCapture={setCapturing}
          onReset={resetBinding}
          onResetAll={resetAllBindings}
          providers={providers}
          provider={provider}
          onReloadProviders={refreshProviderUpdates}
          projectPath={activeProject ?? cwd}
          project={
            projects.find((project) => project.path === activeProject) ?? null
          }
          projects={projects}
          onProjectWorktreeMode={updateProjectWorktreeMode}
          onProjectRename={updateProjectName}
          onProjectIcon={updateProjectIcon}
          onProjectAgentDefaults={updateProjectAgentDefaults}
          onProjectRemove={removeProjectEntry}
          projectActionsCount={scripts.length}
          onAddProjectAction={() => setShowActionDialog(true)}
          onOpenSession={(id) => {
            setShowSettings(false);
            void selectSession(id);
          }}
          onSessionsImported={refreshSessions}
          memoryEnabled={memorySettingsEnabled}
          deviceSyncEnabled={deviceSyncSettingsEnabled}
          onClose={() => {
            setShowSettings(false);
            setCapturing(null);
          }}
        />
      ) : showSceneStudio && scenesSurfaceEnabled ? (
        <SceneStudio
          scenes={scenes}
          active={
            scenes.find((scene) => scene.reference === activeSceneName) ?? null
          }
          request={sceneEditorRequest}
          providers={providers}
          skills={activeSkills}
          cwd={cwd || "."}
          onRequest={setSceneEditorRequest}
          onScene={(reference) => applySceneChoice(reference)}
          onSaved={handleSceneSaved}
          onDeleted={handleSceneDeleted}
          onClose={() => {
            setSceneEditorRequest(null);
            setShowSceneStudio(false);
          }}
        />
      ) : (
        // page-in makes the return from settings (which remounts this whole subtree) a transition
        // rather than a cut, and doubles as the app's own opening animation.
        <div className="workspace-shell bg-sidebar animate-page-in relative flex min-h-0 flex-1">
          {/* ---------------- sessions rail ---------------- */}
          {railOverlay && narrowRailOpen && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label={t("rail.collapse")}
              className="absolute inset-0 z-40 size-auto rounded-none bg-black/35 hover:bg-black/35"
              onClick={() => setNarrowRailOpen(false)}
            />
          )}
          <SessionRail
            onOpenTasks={() => {
              setShowTaskBoard(false);
              setShowPluginManager(false);
              setShowAutomations(false);
              setShowPullRequests(false);
              setShowDocker(false);
              setShowFeishu(false);
              if (railOverlay) setNarrowRailOpen(false);
            }}
            projects={projects}
            sessions={sessions}
            archivedSessions={archivedSessions}
            previews={previews}
            activeSession={activeSession}
            runningSessions={runningSessions}
            onSelect={(id) => {
              setShowTaskBoard(false);
              setShowPluginManager(false);
              setShowAutomations(false);
              setShowPullRequests(false);
              setShowDocker(false);
              setShowFeishu(false);
              void selectSession(id);
              if (railOverlay) setNarrowRailOpen(false);
            }}
            onNew={() => {
              setShowTaskBoard(false);
              setShowPluginManager(false);
              setShowAutomations(false);
              setShowPullRequests(false);
              setShowDocker(false);
              setShowFeishu(false);
              createTaskDraft();
              if (railOverlay) setNarrowRailOpen(false);
            }}
            quickChatOpen={quickChatOpen}
            onToggleQuickChat={() => {
              setQuickChatOpen((current) => !current);
              if (narrowLayout) setNarrowRailOpen(false);
            }}
            onRename={(id, title) =>
              void renameSession(id, title).then(refreshSessions)
            }
            onPin={(id, pinned) =>
              void pinSession(id, pinned).then(refreshSessions)
            }
            onArchive={async (id, archived) => {
              await archiveSession(id, archived);
              await refreshSessions();
            }}
            onDiscardWorktree={(s) => void discardWorktreeForSession(s)}
            onRemoveProject={(path) => void requestProjectRemoval(path)}
            onAddProject={() => void addProjectFolder()}
            displayProvider={displayProvider}
            onOpenMarket={() => {
              setShowTaskBoard(false);
              openPluginManager();
            }}
            onOpenAutomations={openAutomations}
            onOpenAssistant={() => setShowMissionControl(true)}
            deviceConnectionsAvailable={componentEnabled("remote.modal")}
            deviceConnectionsOpen={showRemote}
            onOpenDeviceConnections={() => {
              setShowRemote(true);
              if (railOverlay) setNarrowRailOpen(false);
            }}
            width={railWidth}
            onWidth={setRailWidth}
            newHint={hint("new_session")}
            onOpenSettings={() => {
              setShowTaskBoard(false);
              setShowPluginManager(false);
              setShowAutomations(false);
              setShowPullRequests(false);
              setShowDocker(false);
              setShowFeishu(false);
              setSettingsInitialTab("general");
              if (railOverlay) setNarrowRailOpen(false);
              setShowSettings(true);
            }}
            collapsed={displayedRailCollapsed}
            overlay={railOverlay}
            taskBoardOpen={showTaskBoard}
            onOpenTaskBoard={() => {
              if (showTaskBoard) setShowTaskBoard(false);
              else openTaskBoard();
              if (railOverlay) setNarrowRailOpen(false);
            }}
            pullRequestsOpen={showPullRequests}
            onOpenPullRequests={() => {
              if (showPullRequests) setShowPullRequests(false);
              else openPullRequests();
              if (railOverlay) setNarrowRailOpen(false);
            }}
            automationsOpen={showAutomations}
            pluginManagerOpen={showPluginManager}
            dockerAvailable={dockerPlugin !== null}
            dockerOpen={showDocker}
            onOpenDocker={() => {
              if (showDocker) setShowDocker(false);
              else openDocker();
              if (railOverlay) setNarrowRailOpen(false);
            }}
            quickQuota={railQuickQuota}
            quickQuotaLoading={quickQuotaLoading}
            quickQuotaProviderName={quickQuotaProviderName}
            onOpenUsage={() => {
              setShowTaskBoard(false);
              setShowPluginManager(false);
              setShowAutomations(false);
              setShowPullRequests(false);
              setShowDocker(false);
              setShowFeishu(false);
              setSettingsInitialTab("usage");
              if (railOverlay) setNarrowRailOpen(false);
              setShowSettings(true);
            }}
            pluginActions={
              <PluginUiSlot
                slot="rail.features"
                contributions={pluginUiActions["rail.features"]}
                onInvoke={async (contribution) => {
                  await invokePluginAction(contribution);
                  if (railOverlay) setNarrowRailOpen(false);
                }}
              />
            }
            resourceSections={
              collaborationConnector ? <div ref={setFeishuRailHost} /> : null
            }
          />

          <Suspense fallback={<PageLoadingFallback />}>
            {showDocker && dockerPlugin ? (
              <DockerPage
                enabled={dockerPluginReady}
                callCommand={callDocker}
                onOpenPluginManager={openPluginManager}
              />
            ) : null}

            {collaborationConnector ? (
              <FeishuWorkspacePage
                enabled
                detailVisible={showFeishu}
                sessionId={activeSession}
                callCommand={callFeishu}
                subscribeEvents={subscribeFeishuEvents}
                onHandoff={async (prompt) => {
                  await run([{ type: "text", text: prompt }]);
                }}
                onOpenPluginManager={openFeishuPluginSettings}
                navigationHost={feishuRailHost}
                settingsHost={feishuSettingsHost}
                onSelectResource={openFeishu}
              />
            ) : null}

            {showPullRequests && (
              <PullRequestsPage
                onChat={chatAboutPullRequest}
                tasks={pullRequestTasks}
                activeTaskId={activeBoardTask?.id ?? null}
                onLinkTask={linkPullRequestToTask}
                onUnlinkTask={unlinkPullRequestFromTask}
                onOpenTask={() => openTaskBoard()}
              />
            )}

            {showAutomations &&
              (componentEnabled("automation.page") ? (
                <AutomationsPage
                  projects={projects}
                  providers={providers}
                  defaultProject={(activeProject ?? cwd) || "."}
                  defaultProvider={provider}
                  onAddProject={() => void addProjectFolder()}
                  onOpenSession={(session) => {
                    setShowAutomations(false);
                    void selectSession(session);
                  }}
                />
              ) : null)}

            {showTaskBoard && (
              <TaskBoardPage
                sessions={taskBoardSessions}
                onOpenSession={(id) => {
                  setShowTaskBoard(false);
                  void selectSession(id);
                }}
                onAskSession={(id, prompt) => {
                  const paneId =
                    paneBoundToSession(paneContentsRef.current, id) ??
                    focusedPaneRef.current;
                  setShowTaskBoard(false);
                  void continueTaskBoardPrompt({
                    target: { paneId, sessionId: id },
                    prompt,
                    selectSession: async (sessionId, targetPaneId) =>
                      await selectSession(sessionId, targetPaneId),
                    isTargetActive: () =>
                      focusedPaneRef.current === paneId &&
                      paneContentsRef.current[paneId]?.sessionId === id,
                    openDocumentMode: () => setDocMode(true),
                    insertMarkdown: async (markdown, mode) =>
                      await (paneEditorRefsFor(
                        paneId
                      ).insertMarkdownRef.current?.(markdown, mode) ??
                        Promise.resolve()),
                    focusEditor: () =>
                      paneEditorRefsFor(paneId).focusRef.current?.(),
                  });
                }}
                onStartTask={startBoardTask}
              />
            )}

            {showPluginManager && (
              <PluginManagerPage
                plugins={localizedPluginManagerModel.plugins}
                components={localizedPluginManagerModel.components}
                marketplaceItems={localizedPluginManagerModel.marketplaceItems}
                marketplaceSources={
                  localizedPluginManagerModel.marketplaceSources
                }
                labels={pluginManagerLabels}
                scope={pluginManagerScope}
                projects={pluginManagerProjects}
                initialPluginId={pluginManagerInitialPluginId}
                pluginDetailsExtensions={issueConnectors.map((connector) => ({
                  pluginId: `bundle:${connector.plugin_id}`,
                  content: (
                    <IssueConnectorSettings
                      key={`${connector.plugin_id}:${connector.connector_id}`}
                      connector={connector}
                      onOpen={() => setIssueConnector(connector)}
                    />
                  ),
                }))}
                pluginDetailsExtension={
                  collaborationConnector
                    ? {
                        pluginId: `bundle:${collaborationConnector.pluginId}`,
                        content: <div ref={setFeishuSettingsHost} />,
                      }
                    : null
                }
                recovery={
                  (selectedManagedCatalog ?? managedUserCatalog)?.recovery
                }
                onScopeChange={(scope) => {
                  const normalized =
                    scope.kind === "user"
                      ? scope
                      : {
                          kind: "project" as const,
                          projectPath: normalizePluginProjectPath(
                            scope.projectPath
                          ),
                        };
                  setPluginManagerScope(normalized);
                  void loadManagedCatalog(normalized).catch(
                    (error: unknown) => {
                      toast(
                        t("pluginManager.scopeLoadFailed", {
                          error: String(error),
                        }),
                        "error"
                      );
                    }
                  );
                }}
                onPlanChange={planManagerChange}
                onApplyChange={applyManagerChange}
                onSaveConfig={saveManagerConfig}
                onResetPlugin={async (pluginId, scope) => {
                  await resetManagedPlugin(
                    pluginId,
                    toManagedPluginScope(scope)
                  );
                  await refreshPluginManagerData(scope);
                  toast(pluginManagerLabels.settingsReset, "success");
                }}
                onInstallMarketplaceItem={async ({ itemId, scope }) => {
                  if (scope.kind !== "user") {
                    throw new Error(t("pluginManager.marketplaceUserOnly"));
                  }
                  const id = itemId.replace(/^market:/, "");
                  const item = pluginManagerModel.marketplaceItems.find(
                    (candidate) => candidate.id === itemId
                  );
                  if (item?.marketplace) {
                    await installMarketplacePlugin(
                      item.marketplace.manifestPath,
                      item.marketplace.pluginName
                    );
                  } else {
                    await marketInstall(id);
                  }
                  await refreshPluginManagerData(scope);
                  toast(t("pluginHub.componentInstalledToast"), "success");
                }}
                onRefreshMarketplace={async () => {
                  await refreshPluginManagerData(pluginManagerScope);
                }}
                onOpenMarketplace={async () => {
                  const selected = await pickPluginMarketplace();
                  if (selected) setLocalPluginMarketplace(selected);
                }}
                onImportGithub={async (repository) => {
                  const result = await githubImportPlugin(repository);
                  await refreshPluginManagerData(pluginManagerScope);
                  toast(
                    t("pluginHub.pluginInstalledToast", {
                      name: result.plugin.name,
                    }),
                    "success"
                  );
                  return {
                    pluginId: result.plugin.id,
                    name: result.plugin.name,
                    version: result.plugin.version,
                  };
                }}
                onSetBundleEnabled={async (pluginId, enabled) => {
                  await setPluginEnabled(pluginId, enabled);
                  await refreshPluginManagerData(pluginManagerScope);
                  toast(
                    t(
                      enabled
                        ? "pluginHub.pluginEnabledToast"
                        : "pluginHub.pluginDisabledToast"
                    ),
                    "success"
                  );
                }}
                onSetBundleTrusted={async (pluginId, trusted) => {
                  await setPluginTrusted(pluginId, trusted);
                  await refreshPluginManagerData(pluginManagerScope);
                  toast(
                    t(
                      trusted
                        ? "pluginHub.pluginTrustedToast"
                        : "pluginHub.pluginUntrustedToast"
                    ),
                    "success"
                  );
                }}
                onUninstallBundle={async (pluginId, keepData) => {
                  await uninstallPlugin(pluginId, keepData);
                  await refreshPluginManagerData(pluginManagerScope);
                  toast(t("pluginHub.pluginUninstalledToast"), "success");
                }}
                onApplyScaffold={
                  cwd
                    ? async (pluginId, scaffoldId) => {
                        const result = await applyPluginScaffold(
                          pluginId,
                          scaffoldId,
                          cwd
                        );
                        toast(
                          t("pluginHub.scaffoldInstalledToast", {
                            count: result.files,
                          }),
                          "success"
                        );
                        return result;
                      }
                    : undefined
                }
              />
            )}
          </Suspense>

          <div
            ref={sessionWorkspaceRef}
            aria-hidden={
              showTaskBoard ||
              showPluginManager ||
              showAutomations ||
              showPullRequests ||
              showDocker ||
              showFeishu ||
              undefined
            }
            className={
              showTaskBoard ||
              showPluginManager ||
              showAutomations ||
              showPullRequests ||
              showDocker ||
              showFeishu
                ? "hidden"
                : "contents"
            }
          >
            <div
              data-workspace-stack
              className="flex min-h-0 min-w-0 flex-1 flex-col"
            >
              {/* ---------------- the session column ---------------- */}
              <main
                className={cn(
                  "bg-background rounded-card shadow-raised @container/workspace m-2 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden",
                  dockTab !== null && "mr-0"
                )}
                ref={mainRef}
              >
                {/* Tiling workspace: every pane renders the full column body below, so each keeps its
                own composer draft (its DocEditor is a distinct, stable-keyed instance that survives
                relayout). PaneTiles focuses a pane on mouse-down, while actions and async receipts
                carry that pane's id so later focus changes cannot redirect work or clear a draft.
                Values that shadow App scope here are per-pane; everything else falls through to the
                focused-session state by closure (a known v1 limitation for background panes). */}
                <PaneTiles
                  layout={paneLayout}
                  onFocusPane={focusPaneById}
                  onResizeSplit={resizeSplitById}
                  renderPane={(paneId, paneFocused) => {
                    const activeSession =
                      paneContents[paneId]?.sessionId ?? null;
                    const turns = turnsByPane[paneId] ?? EMPTY_TURNS;
                    const running =
                      activeSession === null
                        ? pendingSessionRunning &&
                          pendingCreationPane === paneId
                        : runningSessions.has(activeSession);
                    const transcriptState =
                      transcriptStateByPane[paneId] ??
                      EMPTY_PANE_TRANSCRIPT_STATE;
                    const sessionLoading = transcriptState.loading;
                    const hasConversationContent =
                      turns.length > 0 || running || sessionLoading;
                    const transcriptNextBefore = transcriptState.nextBefore;
                    const { loadingEarlier } = transcriptState;
                    const docEmpty = editorEmptyByPane[paneId] ?? true;
                    const editorKey = editorKeyByPane[paneId] ?? 0;
                    const docMode = paneFocused ? focusedDocMode : false;
                    const activeTitle = paneFocused
                      ? focusedActiveTitle
                      : titleForSession(activeSession);
                    const activeArchived =
                      activeSession !== null &&
                      archivedSessions.some((s) => s.id === activeSession);
                    const paneRefs = paneEditorRefsFor(paneId);
                    const { getBlocksRef } = paneRefs;
                    const { insertTextRef } = paneRefs;
                    const { insertAnnotationRef } = paneRefs;
                    const { insertFileRef } = paneRefs;
                    const { insertSessionRef } = paneRefs;
                    const focusEditorRef = paneRefs.focusRef;
                    const clearEditorRef = paneRefs.clearRef;
                    const { insertMarkdownRef } = paneRefs;
                    const { openSkillPickerRef } = paneRefs;
                    const { insertSkillRef } = paneRefs;
                    const { insertBriefRef } = paneRefs;
                    const { insertIssueRef } = paneRefs;
                    const { insertCanvasRef } = paneRefs;
                    const { insertCanvasDraftRef } = paneRefs;
                    const { restoreCanvasDocumentRef } = paneRefs;
                    const { freezeCanvasesRef } = paneRefs;
                    const { canvasDeliveryErrorRef } = paneRefs;
                    // Per-session project/cwd/git: the focused pane keeps the app's authoritative values;
                    // a background pane derives its project from its session record and reads git from
                    // the single-slot cache (which yields empty, not another project's status, when the
                    // pane's workspace isn't the one currently loaded).
                    const paneStored =
                      activeSession === null
                        ? null
                        : (sessions.find((s) => s.id === activeSession) ??
                          archivedSessions.find(
                            (s) => s.id === activeSession
                          ) ??
                          null);
                    const bgProjectPath = paneStored
                      ? sessionProjectPath(paneStored)
                      : null;
                    const cwd = paneFocused
                      ? focusedCwd
                      : (paneStored?.cwd ?? focusedCwd);
                    const activeProject = paneFocused
                      ? focusedActiveProject
                      : bgProjectPath != null &&
                          bgProjectPath !== "" &&
                          projects.some((p) => p.path === bgProjectPath)
                        ? bgProjectPath
                        : null;
                    const activeProjectRecord = paneFocused
                      ? focusedActiveProjectRecord
                      : (projects.find(
                          (project) => project.path === activeProject
                        ) ?? null);
                    const activeProjectName = paneFocused
                      ? focusedActiveProjectName
                      : (activeProjectRecord?.name ?? null);
                    const paneGitWorkspace = paneFocused
                      ? currentGitWorkspace
                      : workspaceStateForCwd(
                          gitWorkspace,
                          cwd || ".",
                          EMPTY_GIT_WORKSPACE
                        );
                    const git = paneFocused
                      ? focusedGit
                      : paneGitWorkspace.value.status;
                    const gitAction = paneFocused
                      ? focusedGitNextAction
                      : resolveGitNextAction({
                          status: paneGitWorkspace.value.status,
                          loading: paneGitWorkspace.loading,
                          sourceControl: paneGitWorkspace.value.sourceControl,
                          pullRequest: paneGitWorkspace.value.pullRequest,
                          forgeError: paneGitWorkspace.value.forgeError,
                          taskWorktree:
                            paneStored?.worktree_path != null &&
                            paneStored?.worktree_path !== "" &&
                            paneStored.worktree_discarded !== true,
                          canCleanup:
                            paneStored?.worktree_path != null &&
                            paneStored?.worktree_path !== "" &&
                            paneStored.worktree_discarded !== true,
                        });
                    // Per-session model/config/usage: the focused pane keeps the authoritative single
                    // values; a background pane reads its own session's recorded snapshot.
                    const models = paneFocused
                      ? focusedModels
                      : activeSession != null && activeSession !== ""
                        ? (modelsBySession[activeSession] ?? [])
                        : [];
                    const currentModel = paneFocused
                      ? focusedCurrentModel
                      : activeSession != null && activeSession !== ""
                        ? (currentModelBySession[activeSession] ?? null)
                        : null;
                    const defaultModel = paneFocused
                      ? focusedDefaultModel
                      : activeSession != null && activeSession !== ""
                        ? (defaultModelBySession[activeSession] ?? null)
                        : null;
                    const configOptions = paneFocused
                      ? focusedConfigOptions
                      : activeSession != null && activeSession !== ""
                        ? (configOptionsBySession[activeSession] ?? [])
                        : [];
                    // Usage is polled only for the focused session; a background pane hides its cost
                    // segment rather than borrow the focused figures.
                    const sessionUsage = paneFocused
                      ? focusedSessionUsage
                      : null;
                    const activeInteractionCapabilities =
                      activeSession != null && activeSession !== ""
                        ? (interactionCapabilities[activeSession] ?? null)
                        : null;
                    const activeAppshotKey =
                      activeSession ?? `draft:${(activeProject ?? cwd) || "."}`;
                    const activeAppshots =
                      pendingAppshots[activeAppshotKey] ?? EMPTY_APPSHOTS;
                    const paneProvider =
                      activeSession != null &&
                      activeSession !== "" &&
                      paneStored
                        ? providerLabel(paneStored.provider)
                        : provider;
                    const paneSessionConfig: SessionConfig = {
                      ...sessionConfig,
                      provider: paneProvider,
                      hasSession: activeSession !== null,
                      providerSwitching:
                        activeSession !== null &&
                        providerSwitchingSessions.has(activeSession),
                      providerChangeDisabled:
                        activeSession !== null &&
                        (running ||
                          providerSwitchingSessions.has(activeSession)),
                      onProvider: (next) =>
                        changeConversationProvider(activeSession, next, null),
                      onProviderModel: (nextProvider, nextModel) =>
                        changeConversationProvider(
                          activeSession,
                          nextProvider,
                          nextModel
                        ),
                    };
                    return (
                      <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden">
                        {/* Pane headers share a compact baseline below the native window strip.
                            Only marked background/title elements drag; controls remain interactive. */}
                        <header
                          data-has-conversation={
                            hasConversationContent ? "true" : undefined
                          }
                          className={cn(
                            "session-header window-titlebar electrobun-webkit-app-region-drag flex min-w-0 shrink-0 items-center gap-2 pr-4",
                            displayedRailCollapsed
                              ? "window-controls-safe-main"
                              : "pl-4"
                          )}
                        >
                          {/* Breadcrumb, reference-style: project / thread. */}
                          {activeProjectName != null &&
                            activeProjectName !== "" && (
                              <>
                                <span className="session-header-project-context electrobun-webkit-app-region-drag text-ui text-muted-foreground max-w-40 truncate">
                                  {activeProjectName}
                                </span>
                                {(Boolean(activeSession) ||
                                  activeBoardTask != null) && (
                                  <span className="session-header-project-context text-ui text-muted-foreground/50 shrink-0">
                                    /
                                  </span>
                                )}
                              </>
                            )}
                          {activeBoardTask ? (
                            <TooltipButton
                              type="button"
                              variant="ghost"
                              size="icon-xs"
                              className="text-primary"
                              label={t("taskboard.open")}
                              tooltip={activeBoardTask.id}
                              onClick={openTaskBoard}
                            >
                              <SquareKanban className="size-3.5" aria-hidden />
                            </TooltipButton>
                          ) : null}
                          {(Boolean(activeSession) ||
                            activeBoardTask != null) && (
                            <span className="session-header-title electrobun-webkit-app-region-drag text-ui max-w-96 truncate font-medium">
                              {activeTitle}
                            </span>
                          )}
                          {/* The session title trails the task title for context — unless both name the
                same thread (a task created from a single-prompt thread). */}
                          {activeBoardTask == null ? null : (
                            <SessionTitlePair
                              taskTitle={activeBoardTask.title}
                              sessionTitle={activeSessionTitle}
                            />
                          )}
                          {!activeBoardTask &&
                          activeSession != null &&
                          activeSession !== "" ? (
                            <span className="rounded-control bg-fill-rest text-metadata text-muted-foreground px-2 py-0.5">
                              {t("rail.newTemporarySession")}
                            </span>
                          ) : null}

                          <div className="electrobun-webkit-app-region-drag flex-1" />

                          <div className="session-header-toolbar gap-inline flex min-w-0 shrink-0 items-center">
                            {/* Full-page mode hides the transcript, so the header carries the only sign that a turn
                  is in flight — and the way back to the answer without leaving the mode for good. */}
                            {docMode && hasConversationContent && (
                              <Button
                                type="button"
                                variant="ghost"
                                size="compact"
                                onClick={() => toggleDocMode(false)}
                                className="text-callout text-muted-foreground mr-1 shrink-0 gap-1.5"
                                title={t("header.showTranscript", {
                                  count: turns.length,
                                })}
                              >
                                {(running || sessionLoading) && (
                                  <span className="bg-primary size-1.5 animate-pulse rounded-full" />
                                )}
                                {sessionLoading
                                  ? t("session.loading")
                                  : awaitingInput
                                    ? t("session.awaitingInput")
                                    : running
                                      ? t("header.running")
                                      : t("header.turns", {
                                          count: turns.length,
                                        })}
                              </Button>
                            )}

                            <div className="session-header-context-actions flex min-w-0 shrink-0 items-center gap-2">
                              {issueDeliveries
                                .filter(
                                  (delivery) =>
                                    delivery.session_id === activeSession
                                )
                                .map((delivery) => (
                                  <Button
                                    key={delivery.id}
                                    variant="ghost"
                                    size="compact"
                                    onClick={() =>
                                      setIssueConnector({
                                        plugin_id: delivery.plugin_id,
                                        connector_id: delivery.connector_id,
                                        name: delivery.issue.identifier,
                                      })
                                    }
                                  >
                                    {delivery.issue.identifier} ·{" "}
                                    {issueStageLabel(delivery.stage, t)}
                                  </Button>
                                ))}
                              <PluginUiSlot
                                slot="session.header"
                                contributions={
                                  pluginUiActions["session.header"]
                                }
                                onInvoke={invokePluginAction}
                              />
                              <EnvironmentPopover
                                suppressed={
                                  showTaskBoard ||
                                  showPluginManager ||
                                  showAutomations ||
                                  showPullRequests ||
                                  showDocker ||
                                  showFeishu
                                }
                                project={activeProjectName}
                                projectPath={
                                  activeProjectName != null &&
                                  activeProjectName !== ""
                                    ? activeProject
                                    : null
                                }
                                projects={projects}
                                git={git}
                                diffStat={diffStat}
                                onRefresh={refreshGit}
                                onSelectProject={selectProject}
                                onAddProject={() => void addProjectFolder()}
                                onOpenSourceControl={openSourceControl}
                                onOpenSettings={() => {
                                  setSettingsInitialTab("general");
                                  setShowSettings(true);
                                }}
                                preview={interactivePreview}
                              />
                            </div>

                            <SessionHeaderActions
                              gitAction={gitAction}
                              actions={scripts}
                              editorLaunchersAvailable={
                                editorLaunchersAvailable
                              }
                              fileManagerLabel={fileManagerLabel}
                              onRunAction={runProjectAction}
                              onAddAction={() => setShowActionDialog(true)}
                              onOpenCursor={() =>
                                void openWorkingDirectory("cursor")
                              }
                              onOpenAntigravity={() =>
                                void openWorkingDirectory("antigravity")
                              }
                              onOpenFinder={() =>
                                void openWorkingDirectory("finder")
                              }
                              finderHint={hint("open_finder")}
                              onOpenSourceControl={openSourceControl}
                              onOpenPullRequest={() =>
                                manualDockTab("pull-request")
                              }
                              onCleanupWorktree={() => {
                                if (paneStored)
                                  void discardWorktreeForSession(paneStored);
                              }}
                              onCheckpoint={() => void doCheckpoint()}
                              onPush={() =>
                                void doPush().catch(() => {
                                  /* empty */
                                })
                              }
                              onMoveTask={() =>
                                activeSession != null &&
                                activeSession !== "" &&
                                setShowTaskHandoff(true)
                              }
                            />

                            {/* Keep pane/window layout controls together at the trailing edge. */}
                            <PaneLayoutToolbar
                              onSplitRight={() =>
                                splitPaneById(paneId, "right")
                              }
                              onSplitDown={() =>
                                splitPaneById(paneId, "bottom")
                              }
                              onClose={() => closePaneById(paneId)}
                              canClose={multiPane}
                              labels={{
                                splitRight: t("pane.splitRight"),
                                splitDown: t("pane.splitDown"),
                                close: t("pane.close"),
                              }}
                              groupLabel={t("pane.layoutActions")}
                              viewLabel={t("pane.viewMenu")}
                              shortcuts={{
                                splitRight: hint("split_pane_right"),
                                splitDown: hint("split_pane_down"),
                                sidePanel: hint("toggle_side_panel"),
                              }}
                              panelLabel={t("pane.sidePanel")}
                              panelActive={dockTab !== null}
                              onTogglePanel={toggleSidePanel}
                            />
                          </div>
                        </header>

                        {/* Horizontal stage track (R9): rendered above the transcript only while the active
              session is bound to a pipeline instance. Pipeline state is the focused session's, so a
              background pane never paints it. */}
                        {paneFocused &&
                          scenesSurfaceEnabled &&
                          pipelineDetail &&
                          activeSession != null &&
                          activeSession !== "" && (
                            <StageTrack
                              detail={pipelineDetail}
                              onSelectSession={(id) => void selectSession(id)}
                            />
                          )}

                        {paneStored?.activity?.state.kind === "failed" && (
                          <div
                            role="alert"
                            className="bg-destructive/10 text-destructive text-body rounded-control mx-4 mt-2 px-3 py-2"
                          >
                            <strong>{t("mission.state.failed")}</strong> ·{" "}
                            {paneStored.activity.state.message}
                          </div>
                        )}
                        {/* The same transcript tree serves the main column and document side panel. Keeping the
              rendering path unified prevents the two modes from drifting, while the scroll
              controller preserves the reader's position as streamed content arrives. */}
                        <div
                          className={cn(
                            "flex min-h-0 flex-1",
                            docMode
                              ? "flex-row @max-[48rem]/workspace:flex-col"
                              : "flex-col"
                          )}
                        >
                          {hasConversationContent && (
                            <TranscriptPane
                              sessionId={activeSession}
                              variant={docMode ? "side" : "main"}
                              turns={turns}
                              loading={sessionLoading}
                              hasEarlier={transcriptNextBefore !== null}
                              loadingEarlier={loadingEarlier}
                              onLoadEarlier={(scroll) =>
                                void loadEarlierTranscript(paneId, scroll)
                              }
                              onSaveTemplate={openTemplateDraft}
                              linkActions={builtinLinkActions}
                              onForkTurn={forkTurnIntoTask}
                              onAddSelection={addSelectedText}
                              onExplainSelection={explainSelectedText}
                              onAskSelectionInSideChat={
                                askSelectedTextInSideChat
                              }
                              before={
                                <PluginUiSlot
                                  slot="transcript.before"
                                  contributions={
                                    pluginUiActions["transcript.before"]
                                  }
                                  onInvoke={invokePluginAction}
                                />
                              }
                            />
                          )}

                          {/* One wrapper in both modes so the Composer keeps its tree position across the toggle —
                BlockNote unmounts (and takes the draft with it) if the structure around it changes.
                Compact, the wrapper is just the composer's slot; expanded, it is the document column
                inside the outer transcript/document row. Keep this wrapper vertical so banners and
                plugin contributions remain above the document instead of consuming its width. An empty
                thread gives the heading the spare space above the bottom composer. */}
                          <div
                            ref={heroScrollRef}
                            className={cn(
                              "flex",
                              docMode
                                ? "order-1 min-h-0 min-w-0 flex-1 flex-col"
                                : turns.length === 0 && !sessionLoading
                                  ? "hero-scroll-shell order-2 min-h-0 flex-1 flex-col overflow-y-auto"
                                  : "order-2 shrink-0 flex-col"
                            )}
                          >
                            {docMode &&
                              turns.length === 0 &&
                              !sessionLoading && (
                                <div className="document-intro pt-page pb-surface-inset">
                                  <h1 className="text-heading font-semibold">
                                    {t("workspace.startTitle")}
                                  </h1>
                                  <p className="text-body text-muted-foreground mt-2">
                                    {t("workspace.startHint")}
                                  </p>
                                </div>
                              )}
                            {/* "What should we build in <project>?" — the project name is the project switcher. */}
                            {!docMode &&
                              turns.length === 0 &&
                              !sessionLoading && (
                                <div
                                  data-workspace-greeting
                                  className="flex min-h-24 flex-1 shrink-0 items-center justify-center px-6 py-6"
                                >
                                  <h1 className="text-page text-center font-medium">
                                    {activeProjectName == null ||
                                    activeProjectName === "" ? (
                                      t("transcript.greeting")
                                    ) : (
                                      <>
                                        {t("transcript.greetingIn")}{" "}
                                        <DropdownMenu>
                                          <DropdownMenuTrigger
                                            render={
                                              <Button
                                                type="button"
                                                variant="link"
                                                size="compact"
                                                className="decoration-muted-foreground/40 h-auto px-0 py-0 [font-size:inherit] [line-height:inherit] [font-weight:inherit] [letter-spacing:inherit] text-inherit decoration-dotted underline-offset-[7px]"
                                                title={
                                                  activeProject ?? undefined
                                                }
                                              >
                                                {activeProjectName ??
                                                  t("rail.noProject")}
                                              </Button>
                                            }
                                          />
                                          <DropdownMenuContent
                                            side="top"
                                            align="center"
                                            className="w-60"
                                          >
                                            {projects.length > 0 && (
                                              <>
                                                <DropdownMenuGroup>
                                                  {projects.map((project) => (
                                                    <DropdownMenuItem
                                                      key={project.path}
                                                      onClick={() =>
                                                        selectProject(
                                                          project.path
                                                        )
                                                      }
                                                    >
                                                      <Folder />
                                                      <span
                                                        className="min-w-0 flex-1 truncate"
                                                        title={project.path}
                                                      >
                                                        {project.name}
                                                      </span>
                                                      {project.path ===
                                                        activeProject && (
                                                        <Check />
                                                      )}
                                                    </DropdownMenuItem>
                                                  ))}
                                                </DropdownMenuGroup>
                                                <DropdownMenuSeparator />
                                              </>
                                            )}
                                            <DropdownMenuItem
                                              onClick={() =>
                                                void addProjectFolder()
                                              }
                                            >
                                              <FolderPlus />
                                              {t("rail.addProject")}
                                            </DropdownMenuItem>
                                          </DropdownMenuContent>
                                        </DropdownMenu>{" "}
                                        {t("transcript.greetingEnd")}
                                      </>
                                    )}
                                  </h1>
                                </div>
                              )}
                            {/* An archived chat reads, but doesn't run: the composer yields its slot to this notice
                  until the session is restored. The composer stays mounted (hidden) — unmounting
                  BlockNote would take an in-progress draft with it. */}
                            {activeArchived && (
                              <div className="shrink-0 px-6 pt-3 pb-6">
                                <Card
                                  variant="surface"
                                  density="compact"
                                  className="mx-auto w-full max-w-3xl flex-row items-center px-4"
                                >
                                  <Archive className="text-muted-foreground size-4 shrink-0" />
                                  <p className="text-ui text-muted-foreground min-w-0 flex-1">
                                    {t("archived.notice")}
                                  </p>
                                  <Button
                                    size="sm"
                                    variant="secondary"
                                    className="shrink-0"
                                    onClick={() =>
                                      activeSession &&
                                      void archiveSession(
                                        activeSession,
                                        false
                                      ).then(refreshSessions)
                                    }
                                  >
                                    {t("archived.restore")}
                                  </Button>
                                </Card>
                              </div>
                            )}
                            {/* Quiet scene banner (R8): stage completion / hook suggestions for the focused
                  session, rendered above the composer. Dismissal is remembered by the core. */}
                            {scenesSurfaceEnabled &&
                              sceneBanner &&
                              sceneBanner.session === activeSession &&
                              !activeArchived && (
                                <SceneBanner
                                  banner={sceneBanner}
                                  scenes={scenes}
                                  onApplyScene={(reference) => {
                                    applySceneChoice(reference);
                                    setSceneBanner(null);
                                  }}
                                  onAdvancePipeline={(instanceId, toStage) => {
                                    void advancePipelineChoice(
                                      instanceId,
                                      toStage
                                    );
                                    setSceneBanner(null);
                                  }}
                                  onAdvancePipelineNewSession={(
                                    instanceId,
                                    toStage
                                  ) => {
                                    void advancePipelineInNewSession(
                                      instanceId,
                                      toStage
                                    );
                                    setSceneBanner(null);
                                  }}
                                  onDismiss={() => {
                                    void dismissSceneBanner(
                                      sceneBanner.session,
                                      sceneBanner.stateKey
                                    );
                                    setSceneBanner(null);
                                  }}
                                />
                              )}
                            {!activeArchived && (
                              <PluginUiSlot
                                slot="composer.above"
                                contributions={
                                  pluginUiActions["composer.above"]
                                }
                                onInvoke={invokePluginAction}
                              />
                            )}
                            {permission != null &&
                              !permission.form &&
                              !activeArchived && (
                                <PermissionCard
                                  key={`${permission.session}:${permission.requestId}`}
                                  request={permission}
                                  pendingCount={activePendingInputs.length}
                                  onAnswer={answer}
                                />
                              )}
                            <div
                              className={cn(
                                "contents",
                                activeArchived && "hidden"
                              )}
                            >
                              <Composer
                                config={paneSessionConfig}
                                checkout={{
                                  project: activeProjectName ?? cwd,
                                  branch:
                                    git?.is_repo === true ? git.branch : null,
                                  dirty: git?.files.length ?? 0,
                                  onOpen: openSourceControl,
                                }}
                                docMode={docMode}
                                onDocMode={toggleDocMode}
                                boundsRef={mainRef}
                                models={
                                  activeSession === null
                                    ? (providers.find(
                                        (candidate) => candidate.id === provider
                                      )?.models ?? [])
                                    : models
                                }
                                currentModel={currentModel}
                                defaultModel={defaultModel}
                                contextWindow={activeContextWindow(
                                  contextWindows,
                                  activeSession
                                )}
                                usage={sessionUsage}
                                {...(activeInteractionCapabilities?.compact_context ===
                                true
                                  ? {
                                      onCompactContext: () =>
                                        void run(
                                          [{ type: "text", text: "/compact" }],
                                          undefined,
                                          paneId,
                                          activeAppshots
                                        ),
                                    }
                                  : {})}
                                onModel={(id) =>
                                  changeSessionModel(activeSession, id)
                                }
                                configOptions={configOptions}
                                onConfigOption={(configId, value) =>
                                  changeSessionConfigOption(
                                    activeSession,
                                    configId,
                                    value
                                  )
                                }
                                running={running}
                                loading={sessionLoading}
                                docEmpty={docEmpty}
                                appshots={activeAppshots}
                                onRemoveAppshot={(id) =>
                                  removePendingAppshots([id])
                                }
                                onRun={() =>
                                  void run(
                                    undefined,
                                    undefined,
                                    paneId,
                                    activeAppshots
                                  )
                                }
                                onQueue={() =>
                                  void sendDuringTurn(
                                    "queued",
                                    undefined,
                                    paneId,
                                    activeAppshots
                                  )
                                }
                                onMultitask={startParallelTask}
                                onSteer={() =>
                                  void sendDuringTurn(
                                    "steer",
                                    undefined,
                                    paneId,
                                    activeAppshots
                                  )
                                }
                                steeringSupported={
                                  activeInteractionCapabilities?.steering ??
                                  false
                                }
                                onStop={() =>
                                  activeSession != null &&
                                  activeSession !== "" &&
                                  void cancelTurn(activeSession)
                                }
                                onAttachFile={() => {
                                  if (componentEnabled("files.surface"))
                                    setShowFiles(true);
                                  else
                                    toast(
                                      "Files are disabled in Plugins.",
                                      "info"
                                    );
                                }}
                                onAttachImages={attachPromptImages}
                                onInsertSkill={() =>
                                  openSkillPickerRef.current?.()
                                }
                                onInsertIssue={() => {
                                  if (componentEnabled("issues.modal"))
                                    setShowIssues(true);
                                  else
                                    toast(
                                      "Issues are disabled in Plugins.",
                                      "info"
                                    );
                                }}
                                onOpenMarket={openPluginManager}
                                onNewSkill={() =>
                                  setSkillDraft({ name: "", text: "" })
                                }
                                canvasEnabled={canvasUiEnabled}
                                onInsertCanvas={() =>
                                  void insertCanvasRef.current?.()
                                }
                                voiceEnabled={voiceComposerEnabled}
                                onVoiceText={(t) => insertTextRef.current?.(t)}
                                // R11: with an active-scene brief, a finished dictation is structured into a
                                // pre-filled brief card; any failure degrades to the raw-text insert above.
                                // No brief → the handler is undefined and voice behaves exactly as before.
                                onVoiceTranscript={
                                  voiceComposerEnabled && scenesSurfaceEnabled
                                    ? makeTranscriptHandler({
                                        scene:
                                          scenes.find(
                                            (s) =>
                                              s.reference === activeSceneName
                                          ) ?? null,
                                        structureBrief,
                                        insertBrief: (scene, values) =>
                                          insertBriefRef.current?.(
                                            scene,
                                            values
                                          ),
                                        insertText: (text) =>
                                          insertTextRef.current?.(text),
                                        onDegrade: () =>
                                          toast(
                                            t("voice.structureFailed"),
                                            "error"
                                          ),
                                      })
                                    : undefined
                                }
                                runHint={hint("run")}
                                skillHint={hint("open_skill_picker")}
                                filesHint={hint("open_files")}
                                pluginActions={
                                  <PluginUiSlot
                                    slot="composer.toolbar"
                                    contributions={
                                      pluginUiActions["composer.toolbar"]
                                    }
                                    onInvoke={invokePluginAction}
                                  />
                                }
                                sessionId={activeSession}
                                insertBriefRef={insertBriefRef}
                              >
                                <DocEditor
                                  key={editorKey}
                                  skills={activeSkills}
                                  cwd={cwd || "."}
                                  sessionId={activeSession}
                                  getBlocksRef={getBlocksRef}
                                  insertTextRef={insertTextRef}
                                  insertAnnotationRef={insertAnnotationRef}
                                  insertFileRef={insertFileRef}
                                  insertSessionRef={insertSessionRef}
                                  focusRef={focusEditorRef}
                                  clearRef={clearEditorRef}
                                  insertMarkdownRef={insertMarkdownRef}
                                  openSkillPickerRef={openSkillPickerRef}
                                  sceneSkills={(() => {
                                    const scene = scenes.find(
                                      (s) => s.reference === activeSceneName
                                    );
                                    return scene?.skills
                                      ? {
                                          pinned: scene.skills.pinned ?? [],
                                          suppressUnpinned:
                                            scene.skills.suppress_unpinned ??
                                            false,
                                        }
                                      : null;
                                  })()}
                                  insertSkillRef={insertSkillRef}
                                  insertBriefRef={insertBriefRef}
                                  insertIssueRef={insertIssueRef}
                                  canvasEnabled={canvasUiEnabled}
                                  canvasRuntime={canvasRuntime}
                                  createCanvas={createCanvas}
                                  insertCanvasRef={insertCanvasRef}
                                  insertCanvasDraftRef={insertCanvasDraftRef}
                                  restoreCanvasDocumentRef={
                                    restoreCanvasDocumentRef
                                  }
                                  freezeCanvasesRef={freezeCanvasesRef}
                                  canvasDeliveryErrorRef={
                                    canvasDeliveryErrorRef
                                  }
                                  onPasteImages={attachPromptImages}
                                  onEmptyChange={paneEmptyHandlerFor(paneId)}
                                  onDocumentChange={paneDocumentHandlerFor(
                                    paneId
                                  )}
                                />
                              </Composer>
                            </div>
                          </div>
                        </div>
                      </div>
                    );
                  }}
                />
              </main>
            </div>

            {/* ---------------- right work dock ---------------- */}
            {/* Always mounted: closing animates the width to zero instead of unmounting, which both
                plays the full collapse and keeps shells alive across close/open. */}
            <Dock
              titlebarHost={dockTitlebarHost}
              open={dockTab !== null}
              tab={dockTab}
              availableSurfaces={availableDockSurfaces}
              onTab={manualDockTab}
              onClose={() => manualDockTab(null)}
              autoTab={dockAutoHint?.surface ?? null}
              content={{
                trajectory: (
                  <TrajectoryView
                    turns={turns}
                    usage={focusedSessionUsage}
                    hasEarlier={focusedTranscriptState.nextBefore !== null}
                    loadingEarlier={focusedTranscriptState.loadingEarlier}
                    onLoadEarlier={() =>
                      void loadEarlierTranscript(paneLayout.focused)
                    }
                  />
                ),
                browser: (
                  <BrowserPanel
                    url={browserUrl}
                    projectPath={lspProjectPath}
                    visible={dockTab !== null}
                    onNavigate={setBrowserUrl}
                    onAnnotate={(notes) => void annotate(notes)}
                  />
                ),
                terminal: (
                  <TerminalDockContent
                    cwd={cwd || null}
                    projectPath={lspProjectPath}
                    sessionKey={activeSession ?? "main"}
                    onSendText={(text) => insertTextRef.current?.(text)}
                  />
                ),
                "side-chat": (
                  <SideChatPanel
                    open={dockTab === "side-chat"}
                    onClose={() => manualDockTab(null)}
                    provider={provider}
                    providers={providers}
                    cwd={cwd || "."}
                    model={currentModel}
                    mode={mode}
                    sandbox={sandbox}
                    voiceEnabled={voiceComposerEnabled}
                    seed={sideChatSeed}
                    onSeedHandled={(id) =>
                      setSideChatSeed((current) =>
                        current?.id === id ? null : current
                      )
                    }
                    linkActions={builtinLinkActions}
                  />
                ),
                files: (
                  <FileDockContent
                    cwd={cwd || null}
                    openFiles={openFiles}
                    activeFile={activeFile}
                    reveal={fileReveal}
                    highlightFile={dockAutoHint?.file ?? null}
                    onActiveFile={(path) => {
                      setActiveFile(path);
                      setFileReveal(null);
                    }}
                    onCloseFile={closeFileTab}
                    onInsertFile={(path) => insertFileRef.current?.(path)}
                    onOpenFile={openFileTab}
                    onSendText={(text) => insertTextRef.current?.(text)}
                  />
                ),
                git: (
                  <GitDockContent
                    status={git}
                    action={gitNextAction}
                    onOpenSourceControl={openSourceControl}
                    onPush={() =>
                      void doPush().catch(() => {
                        /* empty */
                      })
                    }
                    onOpenPullRequest={() => manualDockTab("pull-request")}
                    onCleanupWorktree={() => {
                      if (activeSessionRecord) {
                        void discardWorktreeForSession(activeSessionRecord);
                      }
                    }}
                  />
                ),
                "pull-request": (
                  <PullRequestDockContent
                    cwd={cwd || null}
                    status={git}
                    onRefresh={refreshGit}
                  />
                ),
              }}
              width={dockWidth}
              onWidth={setDockWidth}
              reservedWidth={railInlineWidth + APP_NAV_WIDTH}
            />
          </div>
        </div>
      )}

      <QuickChatPanel
        open={quickChatOpen}
        onClose={() => setQuickChatOpen(false)}
        provider={provider}
        providers={providers}
        cwd={cwd || "."}
        model={currentModel}
        mode={mode}
        sandbox={sandbox}
        voiceEnabled={voiceComposerEnabled}
        seed={null}
        onSeedHandled={() => {
          /* empty */
        }}
        linkActions={builtinLinkActions}
      />

      {/* ---------------- dialogs ---------------- */}
      {showSourceControl && componentEnabled("git.surface") && (
        <SourceControlModal
          key={cwd || "."}
          cwd={cwd || "."}
          status={git}
          statusLoading={currentGitWorkspace.loading}
          checkpoints={checkpoints}
          checkpointsLoading={currentCheckpointWorkspace.loading}
          onCommit={async (m) => {
            try {
              await gitCommit(cwd || ".", m);
              toast("Committed.", "success");
            } catch (error) {
              toast(`Commit failed: ${error}`, "error");
              throw error;
            } finally {
              refreshGit();
            }
          }}
          onPush={doPush}
          onCheckpoint={doCheckpoint}
          onRevert={async (c) => {
            await gitRevert(cwd || ".", c);
            refreshGit();
          }}
          onRefresh={() => {
            refreshGit();
            refreshCheckpoints();
          }}
          onClose={() => setShowSourceControl(false)}
        />
      )}
      {showPalette && (
        <CommandPalette
          commands={paletteCommands}
          search={searchPaletteCommands}
          onClose={() => setShowPalette(false)}
        />
      )}
      <ProjectActionDialog
        open={showActionDialog}
        actions={scripts}
        bindings={bindings}
        onOpenChange={setShowActionDialog}
        onSave={saveProjectAction}
      />
      {showRemote && componentEnabled("remote.modal") && (
        <RemoteModal onClose={() => setShowRemote(false)} />
      )}
      {showTaskHandoff && activeSession != null && activeSession !== "" && (
        <TaskHandoffDialog
          session={activeSession}
          onClose={() => setShowTaskHandoff(false)}
          onTransferred={() => {
            setShowTaskHandoff(false);
            void refreshSessions();
            toast("Task moved to the remote device.", "success");
          }}
        />
      )}
      {issueConnector ? (
        <IssueDeliveryDialog
          connector={issueConnector}
          projects={projects}
          providers={providers}
          repository={activeProject ?? ""}
          onOpenSession={(id) => {
            setIssueConnector(null);
            setShowPluginManager(false);
            void selectSession(id);
          }}
          onClose={() => setIssueConnector(null)}
        />
      ) : null}
      {showIssues && componentEnabled("issues.modal") && (
        <IssuesModal
          issueTrackers={issueConnectors.map((connector) => ({
            name: connector.name,
            open: () => {
              setShowIssues(false);
              setIssueConnector(connector);
            },
          }))}
          cwd={cwd || "."}
          scenes={scenesSurfaceEnabled ? scenes : []}
          onInsert={(i) => void insertIssue(i)}
          onDelegate={(i, sceneReference) =>
            void onDelegateIssue(i, sceneReference)
          }
          onOpenSession={(session) => {
            setShowIssues(false);
            void selectSession(session);
          }}
          onClose={() => setShowIssues(false)}
        />
      )}
      {preview && (
        <PreviewModal preview={preview} onClose={() => setPreview(null)} />
      )}
      {showMissionControl && (
        <MissionControlPage
          sessions={sessions}
          runningSessions={runningSessions}
          contextWindows={contextWindows}
          sceneBySession={
            scenesSurfaceEnabled
              ? sceneBySessionRef.current
              : EMPTY_SCENE_BY_SESSION
          }
          onSelect={(id) => void selectSession(id)}
          onReview={openSourceControl}
          onClose={() => setShowMissionControl(false)}
        />
      )}
      {showScenePicker && scenesSurfaceEnabled && (
        <ScenePicker
          scenes={scenes}
          active={scenes.find((s) => s.reference === activeSceneName) ?? null}
          auto={autoScene}
          onAuto={setAutoSceneChoice}
          onScene={(reference) => applySceneChoice(reference)}
          onCreate={() => {
            setShowScenePicker(false);
            setSceneEditorRequest({ kind: "create" });
            setShowSceneStudio(true);
          }}
          onEdit={(scene) => {
            setShowScenePicker(false);
            setSceneEditorRequest({ kind: "edit", scene });
            setShowSceneStudio(true);
          }}
          onDuplicate={(scene) => {
            setShowScenePicker(false);
            setSceneEditorRequest({ kind: "duplicate", scene });
            setShowSceneStudio(true);
          }}
          onClose={() => setShowScenePicker(false)}
        />
      )}
      {sceneEscalation && scenesSurfaceEnabled && (
        <SceneEscalationDialog
          sceneLabel={
            scenes.find((s) => s.reference === sceneEscalation.reference)
              ?.title ?? sceneEscalation.reference
          }
          from={sceneEscalation.from}
          to={sceneEscalation.to}
          onConfirm={() => {
            const pending = sceneEscalation;
            setSceneEscalation(null);
            if (pending.kind === "soft")
              applySceneChoice(pending.reference, { confirmed: true });
            else if (pending.kind === "pipeline" && pending.pipeline)
              void advancePipelineChoice(
                pending.pipeline.instanceId,
                pending.pipeline.toStage,
                true
              );
            else if (pending.kind === "pipeline_new" && pending.pipeline)
              void advancePipelineInNewSession(
                pending.pipeline.instanceId,
                pending.pipeline.toStage,
                true
              );
            else void restartInScene(true);
          }}
          onCancel={() => setSceneEscalation(null)}
        />
      )}
      {showFiles && componentEnabled("files.surface") && (
        <FileBrowserModal
          cwd={cwd || "."}
          onInsert={(p) => {
            insertFileRef.current?.(p);
            setShowFiles(false);
          }}
          onClose={() => setShowFiles(false)}
        />
      )}
      {showWorkspaceSearch && componentEnabled("search.modal") && (
        <WorkspaceSearchModal
          cwd={cwd || "."}
          onOpen={(match) => openFileTab(match.path, match)}
          onClose={() => setShowWorkspaceSearch(false)}
        />
      )}

      {skillDraft && (
        <Dialog open onOpenChange={(o) => !o && setSkillDraft(null)}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>New skill</DialogTitle>
            </DialogHeader>
            <Input
              placeholder="Skill name"
              value={skillDraft.name}
              onChange={(e) =>
                setSkillDraft({ ...skillDraft, name: e.target.value })
              }
            />
            <Textarea
              placeholder="Prompt fragment inserted when this skill is picked"
              value={skillDraft.text}
              onChange={(e) =>
                setSkillDraft({ ...skillDraft, text: e.target.value })
              }
            />
            <DialogFooter>
              <Button variant="outline" onClick={() => setSkillDraft(null)}>
                Cancel
              </Button>
              <Button onClick={() => void saveDraft()}>Save</Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}

      {templateDraft !== null && (
        <TemplateDialog
          source={templateDraft}
          onClose={() => setTemplateDraft(null)}
          onSaved={() => {
            setTemplateDraft(null);
            void refreshSkills();
            toast(t("templateFrom.saved"), "success");
          }}
        />
      )}

      {permission?.form && (
        <QuestionDialog
          key={`${permission.session}:${permission.requestId}`}
          form={permission.form}
          onAnswer={(value) => void answerQuestion(value)}
        />
      )}
    </div>
  );
}

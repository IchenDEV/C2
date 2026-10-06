import { useEffect, useId, useRef, useState } from "react";
import type { MutableRefObject, ReactNode } from "react";

import { SearchField } from "@/components/business/search-field";
import { SelectableRow } from "@/components/business/selectable-row";
import { ActivityOrb } from "@/components/ui/activity-orb";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ControlChip as Chip } from "@/components/ui/control-chip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  ArrowUp,
  BrainCircuit,
  ChevronDown,
  FileText,
  Folder,
  GitBranch,
  ImagePlus,
  ListChecks,
  Lock,
  LockOpen,
  Maximize2,
  Minimize2,
  Paperclip,
  PenLine,
  Sparkles,
  SlidersHorizontal,
  Square,
  Star,
  Store,
  Ticket,
  TriangleAlert,
  X,
} from "@/components/ui/icons";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import type { ConfigOptionInfo, AppshotCapture, ModelChoice } from "../bridge";
import { briefOfferVisible } from "../editor/slotCard";
import { useT } from "../i18n";
import { ProviderIcon } from "../providers/ProviderIcon";
import { VoiceButton } from "../voice/VoiceButton";
import { memoryPresetsForProvider } from "./config";
import type { SessionConfig } from "./config";
import type { ContextWindow } from "./contextWindow";
import { SESSION_MODES, sessionMode } from "./mode";
import { useProviderModelFavorites } from "./modelFavorites";
import { useProviderModelPreferences } from "./modelPreferences";
import { familyOf, groupModels, pickVariant, variantOf } from "./models";
import type { Effort } from "./models";
import type { SceneInfo } from "./scene";
import { SceneChip } from "./SceneChip";
import { worktreeGatingReason } from "./sessionEvents";
// Explicit extension: this directory also contains the case-colliding `statusline.ts` helper.
import { Statusline } from "./Statusline.tsx";
import type { StatuslineUsage } from "./Statusline.tsx";

interface ComposerProps {
  /** The document editor itself. The composer only owns the frame around it. */
  children: ReactNode;
  config: SessionConfig;
  /** The checkout bar under the card: execution location on the left, source control on the right. */
  checkout?: {
    project: string | null;
    branch: string | null;
    dirty: number;
    onOpen: () => void;
  } | null;
  /** Full-page authoring: the document takes the whole column and the transcript steps aside. */
  docMode: boolean;
  onDocMode: (v: boolean) => void;
  /** The column the composer lives in; bounds compact content so it can't swallow the transcript. */
  boundsRef: React.MutableRefObject<HTMLElement | null>;
  /** What the agent reported it can run. Empty until a session exists, or if it reports none. */
  models: ModelChoice[];
  currentModel: string | null;
  /** The current session's authoritative provider context usage/capacity, if reported. */
  contextWindow: ContextWindow | null;
  /** Per-session cost/burn for the statusline; null until the core's usage command exists. */
  usage?: StatuslineUsage | null;
  /** Present only after the live provider session advertises its native `/compact` command. */
  onCompactContext?: () => void;
  /** The adapter's own pick at session/new — worth a "Default" badge in the menus. */
  defaultModel: string | null;
  onModel: (id: string) => void;
  /** Selectors the agent reported as session config options — model and reasoning effort. */
  configOptions: ConfigOptionInfo[];
  onConfigOption: (configId: string, value: string) => void;
  running: boolean;
  /** Session summary is visible while its transcript detail is still loading; sending is gated. */
  loading: boolean;
  docEmpty: boolean;
  appshots?: AppshotCapture[];
  onRemoveAppshot?: (id: string) => void;
  onRun: () => void;
  onQueue: () => void;
  onMultitask: () => void;
  onSteer: () => void;
  onStop: () => void;
  steeringSupported: boolean;
  onAttachFile: () => void;
  onAttachImages: (files: readonly File[]) => void | Promise<void>;
  onInsertSkill: () => void;
  onInsertIssue: () => void;
  onOpenMarket: () => void;
  onNewSkill: () => void;
  canvasEnabled: boolean;
  onInsertCanvas: () => void;
  /** Component-policy gate; false means the voice plugin may already be unloaded. */
  voiceEnabled: boolean;
  onVoiceText: (text: string) => void;
  /** R11: present only when the active scene has a brief — voice then structures into it. */
  onVoiceTranscript?: (full: string) => Promise<void>;
  runHint: string;
  skillHint: string;
  filesHint: string;
  /** Host-rendered declarative plugin actions in the compact control row. */
  pluginActions?: ReactNode;
  /** Keys the per-session brief-offer dismissal; null while no session exists yet. */
  sessionId?: string | null;
  /** Editor-owned seam that inserts the active scene's brief as a slot card (R5). */
  insertBriefRef?: MutableRefObject<
    ((scene: SceneInfo, values?: Record<string, string>) => void) | null
  >;
}

function displayGitRef(reference: string | null | undefined): string | null {
  if (reference == null || reference === "") return null;
  return reference
    .replace(/^refs\/heads\//, "")
    .replace(/^refs\/remotes\//, "");
}

/**
 * The persistent checkout control. It keeps execution location separate from source control, and
 * only presents baselines the engine can actually materialize.
 */
export function CheckoutBar({
  config,
  checkout,
}: {
  config: SessionConfig;
  checkout: NonNullable<ComposerProps["checkout"]>;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const gatingReason = worktreeGatingReason(
    config.hasSession,
    config.worktreeOptions,
    config.worktreeOptionsLoading
  );
  const selectedKind = config.hasSession
    ? (config.activeWorktreeBaseline?.kind ?? null)
    : config.worktreeBase;
  const selected =
    selectedKind == null
      ? null
      : config.worktreeOptions.find((option) => option.kind === selectedKind);
  const selectedRef = displayGitRef(
    config.hasSession
      ? config.activeWorktreeBaseline?.ref
      : selected?.resolved?.ref
  );
  const modeLabel = config.activeWorktreeUnknown
    ? t("checkout.legacy")
    : selectedKind == null
      ? t("checkout.project")
      : config.hasSession
        ? t("checkout.sessionWorktree")
        : t("checkout.newWorktree");
  const projectDetail =
    checkout.branch != null && checkout.branch !== ""
      ? checkout.dirty > 0
        ? t("checkout.branchDirty", {
            branch: checkout.branch,
            count: checkout.dirty,
          })
        : t("checkout.branchClean", { branch: checkout.branch })
      : t("checkout.notRepository");
  const projectLabel =
    checkout.project == null ||
    checkout.project === "" ||
    checkout.project === "."
      ? t("rail.noProject")
      : checkout.project;

  useEffect(() => {
    if (
      !config.hasSession &&
      gatingReason !== null &&
      config.worktreeBase !== null
    ) {
      config.onWorktreeBase(null);
    }
  }, [
    config.hasSession,
    config.onWorktreeBase,
    config.worktreeBase,
    gatingReason,
  ]);

  return (
    <div
      data-checkout-bar
      className="min-h-control-field rounded-module bg-fill-rest text-metadata text-muted-foreground relative z-0 mx-4 -mt-4 flex min-w-0 flex-wrap items-center gap-x-2 px-2 pt-5 pb-1"
    >
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger
          render={
            <Button
              type="button"
              variant="ghost"
              size="compact"
              focusStyle="inset"
              className="min-w-0 shrink gap-1.5 px-2"
              title={t("checkout.choose")}
              aria-label={`${t("checkout.title")}: ${modeLabel}`}
              aria-expanded={open}
            >
              <Folder className="size-3.5 shrink-0" aria-hidden="true" />
              <span className="text-body min-w-0 truncate">{modeLabel}</span>
              <ChevronDown className="size-3 shrink-0" aria-hidden="true" />
            </Button>
          }
        />
        <PopoverContent
          align="start"
          side="bottom"
          sideOffset={10}
          className="w-80 max-w-(--available-width) p-1.5"
        >
          {config.hasSession ? (
            <>
              <SelectableRow
                selected
                disabled
                label={
                  selectedRef ??
                  (selectedKind == null ? projectLabel : modeLabel)
                }
                accessibilityContext={modeLabel}
                meta={t("checkout.currentBadge")}
                description={
                  config.activeWorktreeUnknown
                    ? t("worktree.legacyUnknownHint")
                    : (config.activeWorktreeBaseline?.display ?? projectDetail)
                }
                onSelect={() => {
                  /* empty */
                }}
              />
              <p className="text-callout text-muted-foreground px-2.5 pt-1.5 pb-1">
                {t("worktree.fixedForSession")}
              </p>
            </>
          ) : (
            <>
              <SelectableRow
                selected={config.worktreeBase == null}
                label={projectLabel}
                accessibilityContext={t("checkout.project")}
                meta={
                  config.worktreeBase == null
                    ? t("checkout.currentBadge")
                    : t("checkout.projectBadge")
                }
                description={projectDetail}
                onSelect={() => {
                  config.onWorktreeBase(null);
                  setOpen(false);
                }}
              />

              {WORKTREE_BASELINES.map((kind) => {
                const option = config.worktreeOptions.find(
                  (candidate) => candidate.kind === kind
                );
                const unavailable =
                  !config.worktreeOptionsLoading && option?.resolved == null;
                const detail = config.worktreeOptionsLoading
                  ? t("worktree.resolving")
                  : (option?.resolved?.display ??
                    option?.unavailable_reason ??
                    gatingReason ??
                    t("worktree.unavailable"));
                return (
                  <SelectableRow
                    key={kind}
                    selected={config.worktreeBase === kind}
                    disabled={unavailable}
                    label={
                      displayGitRef(option?.resolved?.ref) ??
                      (kind === "current"
                        ? t("checkout.currentRef")
                        : t("checkout.originRef"))
                    }
                    accessibilityContext={
                      kind === "current"
                        ? t("checkout.currentRef")
                        : t("checkout.originRef")
                    }
                    meta={
                      unavailable
                        ? t("checkout.unavailableBadge")
                        : config.worktreeBase === kind
                          ? t("checkout.currentBadge")
                          : t("checkout.worktreeBadge")
                    }
                    description={detail}
                    onSelect={() => {
                      config.onWorktreeBase(kind);
                      setOpen(false);
                    }}
                  />
                );
              })}
            </>
          )}
        </PopoverContent>
      </Popover>

      {checkout.branch != null && checkout.branch !== "" && (
        <Button
          type="button"
          variant="ghost"
          size="compact"
          focusStyle="inset"
          onClick={checkout.onOpen}
          className="px-module-inset text-callout text-foreground ml-auto min-w-0 gap-1.5"
          aria-label={t("checkout.openSourceControl", {
            branch: checkout.branch,
          })}
          title={t("checkout.openSourceControl", { branch: checkout.branch })}
        >
          <GitBranch className="size-3" aria-hidden="true" />
          <span className="max-w-36 truncate @lg/composer:max-w-64">
            {checkout.branch}
          </span>
          {checkout.dirty > 0 && (
            <span
              className="text-warning"
              aria-label={t("checkout.changedFiles", { count: checkout.dirty })}
            >
              {checkout.dirty}
            </span>
          )}
        </Button>
      )}
    </div>
  );
}

/** Muted section header inside a picker menu — "Model", "Reasoning". */
function MenuSection({ children }: { children: ReactNode }) {
  return (
    <p className="text-metadata text-muted-foreground shrink-0 px-2.5 pt-1.5 pb-1">
      {children}
    </p>
  );
}

/** The small "Default" pill on the adapter's own pick. */
function DefaultBadge() {
  const t = useT();
  return (
    <Badge variant="secondary" size="status">
      {t("composer.default")}
    </Badge>
  );
}

/** One row of a picker menu, normalized so the two data sources below render identically. */
interface PickerRow {
  key: string;
  label: string;
  detail?: string | null;
  isDefault: boolean;
  selected: boolean;
  select: () => void;
}

function ModelPickerRow({
  row,
  favorite,
  disabled,
  onSelect,
  onToggleFavorite,
}: {
  row: PickerRow;
  favorite: boolean;
  disabled: boolean;
  onSelect: () => void;
  onToggleFavorite: () => void;
}) {
  const t = useT();
  const favoriteLabel = t(
    favorite ? "composer.unfavoriteModel" : "composer.favoriteModel",
    { model: row.label }
  );
  return (
    <div
      data-model-picker-row
      className="gap-control-group flex min-w-0 items-center"
    >
      <div className="min-w-0 flex-1">
        <SelectableRow
          selected={row.selected}
          label={row.label}
          description={row.detail}
          meta={row.isDefault ? <DefaultBadge /> : undefined}
          disabled={disabled}
          onSelect={onSelect}
        />
      </div>
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        aria-label={favoriteLabel}
        aria-pressed={favorite}
        title={favoriteLabel}
        onClick={onToggleFavorite}
        className={cn(
          "text-muted-foreground self-center",
          favorite && "text-foreground"
        )}
      >
        <Star
          aria-hidden="true"
          className="size-3.5"
          fill={favorite ? "currentColor" : "none"}
        />
      </Button>
    </div>
  );
}

/**
 * One chip, one question.
 *
 * These used to be a single popover holding provider, working directory, permissions and two
 * isolation toggles — a settings dashboard behind every chip in the row, so clicking "Auto-edit"
 * asked you about four other things first. Each control row chip now opens only its own list.
 */
export function SessionModePicker({
  mode,
  sandbox,
  disabled = false,
  onMode,
}: {
  mode: SessionConfig["mode"];
  sandbox: SessionConfig["sandbox"];
  disabled?: boolean;
  onMode: SessionConfig["onSessionMode"];
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const active = sessionMode(mode, sandbox);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Chip
            tone={active === "full_access" ? "warning" : undefined}
            title={t("config.mode")}
            disabled={disabled}
            aria-busy={disabled}
            aria-label={`${t("config.mode")}: ${t(`mode.${active}`)}`}
            className={cn(
              "text-body",
              active !== "full_access" && "text-foreground",
              disabled && "cursor-wait opacity-60 hover:bg-transparent"
            )}
          >
            {active === "full_access" ? (
              <LockOpen className="size-3 shrink-0" />
            ) : (
              <Lock className="size-3 shrink-0" />
            )}
            <span>{t(`mode.${active}`)}</span>
            <ChevronDown className="size-3 shrink-0" />
          </Chip>
        }
      />
      <PopoverContent align="start" side="top" className="w-64 p-1.5">
        <MenuSection>{t("config.mode")}</MenuSection>
        {SESSION_MODES.map((m) => (
          <SelectableRow
            key={m.id}
            selected={m.id === active}
            label={t(`mode.${m.id}`)}
            description={t(`mode.${m.id}Hint`)}
            disabled={disabled}
            onSelect={() => {
              onMode(m.id);
              setOpen(false);
            }}
          />
        ))}
      </PopoverContent>
    </Popover>
  );
}

export function ModePicker({ config }: { config: SessionConfig }) {
  return (
    <SessionModePicker
      mode={config.mode}
      sandbox={config.sandbox}
      disabled={config.modeChangeDisabled}
      onMode={config.onSessionMode}
    />
  );
}

export function MemoryPicker({ config }: { config: SessionConfig }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const presets = memoryPresetsForProvider(config.provider);
  const active =
    presets.find(
      (preset) =>
        preset.read === config.memoryRead && preset.write === config.memoryWrite
    ) ?? presets[0];

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Chip
            title={t("config.memory")}
            aria-label={`${t("config.memory")}: ${t(`memory.preset.${active.id}`)}`}
          >
            <BrainCircuit className="size-3.5 shrink-0" />
            <span>{t(`memory.preset.${active.id}`)}</span>
            <ChevronDown className="size-3 shrink-0" />
          </Chip>
        }
      />
      <PopoverContent align="start" side="top" className="w-72 p-1.5">
        <MenuSection>{t("config.memory")}</MenuSection>
        {presets.map((preset) => (
          <SelectableRow
            key={preset.id}
            selected={preset.id === active.id}
            label={t(`memory.preset.${preset.id}`)}
            description={t(`memory.preset.${preset.id}Hint`)}
            meta={preset.isDefault === true ? <DefaultBadge /> : undefined}
            onSelect={() => {
              config.onMemoryPolicy(preset.read, preset.write);
              setOpen(false);
            }}
          />
        ))}
      </PopoverContent>
    </Popover>
  );
}

const WORKTREE_BASELINES = ["current", "origin_default"] as const;

/** Worktree isolation is a baseline choice, not a boolean: both commit sources stay explicit. */
export function WorktreePicker({ config }: { config: SessionConfig }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  // No git repository, no picker: every baseline is unavailable, so the trigger greys out with
  // the core's reason as its tooltip and the draft stays Off. Sessions are never gated — their
  // recorded worktree state renders regardless of what the current directory looks like.
  const gatingReason = worktreeGatingReason(
    config.hasSession,
    config.worktreeOptions,
    config.worktreeOptionsLoading
  );
  useEffect(() => {
    // A project default (or a leftover draft choice) may still name a baseline; snap it back to
    // Off so the disabled trigger and the state creation would use never disagree.
    if (gatingReason !== null && config.worktreeBase !== null)
      config.onWorktreeBase(null);
  }, [gatingReason, config.worktreeBase, config.onWorktreeBase]);
  const selectedKind = config.hasSession
    ? (config.activeWorktreeBaseline?.kind ?? null)
    : config.worktreeBase;
  const selected =
    selectedKind == null
      ? null
      : config.worktreeOptions.find((option) => option.kind === selectedKind);
  const selectedUnavailable =
    !config.hasSession &&
    selectedKind != null &&
    !config.worktreeOptionsLoading &&
    selected?.resolved == null;
  const compactLabel = config.activeWorktreeUnknown
    ? t("worktree.legacyUnknown")
    : config.hasSession && config.activeWorktreeBaseline
      ? config.activeWorktreeBaseline.display
      : selectedKind == null || gatingReason !== null
        ? t("worktree.off")
        : t(`worktree.${selectedKind}`);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Chip
            tone={
              selectedUnavailable && gatingReason === null
                ? "warning"
                : undefined
            }
            title={gatingReason ?? t("config.worktreeHint")}
            aria-label={`${t("config.worktree")}: ${compactLabel}`}
            aria-expanded={open}
            disabled={gatingReason !== null}
            className={cn(
              selectedKind != null &&
                !selectedUnavailable &&
                "text-primary hover:text-primary",
              "disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent"
            )}
          >
            <GitBranch className="size-3.5 shrink-0" />
            <span className="max-w-36 truncate">{compactLabel}</span>
            <ChevronDown className="size-3 shrink-0" />
          </Chip>
        }
      />
      <PopoverContent align="center" side="top" className="w-80 p-1.5">
        <MenuSection>{t("config.worktree")}</MenuSection>
        {config.hasSession ? (
          <>
            <SelectableRow
              selected
              label={
                config.activeWorktreeUnknown
                  ? t("worktree.legacyUnknown")
                  : selectedKind == null
                    ? t("worktree.off")
                    : t(`worktree.${selectedKind}`)
              }
              description={
                config.activeWorktreeUnknown
                  ? t("worktree.legacyUnknownHint")
                  : (config.activeWorktreeBaseline?.display ??
                    t("worktree.offHint"))
              }
              disabled
              onSelect={() => {
                /* empty */
              }}
            />
            <p className="text-callout text-muted-foreground px-2.5 pt-2 pb-1">
              {t("worktree.fixedForSession")}
            </p>
          </>
        ) : (
          <>
            <SelectableRow
              selected={config.worktreeBase == null}
              label={t("worktree.off")}
              description={t("worktree.offHint")}
              onSelect={() => {
                config.onWorktreeBase(null);
                setOpen(false);
              }}
            />
            {WORKTREE_BASELINES.map((kind) => {
              const option = config.worktreeOptions.find(
                (candidate) => candidate.kind === kind
              );
              const unavailable =
                !config.worktreeOptionsLoading && option?.resolved == null;
              const detail = config.worktreeOptionsLoading
                ? t("worktree.resolving")
                : option?.resolved
                  ? `${option.resolved.display} · ${t("worktree.localOnly")}`
                  : (option?.unavailable_reason ?? t("worktree.unavailable"));
              return (
                <SelectableRow
                  key={kind}
                  selected={config.worktreeBase === kind}
                  label={t(`worktree.${kind}`)}
                  description={detail}
                  disabled={unavailable}
                  onSelect={() => {
                    config.onWorktreeBase(kind);
                    setOpen(false);
                  }}
                />
              );
            })}
          </>
        )}
        <p className="text-callout text-muted-foreground px-2.5 pt-2 pb-1">
          {t("worktree.noFetch")}
        </p>
      </PopoverContent>
    </Popover>
  );
}

/**
 * The model this turn will run on: a model chip, and an effort chip when the model comes in
 * reasoning variants.
 *
 * Agents describe this two different ways, so the picker reads both and renders one menu.
 *
 * Newer adapters (claude-agent-acp, codex-acp) report *selectors* as session config options — a
 * "model" one and a "thought_level" one — and those are taken at face value: no parsing, and
 * effort is independent of the model, which is the only way it works for Claude Code.
 *
 * Older adapters report a flat model list instead, and encode effort by minting one entry per
 * level ("gpt-5.1-codex low/medium/high"). `groupModels` folds those back into families: the first
 * chip picks the family, the second the effort, and together they resolve to one of the adapter's
 * own ids.
 *
 * Both APIs are optional. Before session creation only CLI-discovered choices are available;
 * otherwise the provider owns its default and can report model/config choices after connection.
 */
export function ModelPicker({
  models,
  current,
  defaultModel,
  provider,
  onModel,
  configOptions,
  onConfigOption,
  hasSession,
  providerConfig,
  showWhenUnavailable = false,
  disabled = false,
}: {
  models: ModelChoice[];
  current: string | null;
  defaultModel: string | null;
  provider: string;
  onModel: (id: string) => void;
  configOptions: ConfigOptionInfo[];
  onConfigOption: (configId: string, value: string) => void;
  hasSession: boolean;
  /** Primary Composer only: fold Provider browsing into the model surface. */
  providerConfig?: SessionConfig;
  /** Keep an explicit model affordance while a host surface is waiting for provider metadata. */
  showWhenUnavailable?: boolean;
  /** A live turn owns its provider runtime; model and effort changes wait until it ends. */
  disabled?: boolean;
}) {
  const t = useT();
  const [modelOpen, setModelOpen] = useState(false);
  const [modelSearch, setModelSearch] = useState("");
  const [effortOpen, setEffortOpen] = useState(false);
  const [browseProvider, setBrowseProvider] = useState(provider);
  const modelTriggerRef = useRef<HTMLButtonElement>(null);
  const providerSwitcherEnabled = providerConfig !== undefined;
  const providerRegistry = providerConfig?.providers ?? [];
  const providerChoices = providerConfig
    ? providerRegistry.filter(
        (candidate) =>
          candidate.enabled || candidate.id === providerConfig.provider
      )
    : [];
  const pickerProvider = providerSwitcherEnabled ? browseProvider : provider;
  const pickerProviderInfo = providerChoices.find(
    (candidate) => candidate.id === pickerProvider
  );
  const browsingCurrentProvider = pickerProvider === provider;
  const pickerModels = browsingCurrentProvider
    ? models
    : (pickerProviderInfo?.models ?? []);
  const pickerConfigOptions = browsingCurrentProvider ? configOptions : [];
  const pickerCurrent = browsingCurrentProvider ? current : null;
  const pickerDefaultModel = browsingCurrentProvider ? defaultModel : null;
  const selectPickerModel = (id: string) => {
    if (providerConfig && pickerProvider !== provider) {
      providerConfig.onProviderModel(pickerProvider, id);
      return;
    }
    onModel(id);
  };
  const families = groupModels(models);
  const pickerFamilies = groupModels(pickerModels);
  const { favorites, toggle: toggleFavorite } =
    useProviderModelFavorites(pickerProvider);
  const { hidden: hiddenModels } = useProviderModelPreferences(pickerProvider);
  useEffect(() => {
    if (disabled) {
      setModelOpen(false);
      setEffortOpen(false);
    }
  }, [disabled]);
  useEffect(() => {
    if (!modelOpen) setBrowseProvider(provider);
  }, [modelOpen, provider]);
  useEffect(() => {
    if (!providerSwitcherEnabled) return;
    const openProviderModelPicker = () => {
      setBrowseProvider(provider);
      setModelOpen(true);
      window.setTimeout(() => modelTriggerRef.current?.focus(), 0);
    };
    window.addEventListener(
      "codetwo-open-provider-picker",
      openProviderModelPicker
    );
    return () =>
      window.removeEventListener(
        "codetwo-open-provider-picker",
        openProviderModelPicker
      );
  }, [provider, providerSwitcherEnabled]);
  if (
    !providerSwitcherEnabled &&
    !hasSession &&
    models.length === 0 &&
    !showWhenUnavailable
  )
    return null;

  const effortName = (e: Effort | null) =>
    e ? t(`effort.${e}`) : t("composer.default");

  const modelOpt = configOptions.find(
    (o) => o.category === "model" || o.id === "model"
  );
  const pickerModelOpt = pickerConfigOptions.find(
    (o) => o.category === "model" || o.id === "model"
  );
  const effortOpt = configOptions.find(
    (o) =>
      o.category === "thought_level" ||
      o.id === "effort" ||
      o.id === "reasoning_effort"
  );
  const activeFamily = familyOf(families, current);
  const activeVariant = variantOf(families, current);
  const pickerActiveFamily = familyOf(pickerFamilies, pickerCurrent);
  const pickerActiveVariant = variantOf(pickerFamilies, pickerCurrent);

  let modelLabel: string;
  let modelRows: PickerRow[];
  let effortLabel = "";
  let effortRows: PickerRow[] = [];

  if (providerConfig?.providerSwitching === true) {
    modelLabel = t("composer.switchingProvider");
  } else if (modelOpt) {
    // Keep the trigger anchored to the active Provider even while the popup browses another one.
    modelLabel =
      (modelOpt.choices.find((c) => c.id === modelOpt.current)?.name ??
        modelOpt.current) ||
      t("composer.defaultModel");
  } else {
    const active = models.find((m) => m.id === current);
    modelLabel =
      activeFamily?.label ??
      active?.name ??
      current ??
      t("composer.defaultModel");
  }

  if (pickerModelOpt) {
    // The active adapter described its own selector; preserve that provider-owned path.
    modelRows = pickerModelOpt.choices.map((c) => ({
      key: c.id,
      label: c.name,
      detail: c.description,
      isDefault: c.id === pickerDefaultModel,
      selected: c.id === pickerModelOpt.current,
      select: () => onConfigOption(pickerModelOpt.id, c.id),
    }));
  } else {
    // Flat lists from either the active runtime or another registry Provider share one projection.
    modelRows = pickerFamilies.map((f) => ({
      key: f.key,
      label: f.label,
      detail: f.variants[0]?.choice.description,
      isDefault: f.variants.some((v) => v.choice.id === pickerDefaultModel),
      selected: f === pickerActiveFamily,
      select: () =>
        selectPickerModel(
          pickVariant(
            f,
            pickerActiveVariant?.effort ?? null,
            pickerDefaultModel
          ).id
        ),
    }));
  }

  // Effort is independently provider-owned. Some ACP agents (notably Grok) report a normal model
  // list plus a separate metadata-derived effort option, rather than a model config option. Never
  // discard that real ladder just because the model selector arrived through the older surface.
  if (effortOpt) {
    effortLabel =
      effortOpt.choices.find((c) => c.id === effortOpt.current)?.name ??
      effortOpt.current;
    effortRows = effortOpt.choices.map((c) => ({
      key: c.id,
      label: c.name,
      detail: c.description,
      isDefault: false,
      selected: c.id === effortOpt.current,
      select: () => onConfigOption(effortOpt.id, c.id),
    }));
  } else if (activeFamily) {
    effortLabel = effortName(activeVariant?.effort ?? null);
    effortRows = activeFamily.variants.map((v) => ({
      key: v.choice.id,
      label: effortName(v.effort),
      isDefault: v.choice.id === defaultModel,
      selected: v.choice.id === current,
      select: () => onModel(v.choice.id),
    }));
  }

  const favoriteRows: PickerRow[] = [];
  const regularRows: PickerRow[] = [];
  const normalizedSearch = modelSearch.trim().toLocaleLowerCase();
  const visibleModelRows = modelRows.filter(
    (row) => row.selected || !hiddenModels.has(row.key)
  );
  const filteredModelRows = normalizedSearch
    ? visibleModelRows.filter((row) =>
        `${row.label}\n${row.key}\n${row.detail ?? ""}`
          .toLocaleLowerCase()
          .includes(normalizedSearch)
      )
    : visibleModelRows;
  for (const row of filteredModelRows)
    (favorites.has(row.key) ? favoriteRows : regularRows).push(row);
  const renderModelRow = (row: PickerRow) => (
    <ModelPickerRow
      key={row.key}
      row={row}
      favorite={favorites.has(row.key)}
      disabled={disabled}
      onSelect={() => {
        row.select();
        setModelOpen(false);
      }}
      onToggleFavorite={() => toggleFavorite(row.key)}
    />
  );
  const modelMenu =
    modelRows.length === 0 ? (
      <p className="text-callout text-muted-foreground px-2 py-2">
        {t("composer.noModels")}
      </p>
    ) : (
      <>
        <SearchField
          autoFocus
          label={t("composer.searchModels")}
          placeholder={t("composer.searchModels")}
          value={modelSearch}
          clearLabel={t("composer.clearModelSearch")}
          onClear={() => setModelSearch("")}
          onChange={(event) => setModelSearch(event.target.value)}
          className="mb-1"
        />
        <div
          data-model-picker-list
          className="max-h-80 min-h-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-contain"
        >
          {filteredModelRows.length === 0 ? (
            <p className="text-fine text-muted-foreground px-2 py-3 text-center">
              {normalizedSearch
                ? t("composer.noMatchingModels")
                : t("composer.noVisibleModels")}
            </p>
          ) : favoriteRows.length > 0 ? (
            <>
              <MenuSection>{t("composer.favorites")}</MenuSection>
              {favoriteRows.map(renderModelRow)}
              {regularRows.length > 0 ? (
                <MenuSection>{t("composer.model")}</MenuSection>
              ) : null}
              {regularRows.map(renderModelRow)}
            </>
          ) : (
            <>
              <MenuSection>{t("composer.model")}</MenuSection>
              {filteredModelRows.map(renderModelRow)}
            </>
          )}
        </div>
      </>
    );

  return (
    <>
      <Popover
        open={modelOpen}
        onOpenChange={(open) => {
          setModelOpen(disabled ? false : open);
          if (!open) setModelSearch("");
        }}
      >
        <PopoverTrigger
          render={
            <Chip
              ref={modelTriggerRef}
              title={t("composer.model")}
              disabled={disabled}
              aria-busy={disabled}
              className="text-body text-foreground disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent"
            >
              <ProviderIcon provider={provider} className="size-3.5 shrink-0" />
              <span
                role={
                  providerConfig?.providerSwitching === true
                    ? "status"
                    : undefined
                }
                title={modelLabel}
                className="max-w-40 truncate @lg/composer:max-w-56"
              >
                {modelLabel}
              </span>
              <ChevronDown className="size-3 shrink-0" />
            </Chip>
          }
        />
        <PopoverContent
          align="start"
          side="top"
          className={cn(
            "flex max-h-(--available-height) overflow-hidden",
            providerSwitcherEnabled
              ? "w-menu-wide max-w-(--available-width) flex-col p-0"
              : "w-64 flex-col p-1.5"
          )}
        >
          {providerConfig ? (
            <div
              data-provider-model-picker
              className="flex min-h-72 min-w-0 flex-1"
            >
              <div
                data-provider-switcher
                role="listbox"
                aria-label={t("config.provider")}
                aria-orientation="vertical"
                className="bg-fill-quiet flex w-12 shrink-0 flex-col items-center gap-1 overflow-y-auto p-2"
              >
                {providerChoices.map((candidate) => {
                  const selected = candidate.id === pickerProvider;
                  const unavailable =
                    providerConfig.providersStatus === "ready" &&
                    !candidate.available;
                  const displayName =
                    candidate.id === "codex" ? "Codex" : candidate.display_name;
                  return (
                    <Button
                      key={candidate.id}
                      type="button"
                      variant="selectable"
                      size="compact"
                      role="option"
                      aria-label={displayName}
                      title={displayName}
                      aria-selected={selected}
                      data-selected={selected ? "true" : "false"}
                      disabled={
                        disabled ||
                        unavailable ||
                        providerConfig.providersStatus !== "ready"
                      }
                      className="relative size-8 shrink-0 rounded-full p-0"
                      onClick={() => {
                        setModelSearch("");
                        if (
                          candidate.id !== provider &&
                          candidate.models.length === 0 &&
                          providerConfig.providersStatus === "ready" &&
                          candidate.available
                        ) {
                          providerConfig.onProviderModel(candidate.id, null);
                          setModelOpen(false);
                          return;
                        }
                        setBrowseProvider(candidate.id);
                      }}
                    >
                      <ProviderIcon
                        provider={candidate.id}
                        className={cn("size-3.5", unavailable && "opacity-40")}
                      />
                      <span
                        aria-hidden="true"
                        className={cn(
                          "absolute right-0.5 bottom-0.5 size-1.5 rounded-full",
                          unavailable ? "bg-border" : "bg-success"
                        )}
                      />
                    </Button>
                  );
                })}
              </div>
              <div className="flex min-h-0 min-w-0 flex-1 flex-col p-3">
                {providerConfig.providersStatus === "loading" ? (
                  <p
                    role="status"
                    className="text-callout text-muted-foreground px-2 pb-2"
                  >
                    {t("config.providersLoading")}
                  </p>
                ) : null}
                {providerConfig.providersStatus === "error" ? (
                  <div className="rounded-control bg-fill-quiet px-module-inset text-callout mb-1 flex items-center gap-2 py-2">
                    <span
                      role="alert"
                      className="text-muted-foreground min-w-0 flex-1"
                    >
                      {t("config.providersLoadFailed")}
                    </span>
                    <Button
                      type="button"
                      variant="link"
                      size="compact"
                      className="text-foreground shrink-0 px-0 font-medium"
                      onClick={providerConfig.onReloadProviders}
                    >
                      {t("config.retryProviders")}
                    </Button>
                  </div>
                ) : null}
                {modelMenu}
              </div>
            </div>
          ) : (
            modelMenu
          )}
        </PopoverContent>
      </Popover>

      {effortRows.length > 1 && (
        <Popover
          open={effortOpen}
          onOpenChange={(open) => setEffortOpen(disabled ? false : open)}
        >
          <PopoverTrigger
            render={
              <Chip
                title={t("composer.reasoning")}
                disabled={disabled}
                aria-busy={disabled}
                className="text-body text-foreground disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent"
              >
                <span>{effortLabel}</span>
                <ChevronDown className="size-3 shrink-0" />
              </Chip>
            }
          />
          <PopoverContent align="start" side="top" className="w-44 p-1.5">
            <MenuSection>{t("composer.reasoning")}</MenuSection>
            {effortRows.map((r) => (
              <SelectableRow
                key={r.key}
                selected={r.selected}
                label={r.label}
                description={r.detail}
                meta={r.isDefault ? <DefaultBadge /> : undefined}
                disabled={disabled}
                onSelect={() => {
                  r.select();
                  setEffortOpen(false);
                }}
              />
            ))}
          </PopoverContent>
        </Popover>
      )}
    </>
  );
}

/** High-frequency, session-scoped configuration stays one click from the prompt. */
export function SessionControls({
  config,
  models,
  currentModel,
  defaultModel,
  onModel,
  configOptions,
  onConfigOption,
  modelChangeDisabled = false,
  showWorktreePicker = true,
}: {
  config: SessionConfig;
  models: ModelChoice[];
  currentModel: string | null;
  defaultModel: string | null;
  onModel: (id: string) => void;
  configOptions: ConfigOptionInfo[];
  onConfigOption: (configId: string, value: string) => void;
  modelChangeDisabled?: boolean;
  /** The checkout footer owns this choice when present. */
  showWorktreePicker?: boolean;
}) {
  const t = useT();
  const optionsId = useId();
  const [optionsOpen, setOptionsOpen] = useState(false);
  const hasSecondaryOptions = config.memoryEnabled || showWorktreePicker;
  const hasHiddenOverride =
    (config.memoryEnabled &&
      (config.memoryRead !== "inherit" || config.memoryWrite !== "inherit")) ||
    (showWorktreePicker &&
      (config.activeWorktreeUnknown ||
        config.activeWorktreeBaseline !== null ||
        config.worktreeBase !== null));

  return (
    <div
      data-session-controls
      className="flex min-w-0 flex-[1_1_20rem] flex-col items-start gap-1"
    >
      <div className="flex max-w-full flex-wrap items-center gap-1">
        {config.scenesEnabled ? <SceneChip config={config} /> : null}
        <ModelPicker
          models={models}
          current={currentModel}
          defaultModel={defaultModel}
          provider={config.provider}
          onModel={onModel}
          configOptions={configOptions}
          onConfigOption={onConfigOption}
          hasSession={config.hasSession}
          providerConfig={config}
          disabled={modelChangeDisabled || config.providerChangeDisabled}
        />
        <ModePicker config={config} />
        {hasSecondaryOptions && (
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className={cn(
                    "text-foreground relative size-7 shrink-0 rounded-full",
                    optionsOpen && "bg-accent text-foreground"
                  )}
                  aria-label={t(
                    optionsOpen
                      ? "config.hideSessionOptions"
                      : "config.showSessionOptions"
                  )}
                  aria-expanded={optionsOpen}
                  aria-controls={optionsId}
                  onClick={() => setOptionsOpen((open) => !open)}
                >
                  <SlidersHorizontal className="size-3.5" />
                  {hasHiddenOverride ? (
                    <span
                      aria-hidden="true"
                      className="bg-primary absolute top-0.5 right-0.5 size-1.5 rounded-full"
                    />
                  ) : null}
                </Button>
              }
            />
            <TooltipContent>{t("config.sessionOptionsHint")}</TooltipContent>
          </Tooltip>
        )}
      </div>
      {hasSecondaryOptions && optionsOpen ? (
        <div
          id={optionsId}
          data-session-options
          className="flex max-w-full flex-wrap items-center gap-0.5 pl-1"
        >
          {config.memoryEnabled ? <MemoryPicker config={config} /> : null}
          {showWorktreePicker ? <WorktreePicker config={config} /> : null}
        </div>
      ) : null}
    </div>
  );
}

/**
 * The prompt composer.
 *
 * Two shapes, one document. Compact, it's docked to the foot of the transcript and reads like a
 * chat box — that's the point, it's where you type. Expanded, it *becomes the page*: the card
 * chrome falls away and the document runs the full column on the app's own background, with a
 * centred measure and a real block gutter. It is the same BlockNote document in both — headings,
 * lists, code, `/` skills and `@` files work throughout.
 */
/** The one circular send control: the full Composer and the chief-of-staff conversation share it. */
export function ComposerSendButton({
  empty,
  loading = false,
  disabled = false,
  label,
  tooltip,
  hint,
  onClick,
}: {
  empty: boolean;
  loading?: boolean;
  disabled?: boolean;
  label: string;
  tooltip: string;
  hint?: ReactNode;
  onClick: () => void;
}) {
  return (
    <Tooltip>
      {/* Kept enabled on purpose: a disabled button explains nothing, and clicking it
        focuses the document and says what's missing. */}
      <TooltipTrigger
        render={
          <Button
            size="icon"
            variant={empty ? "secondary" : "default"}
            className="size-8 shrink-0 rounded-full"
            onClick={onClick}
            disabled={loading || disabled}
            aria-label={label}
          >
            {loading ? (
              <ActivityOrb state="connecting" aria-hidden="true" />
            ) : (
              <ArrowUp className="size-4" />
            )}
          </Button>
        }
      />
      <TooltipContent>
        {tooltip}
        {!loading && hint != null && (
          <span className="ml-1.5 opacity-60">{hint}</span>
        )}
      </TooltipContent>
    </Tooltip>
  );
}

/**
 * The composer card chrome (surface, editor scroll box, control row) without any session
 * configuration. The chief-of-staff conversation uses it so its input is the same surface as
 * the main Composer without requiring a session, model or checkout.
 */
export function ComposerCard({
  children,
  controls,
  maxHeight = 190,
  docMode = false,
  attachments,
  overlay,
  footer,
}: {
  children: ReactNode;
  controls: ReactNode;
  maxHeight?: number;
  docMode?: boolean;
  attachments?: ReactNode;
  overlay?: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <div
      className={cn(
        // The width container the control row compresses against (see the chip labels below):
        // in compact mode this is the card's own measure, expanded it's the page column — either
        // way, the width the controls actually have.
        "@container/composer isolate flex flex-col",
        docMode ? "min-h-0 flex-1" : "mx-auto w-full max-w-3xl"
      )}
    >
      {/* No `overflow-hidden`: BlockNote's drag/insert handles render just outside the text
            column, and clipping them takes the block gutter away. The compact card paints its own
            surface so its background can never drift away from the editor in WebKit. */}
      <div
        className={cn(
          "composer-card relative z-10 flex flex-col",
          docMode
            ? // Expanded, the composer *is* the page: no card, no border, the app's own surface.
              // `relative` anchors the floating control bar below.
              "min-h-0 flex-1"
            : "rounded-composer bg-canvas shadow-composer dark:bg-raised"
        )}
      >
        <div
          className={cn(
            "min-h-0 overflow-y-auto",
            docMode ? "bn-doc-mode flex-1" : "min-h-28 py-4"
          )}
          style={docMode ? undefined : { maxHeight }}
        >
          {attachments}
          {children}
        </div>

        {/* R5: an empty page in an active scene with a brief offers to start from it. A
              positioned overlay inside the same tree (see the reconciliation note above) — it
              never auto-inserts, and dismissing it keeps it away for this session. Only rendered
              in doc mode, where the card is `relative`. */}
        {overlay}

        {/* Expanded, the control rows *float* over the foot of the page as their own raised card.
              In normal flow it sat at the column's bottom edge, where the transcript panel beside
              the page ended up over the run button and swallowed its clicks; floating on its own
              z-plane keeps every control clickable no matter what the layout around the page does.
              The session configuration row wraps independently, keeping its high-frequency
              controls visible without crowding the run, stop, voice, or document controls.
              `pointer-events-none` on the strip, `auto` on the card: the page stays clickable
              either side of the floating bar. */}
        <div
          className={cn(
            docMode &&
              "pointer-events-none absolute inset-x-0 bottom-5 z-20 px-6"
          )}
        >
          <div
            className={cn(
              "flex min-w-0 flex-wrap items-end gap-1",
              docMode
                ? "rounded-composer bg-canvas shadow-composer dark:bg-raised pointer-events-auto mx-auto w-full max-w-3xl p-2"
                : // Keep every outer edge 8px from the controls. The 24px surface radius then
                  // shares its bottom-right centre with the circular send/stop control.
                  "p-2"
            )}
          >
            {controls}
          </div>
        </div>
      </div>
      {footer}
    </div>
  );
}

export function Composer({
  children,
  config,
  checkout,
  docMode,
  onDocMode,
  boundsRef,
  models,
  currentModel,
  defaultModel,
  contextWindow,
  usage = null,
  onCompactContext,
  onModel,
  configOptions,
  onConfigOption,
  running,
  loading,
  docEmpty,
  appshots = [],
  onRemoveAppshot,
  onRun,
  onQueue,
  onMultitask,
  onSteer,
  onStop,
  steeringSupported,
  onAttachFile,
  onAttachImages,
  onInsertSkill,
  onInsertIssue,
  onOpenMarket,
  onNewSkill,
  canvasEnabled,
  onInsertCanvas,
  voiceEnabled,
  onVoiceText,
  onVoiceTranscript,
  runHint,
  skillHint,
  filesHint,
  pluginActions,
  sessionId,
  insertBriefRef,
}: ComposerProps) {
  const t = useT();
  const imageInputRef = useRef<HTMLInputElement>(null);

  // ---- R5 scene brief: offer banner, + menu entry, required-slot hint --------------------------
  // Dismissal is remembered per session for the Composer's lifetime — a dismissed offer must not
  // come back on every keystroke-then-clear, but a new session gets a fresh offer.
  const briefDismissedRef = useRef(new Set<string>());
  const [, bumpBriefDismissals] = useState(0);
  const sessionKey = sessionId ?? "draft";
  const composerEmpty = docEmpty && appshots.length === 0;
  const activeBrief = config.activeScene?.brief ?? null;
  const insertBrief = () => {
    const scene = config.activeScene;
    if (scene?.brief) insertBriefRef?.current?.(scene);
  };
  const dismissBrief = () => {
    briefDismissedRef.current.add(sessionKey);
    bumpBriefDismissals((n) => n + 1);
  };
  const showBriefOffer = briefOfferVisible({
    docMode,
    docEmpty: composerEmpty,
    hasBrief: activeBrief !== null,
    dismissed: briefDismissedRef.current.has(sessionKey),
  });

  // Required slot-card fields still empty — published by the editor on document change (same
  // window-event seam as `codetwo-open-provider-picker`). A warning near Run, never a block.
  const [unfilledRequired, setUnfilledRequired] = useState<string[]>([]);
  useEffect(() => {
    const onRequiredSlots = (event: Event) => {
      const { detail } = event as CustomEvent<string[]>;
      setUnfilledRequired(Array.isArray(detail) ? detail : []);
    };
    window.addEventListener("codetwo-required-slots", onRequiredSlots);
    return () =>
      window.removeEventListener("codetwo-required-slots", onRequiredSlots);
  }, []);
  // Leave room for at least a few transcript lines, whatever the window size — including when a
  // height saved on a tall window is restored on a short one. Only the applied height is clamped;
  // the saved preference comes back in full once there's room again.
  const [maxHeight, setMaxHeight] = useState(400);
  useEffect(() => {
    const measure = () => {
      const column = boundsRef.current?.getBoundingClientRect().height ?? 720;
      setMaxHeight(Math.max(72, column - 180));
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [boundsRef]);
  const applied = Math.min(190, maxHeight);

  const controls = (
    <>
      <input
        ref={imageInputRef}
        type="file"
        accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml"
        multiple
        hidden
        onChange={(event) => {
          const files = [...(event.currentTarget.files ?? [])];
          event.currentTarget.value = "";
          if (files.length > 0) void onAttachImages(files);
        }}
      />
      <SessionControls
        config={config}
        models={models}
        currentModel={currentModel}
        defaultModel={defaultModel}
        onModel={onModel}
        configOptions={configOptions}
        onConfigOption={onConfigOption}
        modelChangeDisabled={running || loading}
        showWorktreePicker={!checkout}
      />

      <div
        data-composer-actions
        className="ml-auto flex max-w-full flex-wrap items-center justify-end gap-0.5"
      >
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                className="size-7 shrink-0 rounded-full"
                aria-label={t("composer.add")}
              >
                <Paperclip className="size-4" />
              </Button>
            }
          />
          <DropdownMenuContent align="start" side="top" className="w-60">
            <DropdownMenuGroup>
              <DropdownMenuItem onClick={onAttachFile}>
                <FileText />
                {t("composer.mentionFile")}
                {filesHint && (
                  <DropdownMenuShortcut>{filesHint}</DropdownMenuShortcut>
                )}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => imageInputRef.current?.click()}>
                <ImagePlus />
                {t("composer.attachImage")}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={onInsertSkill}>
                <Sparkles />
                {t("composer.insertSkill")}
                {skillHint && (
                  <DropdownMenuShortcut>{skillHint}</DropdownMenuShortcut>
                )}
              </DropdownMenuItem>
              {canvasEnabled && (
                <DropdownMenuItem onClick={onInsertCanvas}>
                  <PenLine />
                  {t("composer.insertCanvas")}
                </DropdownMenuItem>
              )}
              <DropdownMenuItem onClick={onInsertIssue}>
                <Ticket />
                {t("composer.pullIssue")}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={onOpenMarket}>
                <Store />
                {t("composer.market")}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={onNewSkill}>
                <Sparkles />
                {t("composer.newSkill")}
              </DropdownMenuItem>
              {/* With content already in the document the floating offer stays away; the brief is
                still one menu entry away while a scene with one is active. */}
              {activeBrief && !composerEmpty && (
                <DropdownMenuItem onClick={insertBrief}>
                  <ListChecks />
                  {t("brief.menuInsert")}
                </DropdownMenuItem>
              )}
            </DropdownMenuGroup>
            <p className="text-callout text-muted-foreground px-2 pt-1.5 pb-1">
              {t("composer.addHint")}
            </p>
          </DropdownMenuContent>
        </DropdownMenu>

        <Statusline
          contextWindow={contextWindow}
          usage={usage ?? null}
          onCompact={onCompactContext}
          compactDisabled={running || loading || !composerEmpty}
          compactDisabledReason={
            running || loading
              ? t("context.compactBusy")
              : composerEmpty
                ? null
                : t("context.compactDraft")
          }
        />

        {pluginActions}

        {/* Document mode is the app's own feature — it deserves a control you can see, not just a
          chord and a grip gesture. */}
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                className="text-foreground size-7 shrink-0 rounded-full"
                aria-label={
                  docMode
                    ? t("composer.collapseLabel")
                    : t("composer.expandLabel")
                }
                onClick={() => onDocMode(!docMode)}
              >
                {docMode ? (
                  <Minimize2 className="size-3.5" />
                ) : (
                  <Maximize2 className="size-3.5" />
                )}
              </Button>
            }
          />
          <TooltipContent>
            {docMode ? t("composer.collapse") : t("composer.expand")}
          </TooltipContent>
        </Tooltip>

        {voiceEnabled ? (
          <VoiceButton onText={onVoiceText} onTranscript={onVoiceTranscript} />
        ) : null}

        {/* Required slot fields still empty — a hint beside Run, never a gate on it. */}
        {unfilledRequired.length > 0 && (
          <Tooltip>
            <TooltipTrigger
              render={
                <Chip
                  tone="warning"
                  aria-label={t("slotCard.requiredWarning", {
                    slots: unfilledRequired.join(", "),
                  })}
                >
                  <TriangleAlert className="size-3.5 shrink-0" />
                  <span className="hidden @lg/composer:inline">
                    {t("slotCard.requiredShort", {
                      count: unfilledRequired.length,
                    })}
                  </span>
                </Chip>
              }
            />
            <TooltipContent>
              {t("slotCard.requiredWarning", {
                slots: unfilledRequired.join(", "),
              })}
            </TooltipContent>
          </Tooltip>
        )}

        {running ? (
          <>
            <div className="rounded-control bg-background flex shrink-0 items-center border">
              <Tooltip>
                <TooltipTrigger
                  render={
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-8 rounded-full"
                      onClick={onQueue}
                      aria-label={t("composer.queue")}
                    >
                      <ArrowUp className="size-4" />
                    </Button>
                  }
                />
                <TooltipContent>{t("composer.queue")}</TooltipContent>
              </Tooltip>
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <Button
                      variant="ghost"
                      size="icon"
                      className="text-muted-foreground size-7 rounded-full"
                      aria-label={t("composer.sendOptions")}
                    >
                      <ChevronDown className="size-3.5" />
                    </Button>
                  }
                />
                <DropdownMenuContent align="end" side="top">
                  <DropdownMenuItem onClick={onQueue}>
                    {t("composer.queue")}
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={onMultitask}>
                    {t("composer.multitask")}
                  </DropdownMenuItem>
                  {steeringSupported ? (
                    <DropdownMenuItem onClick={onSteer}>
                      {t("composer.steer")}
                    </DropdownMenuItem>
                  ) : null}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    variant="secondary"
                    size="icon"
                    className="size-8 shrink-0 rounded-full"
                    onClick={onStop}
                    aria-label={t("composer.stop")}
                  >
                    <Square className="size-3.5 fill-current" />
                  </Button>
                }
              />
              <TooltipContent>{t("composer.stop")}</TooltipContent>
            </Tooltip>
          </>
        ) : (
          <ComposerSendButton
            empty={composerEmpty}
            loading={loading}
            label={loading ? t("composer.loadingSession") : t("composer.run")}
            tooltip={
              loading
                ? t("composer.loadingSession")
                : composerEmpty
                  ? t("composer.runEmpty")
                  : t("composer.run")
            }
            hint={runHint}
            onClick={onRun}
          />
        )}
      </div>
    </>
  );

  /*
   * Compact and expanded are the *same* element tree with different classes, deliberately. Two
   * branches returning different JSX would give React a different structure to reconcile, which
   * unmounts BlockNote and takes the draft with it — expanding the composer would silently erase
   * what you'd written. Keep one tree; vary the styling.
   */
  return (
    <section
      data-composer-mode={docMode ? "document" : "compact"}
      className={cn(
        "composer-mode-transition flex flex-col",
        // min-w-0: in document mode the composer sits in a row beside the transcript panel and
        // must be able to shrink, or the panel gets pushed off the module's edge.
        docMode ? "min-h-0 min-w-0 flex-1" : "shrink-0 px-6 pt-2 pb-6"
      )}
    >
      <ComposerCard
        docMode={docMode}
        maxHeight={applied}
        controls={controls}
        attachments={
          appshots.length > 0 && (
            <div
              data-appshot-attachments
              className="flex gap-2 overflow-x-auto px-4 pb-2"
            >
              {appshots.map((appshot) => (
                <div
                  key={appshot.id}
                  className="group rounded-control bg-fill-quiet shadow-surface relative flex w-64 shrink-0 items-center gap-2 p-1.5"
                >
                  <img
                    src={appshot.preview_data_url}
                    alt=""
                    className="rounded-control aspect-5/3 w-20 shrink-0 object-cover"
                  />
                  <div className="min-w-0 flex-1 pr-5">
                    <p className="text-metadata truncate font-medium">
                      {appshot.window_title}
                    </p>
                    {appshot.kind === "attachment" ? (
                      <p className="text-callout text-muted-foreground">
                        {t("composer.imageDimensions", {
                          width: appshot.width,
                          height: appshot.height,
                        })}
                      </p>
                    ) : (
                      <>
                        <p className="text-callout text-muted-foreground truncate">
                          {appshot.app_name}
                        </p>
                        <p className="text-metadata text-muted-foreground">
                          {t("composer.appshotText", {
                            count: appshot.text_length,
                          })}
                        </p>
                      </>
                    )}
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="text-muted-foreground absolute top-1 right-1 size-6 opacity-70 hover:opacity-100"
                    aria-label={
                      appshot.kind === "attachment"
                        ? t("composer.removeImage", {
                            title: appshot.window_title,
                          })
                        : t("composer.removeAppshot", {
                            title: appshot.window_title,
                          })
                    }
                    onClick={() => onRemoveAppshot?.(appshot.id)}
                  >
                    <X className="size-3.5" />
                  </Button>
                </div>
              ))}
            </div>
          )
        }
        overlay={
          showBriefOffer &&
          config.activeScene && (
            <div className="pointer-events-none absolute inset-x-0 top-8 z-20 px-6">
              <div className="raised-material canvas-ui-module shadow-raised pointer-events-auto mx-auto flex w-max max-w-full items-center gap-2 px-3 py-2">
                <ListChecks className="text-muted-foreground size-3.5 shrink-0" />
                <span className="text-body text-foreground min-w-0 truncate">
                  {t("brief.offer", { scene: config.activeScene.title })}
                </span>
                <Button size="sm" className="shrink-0" onClick={insertBrief}>
                  {t("brief.insert")}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  className="shrink-0"
                  onClick={dismissBrief}
                >
                  {t("brief.dismiss")}
                </Button>
              </div>
            </div>
          )
        }
        footer={
          !docMode &&
          checkout && <CheckoutBar config={config} checkout={checkout} />
        }
      >
        {children}
      </ComposerCard>
    </section>
  );
}

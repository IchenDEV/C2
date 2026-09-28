import { memo, useEffect, useMemo, useState } from "react";

import { StatusBadge } from "@/components/business/status-badge";
import { ActivityOrb } from "@/components/ui/activity-orb";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Bot,
  BookOpen,
  Brain,
  BrainCircuit,
  Check,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  CircleAlert,
  CircleCheck,
  Clock3,
  Copy,
  ExternalLink,
  GitFork,
  MoreHorizontal,
  Search,
  Terminal,
  Wrench,
} from "@/components/ui/icons";
import { Spinner } from "@/components/ui/spinner";
import { TooltipButton } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { useToast } from "@/ui/toast";

import { canvasGetSnapshot, getPromptImage, openExternal } from "../bridge";
import type { CanvasSnapshot } from "../bridge";
import { useLanguage, useT } from "../i18n";
import {
  agentActivityState,
  deriveAgentRoster,
  isAgentActivityTool,
} from "./agentActivity";
import type { AgentActivity, AgentActivityState } from "./agentActivity";
import { ArtifactPreview } from "./ArtifactPreview";
import { MarkdownContent } from "./MarkdownContent";
import type { BuiltinLinkActions } from "./MarkdownContent";
import {
  canvasExportDataUrl,
  collapsedPrompt,
  isLongPrompt,
  parseCanvasHistoryPrompt,
} from "./promptPreview";
import type { CanvasHistoryMarker } from "./promptPreview";
import { isRunning } from "./turns";
import type { PromptImage, ToolEntry, Turn } from "./turns";

const EMPTY_PROMPT_IMAGES: PromptImage[] = [];
const SEARCH_TOOL_PATTERN = /\b(?:search|searched|find|found|grep|rg)\b/iu;
const READ_TOOL_PATTERN = /\b(?:read|reading|open|view|inspect)\b/iu;
const COMMAND_TOOL_PATTERN =
  /\b(?:command|exec|execute|run|shell|terminal|test)\b/iu;

function duration(t: Turn): string | null {
  if (t.endedAt == null) return null;
  const s = Math.max(0, Math.round((t.endedAt - t.startedAt) / 1000));
  return s >= 60 ? `${Math.floor(s / 60)}m ${s % 60}s` : `${s}s`;
}

type CopyTarget = "prompt" | "response";

function TurnActionButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <TooltipButton
      label={label}
      variant="ghost"
      size="icon-xs"
      className="text-muted-foreground"
      onClick={onClick}
    >
      {children}
    </TooltipButton>
  );
}

function toolStatusDot(status: string): string {
  if (status === "completed") return "bg-success";
  if (status === "failed") return "bg-destructive";
  return "bg-warning";
}

function toolIcon(tool: ToolEntry) {
  const signal = `${tool.kind ?? ""} ${tool.title}`;
  if (SEARCH_TOOL_PATTERN.test(signal)) return Search;
  if (READ_TOOL_PATTERN.test(signal)) return BookOpen;
  if (COMMAND_TOOL_PATTERN.test(signal)) return Terminal;
  return Wrench;
}

function toolHasOutput(tool: ToolEntry): boolean {
  return (tool.outputs?.length ?? 0) > 0;
}

function promptTextWithoutImageMarkers(
  prompt: string,
  images: readonly PromptImage[]
): string {
  let visible = prompt;
  const markers = new Set<string>();
  for (const image of images) {
    markers.add(`[attachment:${image.id}]`);
    if (image.name != null && image.name !== "")
      markers.add(`[image:${image.name}]`);
  }
  for (const marker of markers) visible = visible.split(marker).join("");
  return visible.replaceAll(/\n(?:[ \t]*\n){2,}/gu, "\n\n").trim();
}

function PromptImageThumbnail({ image }: { image: PromptImage }) {
  const [loaded, setLoaded] = useState<Awaited<
    ReturnType<typeof getPromptImage>
  > | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    if (image.previewDataUrl != null && image.previewDataUrl !== "") return;
    let alive = true;
    void getPromptImage(image.id)
      .then((capture) => {
        if (alive) setLoaded(capture);
      })
      .catch(() => {
        if (alive) setFailed(true);
      });
    return () => {
      alive = false;
    };
  }, [image.id, image.previewDataUrl]);

  const src = image.previewDataUrl ?? loaded?.preview_data_url;
  const name = loaded?.window_title ?? image.name ?? "Attached image";
  const width = image.width ?? loaded?.width;
  const height = image.height ?? loaded?.height;

  return (
    <figure
      data-prompt-image={image.id}
      className="rounded-module bg-background/25 ring-foreground/10 flex min-h-24 max-w-80 min-w-0 items-center justify-center overflow-hidden ring-[0.5px]"
    >
      {src != null && src !== "" && !failed ? (
        <img
          src={src}
          alt={name}
          width={width ?? undefined}
          height={height ?? undefined}
          loading="lazy"
          className="block max-h-80 w-full object-contain"
          onError={() => setFailed(true)}
        />
      ) : failed ? (
        <div
          role="img"
          aria-label={`${name} unavailable`}
          className="text-callout text-muted-foreground flex min-w-0 items-center gap-2 px-3 py-8"
        >
          <CircleAlert className="size-4 shrink-0" aria-hidden />
          <span className="truncate">{name}</span>
        </div>
      ) : (
        <div
          role="status"
          aria-label={`Loading ${name}`}
          className="text-muted-foreground px-3 py-8"
        >
          <Spinner />
        </div>
      )}
    </figure>
  );
}

export function safeResourceLink(
  uri: string
): { uri: string; host: string } | null {
  try {
    const parsed = new URL(uri);
    if (
      !["http:", "https:"].includes(parsed.protocol) ||
      parsed.username.length > 0 ||
      parsed.password.length > 0
    ) {
      return null;
    }
    return { uri: parsed.toString(), host: parsed.host };
  } catch {
    return null;
  }
}

function ToolCallBlock({
  tool,
  compact = false,
}: {
  tool: ToolEntry;
  compact?: boolean;
}) {
  const textOutputs = (tool.outputs ?? []).flatMap((output) =>
    output.type === "text" ? [output.text] : []
  );
  const images = (tool.outputs ?? []).flatMap((output) =>
    output.type === "image" ? [output.artifact] : []
  );
  const resourceLinks = (tool.outputs ?? []).flatMap((output) => {
    if (output.type !== "resource_link") return [];
    const safe = safeResourceLink(output.uri);
    return safe ? [{ ...output, ...safe }] : [];
  });
  const hasOutput =
    textOutputs.length + images.length + resourceLinks.length > 0;
  const [open, setOpen] = useState(false);
  const ToolIcon = toolIcon(tool);
  const header = (
    <>
      <ToolIcon className="size-3.5 shrink-0" aria-hidden />
      <span
        className={cn(
          "min-w-0 flex-1 truncate",
          compact ? "text-muted-foreground" : "text-foreground font-medium"
        )}
        title={tool.title}
      >
        {tool.title}
      </span>
      {!compact && tool.kind != null && tool.kind !== "" ? (
        <span className="text-metadata text-muted-foreground shrink-0 font-mono">
          {tool.kind}
        </span>
      ) : null}
      {!compact || tool.status !== "completed" ? (
        <span className="text-callout flex shrink-0 items-center gap-1.5">
          <span
            className={cn("size-1.5 rounded-full", toolStatusDot(tool.status))}
            aria-hidden
          />
          {tool.status}
        </span>
      ) : null}
    </>
  );

  if (!hasOutput) {
    return (
      <div
        className={cn(
          "text-body text-muted-foreground flex min-w-0 items-center gap-2 pe-1",
          compact ? "py-1" : "my-3 py-1.5"
        )}
        data-tool-call={tool.id}
      >
        {header}
      </div>
    );
  }

  return (
    <Collapsible
      open={open}
      onOpenChange={setOpen}
      className={cn("min-w-0", compact ? "py-0.5" : "my-3")}
      data-tool-call={tool.id}
    >
      <CollapsibleTrigger
        render={
          <Button
            type="button"
            variant="ghost"
            size="compact"
            focusStyle="inset"
          />
        }
        className={cn(
          "group text-muted-foreground hover:text-foreground h-auto w-full min-w-0 justify-start gap-2 text-start has-[>svg]:ps-0",
          compact ? "py-1" : "py-1.5"
        )}
      >
        {header}
        <ChevronRight
          className="size-3.5 shrink-0 transition-transform group-data-[state=open]:rotate-90"
          aria-hidden
        />
      </CollapsibleTrigger>
      <CollapsibleContent
        className={cn(
          "divide-border rounded-module bg-fill-quiet min-w-0 divide-y overflow-hidden border",
          compact ? "ms-5 mt-0.5 mb-1" : "mt-1.5"
        )}
      >
        {textOutputs.map((output, index) => (
          <pre
            key={index}
            className="text-code max-h-80 overflow-auto p-3 font-mono break-words whitespace-pre-wrap"
          >
            <code>{output}</code>
          </pre>
        ))}
        {images.length > 0 ? (
          <div
            className="grid grid-cols-1 gap-2 p-2 sm:grid-cols-2"
            aria-label="Generated images"
          >
            {images.map((artifact) => (
              <ArtifactPreview key={artifact.id} artifact={artifact} />
            ))}
          </div>
        ) : null}
        {resourceLinks.length > 0 ? (
          <div className="flex flex-col gap-1.5 p-2" aria-label="Tool links">
            {resourceLinks.map((link) => (
              <Button
                key={link.uri}
                type="button"
                variant="ghost"
                size="row"
                focusStyle="inset"
                className="text-callout min-w-0 gap-2 px-2 py-1.5"
                title={link.uri}
                onClick={() => void openExternal(link.uri)}
              >
                <ExternalLink
                  className="text-muted-foreground size-3.5 shrink-0"
                  aria-hidden
                />
                <span className="text-foreground min-w-0 flex-1 truncate font-medium">
                  {link.name}
                </span>
                <span className="text-metadata text-muted-foreground max-w-52 truncate font-mono">
                  {link.host}
                </span>
              </Button>
            ))}
          </div>
        ) : null}
      </CollapsibleContent>
    </Collapsible>
  );
}

function ToolCallGroup({ tools }: { tools: ToolEntry[] }) {
  const t = useT();
  let status = "completed";
  for (const tool of tools) {
    if (tool.status === "failed") {
      status = tool.status;
      break;
    }
    if (status === "completed" && tool.status !== "completed")
      status = tool.status;
  }
  const active = status !== "completed" && status !== "failed";
  const latest = tools.at(-1)!;
  const LatestIcon = toolIcon(latest);
  const history = toolHasOutput(latest) ? tools : tools.slice(0, -1);
  const historyIsLong = history.length > 6;
  const [open, setOpen] = useState(active);

  useEffect(() => {
    if (active) setOpen(true);
  }, [active, tools.length]);

  return (
    <Collapsible
      open={open}
      onOpenChange={setOpen}
      className="my-3 min-w-0"
      data-tool-call-group={tools[0].id}
    >
      <CollapsibleTrigger
        render={
          <Button
            type="button"
            variant="ghost"
            size="compact"
            focusStyle="inset"
          />
        }
        className="group text-muted-foreground hover:text-foreground h-auto w-full min-w-0 justify-start gap-2 py-1.5 text-start has-[>svg]:ps-0"
      >
        <LatestIcon className="size-3.5 shrink-0" aria-hidden />
        <span className="min-w-0 flex-1 truncate" title={latest.title}>
          {latest.title}
        </span>
        <span className="sr-only">
          {t("turn.tools")} ({tools.length}), {status}
        </span>
        {status === "failed" ? (
          <span className="text-callout text-destructive flex shrink-0 items-center gap-1.5">
            <CircleAlert className="size-3.5" aria-hidden />
            {status}
          </span>
        ) : active ? (
          <span
            className={cn(
              "size-1.5 shrink-0 rounded-full",
              toolStatusDot(status)
            )}
            aria-hidden
          />
        ) : null}
        <ChevronRight
          className="size-3.5 shrink-0 transition-transform group-data-[state=open]:rotate-90"
          aria-hidden
        />
      </CollapsibleTrigger>
      <CollapsibleContent className="min-w-0">
        <div
          className={cn(
            "max-h-56 min-w-0 overflow-y-auto overscroll-contain py-1 ps-5 pe-2",
            historyIsLong && "tool-call-history--faded pb-8"
          )}
          data-tool-call-history
          data-faded={historyIsLong || undefined}
        >
          {history.map((tool) => (
            <ToolCallBlock key={tool.id} tool={tool} compact />
          ))}
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

type RenderBlock =
  | { kind: "text"; text: string }
  | { kind: "tools"; tools: ToolEntry[] };

/** Join adjacent streamed chunks and tool runs while retaining their exact event boundaries. */
function orderedBlocks(turn: Turn): RenderBlock[] {
  const tools = new Map(
    turn.tools
      .filter((tool) => !isAgentActivityTool(tool))
      .map((tool) => [tool.id, tool])
  );
  const seenTools = new Set<string>();
  const blocks: RenderBlock[] = [];
  const appendTool = (tool: ToolEntry) => {
    const tail = blocks.at(-1)!;
    if (tail?.kind === "tools") tail.tools.push(tool);
    else blocks.push({ kind: "tools", tools: [tool] });
  };
  for (const entry of turn.content ?? []) {
    if (entry.kind === "text") {
      const tail = blocks.at(-1)!;
      if (tail?.kind === "text") tail.text += entry.text;
      else blocks.push({ kind: "text", text: entry.text });
      continue;
    }
    const tool = tools.get(entry.toolId);
    if (!tool || seenTools.has(tool.id)) continue;
    seenTools.add(tool.id);
    appendTool(tool);
  }
  // Compatibility for turns produced by older renderers and manually constructed fixtures.
  if (blocks.length === 0 && turn.text)
    blocks.push({ kind: "text", text: turn.text });
  for (const tool of tools.values()) {
    if (!seenTools.has(tool.id)) appendTool(tool);
  }
  return blocks;
}

function canvasKey(canvas: CanvasHistoryMarker): string {
  return `${canvas.id}:${canvas.revision}`;
}

function downloadCanvasPng(
  canvas: CanvasHistoryMarker,
  snapshot: CanvasSnapshot | undefined
): void {
  const exportItem =
    snapshot?.exports.find((item) => item.kind === "overview") ??
    snapshot?.exports[0];
  if (!exportItem || typeof document === "undefined") return;
  const anchor = document.createElement("a");
  anchor.href = canvasExportDataUrl(exportItem);
  anchor.download = `${canvas.id}-${canvas.revision}.png`;
  anchor.click();
}

function requestCanvasDuplicate(canvas: CanvasHistoryMarker): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent("codetwo-canvas-duplicate", {
      detail: { id: canvas.id, revision: canvas.revision },
    })
  );
}

/** A collapsible group of secondary detail (agents / thinking / memory). */
function Detail({
  icon: Icon,
  label,
  count,
  children,
  wide = false,
  defaultOpen = false,
}: {
  icon: typeof Brain;
  label: string;
  count: number;
  children: React.ReactNode;
  wide?: boolean;
  defaultOpen?: boolean;
}) {
  if (count === 0) return null;
  return (
    <Collapsible
      defaultOpen={defaultOpen}
      className={cn("min-w-0", wide && "basis-full")}
    >
      <CollapsibleTrigger
        render={
          <Button type="button" variant="ghost" size="xs" focusStyle="inset" />
        }
        className="group text-muted-foreground hover:text-foreground -ms-1 h-auto gap-1.5 font-normal"
      >
        <ChevronRight className="size-3 transition-transform group-data-[state=open]:rotate-90" />
        <Icon className="size-3" />
        {label} ({count})
      </CollapsibleTrigger>
      <CollapsibleContent className="py-1 ps-4">{children}</CollapsibleContent>
    </Collapsible>
  );
}

function agentStatusLabel(
  state: AgentActivityState,
  t: ReturnType<typeof useT>
): string {
  return t(`turn.agentStatus.${state}`);
}

function agentElapsed(agent: AgentActivity, now: number): string | null {
  if (agent.startedAt === undefined) return null;
  const totalSeconds = Math.max(
    0,
    Math.floor(((agent.endedAt ?? now) - agent.startedAt) / 1000)
  );
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) return `${hours}h ${minutes}m ${seconds}s`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}

function AgentStateIcon({ state }: { state: AgentActivityState }) {
  if (state === "active") {
    return <ActivityOrb state="working" visualSize={20} aria-hidden="true" />;
  }
  if (state === "completed") {
    return <CircleCheck className="text-success size-4" aria-hidden />;
  }
  if (state === "failed") {
    return <CircleAlert className="text-destructive size-4" aria-hidden />;
  }
  return <Clock3 className="text-warning size-4" aria-hidden />;
}

function AgentRosterSection({
  agents,
  label,
  now,
}: {
  agents: readonly AgentActivity[];
  label: string;
  now: number;
}) {
  const t = useT();
  if (agents.length === 0) return null;
  return (
    <div role="group" aria-label={label}>
      <div className="text-metadata text-muted-foreground flex items-center gap-2 px-2 py-1 font-medium tracking-wide uppercase">
        <span>{label}</span>
        <span className="tabular-nums">{agents.length}</span>
      </div>
      <ul className="divide-border divide-y">
        {agents.map((agent) => {
          const state = agentActivityState(agent.status);
          const elapsed = agentElapsed(agent, now);
          return (
            <li
              key={agent.id}
              data-agent-row={agent.id}
              data-agent-state={state}
              className="flex min-w-0 items-start gap-2.5 px-2 py-2"
            >
              <span className="mt-0.5 flex size-5 shrink-0 items-center justify-center">
                <AgentStateIcon state={state} />
              </span>
              <div className="min-w-0 flex-1">
                <div className="flex min-w-0 items-baseline gap-2">
                  <span className="text-body text-foreground min-w-0 flex-1 truncate font-medium">
                    {agent.title}
                  </span>
                  <span className="text-metadata text-muted-foreground shrink-0">
                    {agent.role}
                  </span>
                </div>
                {agent.task != null && agent.task !== "" && (
                  <p className="text-callout text-muted-foreground mt-0.5 line-clamp-2">
                    {agent.task}
                  </p>
                )}
              </div>
              <div className="text-metadata text-muted-foreground shrink-0 text-right">
                <span className="block" aria-live="polite" aria-atomic="true">
                  {agentStatusLabel(state, t)}
                </span>
                {elapsed != null && elapsed !== "" && (
                  <time className="block tabular-nums">{elapsed}</time>
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function AgentRoster({ agents }: { agents: readonly AgentActivity[] }) {
  const t = useT();
  const active = agents.filter((agent) => {
    const state = agentActivityState(agent.status);
    return state === "active" || state === "pending";
  });
  const finished = agents.filter((agent) => {
    const state = agentActivityState(agent.status);
    return state === "completed" || state === "failed";
  });
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (active.length === 0) return;
    const interval = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(interval);
  }, [active.length]);

  return (
    <div data-agent-roster className="flex flex-col gap-1.5">
      <AgentRosterSection
        agents={active}
        label={t("turn.agentGroup.active")}
        now={now}
      />
      <AgentRosterSection
        agents={finished}
        label={t("turn.agentGroup.finished")}
        now={now}
      />
    </div>
  );
}

/**
 * One prompt → response cycle.
 *
 * The prompt sits in a bubble on the right and the answer runs full width beneath it, so a long
 * transcript reads as a conversation instead of a stack of equally-weighted cards. Tool calls keep
 * their streamed position, with adjacent calls sharing one disclosure; thinking and memory
 * metadata stay collapsed underneath.
 */
export const TurnCard = memo(
  ({
    turn,
    canvasSnapshotLoader = canvasGetSnapshot,
    onSaveTemplate,
    linkActions,
    onFork,
  }: {
    turn: Turn;
    canvasSnapshotLoader?: typeof canvasGetSnapshot;
    /** Opens the R2 template dialog over this turn's prompt. Absent → the turn menu is hidden. */
    onSaveTemplate?: (promptText: string) => void;
    /** Native context-menu actions for links rendered inside the assistant response. */
    linkActions?: BuiltinLinkActions;
    /** Starts a new task whose referenced context ends at this completed turn. */
    onFork?: (turn: Turn) => void;
  }) => {
    const t = useT();
    const toast = useToast();
    const { locale } = useLanguage();
    const [promptExpanded, setPromptExpanded] = useState(false);
    const [copied, setCopied] = useState<CopyTarget | null>(null);
    const running = isRunning(turn);
    const queued = turn.delivery === "queued";
    const dur = duration(turn);
    const agents = deriveAgentRoster(turn.tools);
    const activeAgentCount = agents.filter((agent) => {
      const state = agentActivityState(agent.status);
      return state === "active" || state === "pending";
    }).length;
    const blocks = orderedBlocks(turn);
    const history = useMemo(
      () => parseCanvasHistoryPrompt(turn.prompt),
      [turn.prompt]
    );
    const promptImages = turn.promptImages ?? EMPTY_PROMPT_IMAGES;
    const promptText = promptTextWithoutImageMarkers(
      history.visiblePrompt,
      promptImages
    );
    const clock = new Intl.DateTimeFormat(locale, {
      hour: "2-digit",
      minute: "2-digit",
    });
    useEffect(() => {
      if (!copied) return;
      const timeout = window.setTimeout(() => setCopied(null), 1500);
      return () => window.clearTimeout(timeout);
    }, [copied]);
    const copyText = (target: CopyTarget, text: string) => {
      const write = navigator.clipboard?.writeText(text);
      if (write == null) {
        toast(t("turn.copyFailed"), "error");
        return;
      }
      void write
        .then(() => setCopied(target))
        .catch(() => toast(t("turn.copyFailed"), "error"));
    };
    const historySnapshots = useMemo(
      () => new Map<string, CanvasSnapshot>(),
      []
    );
    const [snapshots, setSnapshots] = useState<Record<string, CanvasSnapshot>>(
      {}
    );
    useEffect(() => {
      let cancelled = false;
      for (const canvas of history.canvases) {
        void canvasSnapshotLoader(canvas.id, canvas.revision)
          .then((snapshot) => {
            if (cancelled || !snapshot) return;
            historySnapshots.set(canvasKey(canvas), snapshot);
            setSnapshots((current) => ({
              ...current,
              [canvasKey(canvas)]: snapshot,
            }));
          })
          .catch(() => {
            // Browser/dev fallback has no native bridge; marker metadata remains read-only.
          });
      }
      return () => {
        cancelled = true;
        historySnapshots.clear();
      };
    }, [canvasSnapshotLoader, history.canvases, historySnapshots]);
    const hasDetail =
      agents.length + turn.thoughts.length + (turn.memory?.items.length ?? 0) >
      0;
    const promptIsLong = isLongPrompt(promptText);
    const visiblePrompt =
      promptIsLong && !promptExpanded
        ? collapsedPrompt(promptText)
        : promptText;

    return (
      // Turns arrive one at a time, so each one entering under its own animation reads as the
      // conversation advancing rather than the list redrawing.
      <article aria-busy={running && !queued} className="animate-rise-in py-7">
        {/* prompt */}
        <div className="group/prompt flex flex-col items-end">
          <div className="flex items-start justify-end gap-1">
            {/* Hover-visible turn menu (SessionRail hover-actions idiom). A menu rather than a bare
              button so future turn actions slot in beside "Save as template…". */}
            {onSaveTemplate && (
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon-xs"
                      aria-label={t("templateFrom.menu")}
                      className="text-muted-foreground mt-1 opacity-0 transition-opacity group-hover/prompt:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100"
                    >
                      <MoreHorizontal className="size-3.5" aria-hidden />
                    </Button>
                  }
                />
                <DropdownMenuContent align="end">
                  <DropdownMenuItem
                    onClick={() => onSaveTemplate(history.visiblePrompt)}
                  >
                    {t("templateFrom.saveAs")}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            )}
            <div className="rounded-module bg-secondary text-prose text-secondary-foreground max-w-[86%] px-3.5 py-2">
              {promptImages.length > 0 && (
                <div
                  data-prompt-images
                  className={cn(
                    "grid min-w-0 gap-1.5",
                    visiblePrompt && "mb-2",
                    promptImages.length > 1 && "grid-cols-2"
                  )}
                >
                  {promptImages.map((image, index) => (
                    <PromptImageThumbnail
                      key={`${image.id}-${index}`}
                      image={image}
                    />
                  ))}
                </div>
              )}
              {visiblePrompt && (
                <p className="break-words whitespace-pre-wrap">
                  {visiblePrompt}
                </p>
              )}
              {turn.delivery && (
                <p className="text-metadata text-muted-foreground mt-1.5 font-medium uppercase">
                  {turn.delivery === "queued"
                    ? t("turn.queued", { position: turn.queuePosition ?? 1 })
                    : t("turn.steered")}
                </p>
              )}
              {promptIsLong && (
                <Button
                  type="button"
                  variant="ghost"
                  size="compact"
                  focusStyle="inset"
                  aria-expanded={promptExpanded}
                  onClick={() => setPromptExpanded((value) => !value)}
                  className="text-callout text-muted-foreground mt-1.5 h-auto gap-1 px-0 py-0 font-medium"
                >
                  {promptExpanded ? (
                    <ChevronUp className="size-3" aria-hidden />
                  ) : (
                    <ChevronDown className="size-3" aria-hidden />
                  )}
                  {t(promptExpanded ? "turn.showLess" : "turn.showMore")}
                </Button>
              )}
            </div>
          </div>
          <div
            data-turn-actions="prompt"
            className="text-callout text-muted-foreground mt-1 flex min-h-(--ds-control-mini) items-center gap-1"
          >
            <time dateTime={new Date(turn.startedAt).toISOString()}>
              {clock.format(turn.startedAt)}
            </time>
            {promptText && (
              <TurnActionButton
                label={t(
                  copied === "prompt" ? "turn.copiedPrompt" : "turn.copyPrompt"
                )}
                onClick={() => copyText("prompt", promptText)}
              >
                {copied === "prompt" ? (
                  <Check aria-hidden />
                ) : (
                  <Copy aria-hidden />
                )}
              </TurnActionButton>
            )}
          </div>
        </div>

        {history.canvases.length > 0 && (
          <div className="mt-3 flex flex-col gap-2" aria-label="Canvas history">
            {history.canvases.map((canvas) => {
              const snapshot = snapshots[canvasKey(canvas)];
              const thumbnail =
                snapshot?.exports.find((item) => item.kind === "overview") ??
                snapshot?.exports[0];
              return (
                <section
                  key={canvasKey(canvas)}
                  className="canvas-ui-module bg-fill-quiet text-callout border p-2.5"
                >
                  <div className="flex items-start gap-2">
                    {thumbnail != null && (
                      <img
                        src={canvasExportDataUrl(thumbnail)}
                        alt={`${canvas.title} thumbnail`}
                        className="rounded-control size-14 shrink-0 border object-cover"
                      />
                    )}
                    <div className="min-w-0 flex-1">
                      <h3 className="text-foreground truncate font-medium">
                        {canvas.title}
                      </h3>
                      <p className="text-muted-foreground">
                        rev {canvas.revision}
                        {snapshot == null
                          ? ""
                          : ` · ${snapshot.objectCount} objects`}
                        {snapshot?.frozenAt
                          ? ` · ${new Intl.DateTimeFormat(locale, { dateStyle: "medium" }).format(snapshot.frozenAt)}`
                          : ""}
                      </p>
                      {canvas.textOriginals.length > 0 && (
                        <p className="text-muted-foreground mt-1 break-words whitespace-pre-wrap">
                          {canvas.textOriginals.join("\n")}
                        </p>
                      )}
                    </div>
                  </div>
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    <Button
                      type="button"
                      variant="secondary"
                      size="compact"
                      className="text-metadata text-muted-foreground"
                      disabled={!snapshot?.exports.length}
                      onClick={() => downloadCanvasPng(canvas, snapshot)}
                    >
                      Export PNG
                    </Button>
                    <Button
                      type="button"
                      variant="secondary"
                      size="compact"
                      className="text-metadata text-muted-foreground"
                      onClick={() => requestCanvasDuplicate(canvas)}
                    >
                      Duplicate into Composer
                    </Button>
                  </div>
                </section>
              );
            })}
          </div>
        )}

        {/* Streamed answer blocks keep provider order: text → tool → text → visual/chart. */}
        {blocks.length > 0 ? (
          <div className="mt-3.5 min-w-0">
            {blocks.map((block, index) =>
              block.kind === "text" ? (
                <MarkdownContent
                  key={`text-${index}`}
                  text={block.text}
                  streaming={running && index === blocks.length - 1}
                  linkActions={linkActions}
                />
              ) : block.tools.length === 1 ? (
                <ToolCallBlock key={block.tools[0].id} tool={block.tools[0]} />
              ) : (
                <ToolCallGroup key={block.tools[0].id} tools={block.tools} />
              )
            )}
          </div>
        ) : null}

        {running && !queued && !turn.text && (
          <p
            role="status"
            aria-live="polite"
            className="text-body text-muted-foreground mt-3.5 flex items-center gap-2"
          >
            <ActivityOrb
              state={turn.thoughts.length > 0 ? "solving" : "working"}
              aria-hidden="true"
            />
            {t("turn.working")}
          </p>
        )}

        {turn.error != null && turn.error !== "" && (
          <p className="rounded-control bg-destructive/10 text-body text-destructive mt-3.5 flex items-start gap-1.5 px-3 py-2">
            <CircleAlert className="mt-0.5 size-3.5 shrink-0" />
            {turn.error}
          </p>
        )}

        {/* secondary detail + outcome, on one quiet line */}
        {(hasDetail ||
          (dur != null && dur !== "") ||
          (turn.stopReason != null && turn.stopReason !== "") ||
          queued ||
          (running && turn.text != null && turn.text !== "")) && (
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-0.5">
            <Detail
              icon={Bot}
              label={t("turn.agents")}
              count={agents.length}
              wide
              defaultOpen={activeAgentCount > 0}
            >
              <AgentRoster agents={agents} />
            </Detail>

            <Detail
              icon={Brain}
              label={t("turn.thinking")}
              count={turn.thoughts.length}
            >
              <div className="text-callout text-muted-foreground flex flex-col gap-1 italic">
                {turn.thoughts.map((thought, i) => (
                  <p key={i} className="whitespace-pre-wrap">
                    {thought}
                  </p>
                ))}
              </div>
            </Detail>

            <Detail
              icon={BrainCircuit}
              label={t("turn.memory")}
              count={turn.memory?.items.length ?? 0}
            >
              {turn.memory && (
                <div className="text-callout flex flex-col gap-2">
                  <p className="text-muted-foreground">
                    {t("turn.memoryTokens", {
                      count: new Intl.NumberFormat(locale).format(
                        turn.memory.estimated_tokens
                      ),
                    })}
                  </p>
                  <ul className="flex flex-col gap-1.5">
                    {turn.memory.items.map((item) => {
                      const source = item.source
                        ? `${item.source.session_id.slice(0, 8)}:${item.source.part_seq}`
                        : t("memory.manual");
                      return (
                        <li
                          key={item.id}
                          className="rounded-control bg-fill-quiet px-2 py-1.5"
                        >
                          <div className="text-metadata text-muted-foreground flex items-center gap-1.5">
                            <span className="font-mono">{item.layer}</span>
                            <span aria-hidden="true">·</span>
                            <span>{item.category}</span>
                            <span className="ms-auto font-mono">{source}</span>
                          </div>
                          <p
                            dir="auto"
                            className="text-foreground/80 mt-1 break-words whitespace-pre-wrap"
                          >
                            {item.content}
                          </p>
                        </li>
                      );
                    })}
                  </ul>
                </div>
              )}
            </Detail>

            <span className="ms-auto flex shrink-0 items-center gap-1.5">
              {queued ? (
                <StatusBadge tone="neutral">
                  {t("turn.queued", { position: turn.queuePosition ?? 1 })}
                </StatusBadge>
              ) : running ? (
                <StatusBadge tone="neutral">
                  <Spinner className="size-2.5" /> {t("turn.running")}
                </StatusBadge>
              ) : turn.error != null && turn.error !== "" ? (
                <StatusBadge tone="destructive">{t("turn.failed")}</StatusBadge>
              ) : (
                turn.stopReason != null &&
                turn.stopReason !== "" && (
                  <StatusBadge tone="neutral">{turn.stopReason}</StatusBadge>
                )
              )}
              {dur != null && dur !== "" && (
                <span className="text-metadata text-muted-foreground font-mono">
                  {dur}
                </span>
              )}
            </span>
          </div>
        )}

        {!running && turn.text && (
          <div
            data-turn-actions="response"
            className="text-callout text-muted-foreground mt-2 flex min-h-(--ds-control-mini) items-center gap-1"
          >
            <TurnActionButton
              label={t(
                copied === "response"
                  ? "turn.copiedResponse"
                  : "turn.copyResponse"
              )}
              onClick={() => copyText("response", turn.text)}
            >
              {copied === "response" ? (
                <Check aria-hidden />
              ) : (
                <Copy aria-hidden />
              )}
            </TurnActionButton>
            {onFork &&
              turn.accepted &&
              turn.transcriptStartSeq !== undefined && (
                <TurnActionButton
                  label={t("turn.fork")}
                  onClick={() => onFork(turn)}
                >
                  <GitFork aria-hidden />
                </TurnActionButton>
              )}
            <time
              dateTime={new Date(turn.endedAt ?? turn.startedAt).toISOString()}
            >
              {clock.format(turn.endedAt ?? turn.startedAt)}
            </time>
          </div>
        )}
      </article>
    );
  }
);

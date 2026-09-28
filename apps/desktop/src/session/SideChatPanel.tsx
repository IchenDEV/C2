import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useCallback,
} from "react";
import type { PointerEvent as ReactPointerEvent } from "react";

import { ArrowUp, MessageSquare, Plus, Square, X } from "@/components/ui/icons";
import { cssVars } from "@/lib/cssVars";

import {
  cancelTurn,
  closeTransientSession,
  importPromptImage,
  newSession,
  onEngineEvent,
  setConfigOption,
  setExecutionPolicy,
  setModel,
  setSessionMemoryPolicy,
  submitPrompt,
} from "../bridge";
import type {
  AppshotCapture,
  ConfigOptionInfo,
  CoreEvent,
  DocBlock,
  ModelChoice,
  MemoryAccess,
  PermissionMode,
  ProviderInfo,
  Sandbox,
} from "../bridge";
import { ActivityOrb } from "../components/ui/activity-orb";
import { Button } from "../components/ui/button";
import { Textarea } from "../components/ui/textarea";
import { useT } from "../i18n";
import { cn } from "../lib/utils";
import { VoiceButton } from "../voice/VoiceButton";
import { ModelPicker, SessionModePicker } from "./Composer";
import type { BuiltinLinkActions } from "./MarkdownContent";
import { SESSION_MODES } from "./mode";
import type { SessionMode } from "./mode";
import { TurnCard } from "./TurnCard";
import { applyEvent, newTurn } from "./turns";
import type { Turn } from "./turns";

export interface TransientChatSeed {
  id: string;
  text: string;
}

export type SideChatSeed = TransientChatSeed;

export function transientMemoryPolicy(
  provider: string
): [MemoryAccess, MemoryAccess] {
  return provider === "codex" ? ["allow", "deny"] : ["inherit", "deny"];
}

interface TransientChatTab {
  localId: string;
  title: string | null;
  draft: string;
  sessionId: string | null;
  creationRequestId: string | null;
  turns: Turn[];
  running: boolean;
  models: ModelChoice[];
  currentModel: string | null;
  defaultModel: string | null;
  configOptions: ConfigOptionInfo[];
  provider: string;
  cwd: string;
  mode: PermissionMode;
  sandbox: Sandbox;
  controlError: string | null;
  attachments: AppshotCapture[];
  attaching: boolean;
}

interface PendingCreation {
  tabId: string;
  doc: DocBlock[];
  attachments: AppshotCapture[];
  promptRequestId: string;
}

interface PanelOffset {
  x: number;
  y: number;
}

interface ActivePanelMove {
  pointerId: number;
  startX: number;
  startY: number;
  startOffset: PanelOffset;
  width: number;
  height: number;
}

const QUICK_CHAT_VIEWPORT_INSET = 8;

let localTabSequence = 0;

function localId(prefix: string): string {
  localTabSequence += 1;
  return `${prefix}-${Date.now()}-${localTabSequence}`;
}

function makeTab({
  provider,
  cwd,
  model,
  mode,
  sandbox,
  models,
  draft = "",
  idPrefix,
}: {
  provider: string;
  cwd: string;
  model: string | null;
  mode: PermissionMode;
  sandbox: Sandbox;
  models: ModelChoice[];
  draft?: string;
  idPrefix: "quick-chat" | "side-chat";
}): TransientChatTab {
  return {
    localId: localId(idPrefix),
    title: null,
    draft,
    sessionId: null,
    creationRequestId: null,
    turns: [],
    running: false,
    models,
    currentModel: model,
    defaultModel: model,
    configOptions: [],
    provider,
    cwd,
    mode,
    sandbox,
    controlError: null,
    attachments: [],
    attaching: false,
  };
}

function privateImageBlock(capture: AppshotCapture): DocBlock {
  return capture.kind === "attachment"
    ? { type: "attachment", id: capture.id, name: capture.window_title }
    : { type: "appshot", id: capture.id, title: capture.window_title };
}

const TURN_EVENTS = new Set<CoreEvent["event"]>([
  "memory_context",
  "turn_started",
  "agent_text",
  "agent_thought",
  "tool_call",
  "turn_ended",
  "error",
]);

interface TransientChatPanelProps {
  open: boolean;
  onClose: () => void;
  provider: string;
  providers: ProviderInfo[];
  cwd: string;
  model: string | null;
  mode: PermissionMode;
  sandbox: Sandbox;
  seed: TransientChatSeed | null;
  onSeedHandled: (id: string) => void;
  linkActions?: BuiltinLinkActions;
  voiceEnabled?: boolean;
}

export function QuickChatPanel(props: TransientChatPanelProps) {
  return <TransientChatPanel {...props} surface="quick" />;
}

export function SideChatPanel(props: TransientChatPanelProps) {
  return <TransientChatPanel {...props} surface="side" />;
}

function TransientChatPanel({
  open,
  onClose,
  provider,
  providers,
  cwd,
  model,
  mode,
  sandbox,
  seed,
  onSeedHandled,
  linkActions,
  voiceEnabled = true,
  surface,
}: TransientChatPanelProps & { surface: "quick" | "side" }) {
  const t = useT();
  const floating = surface === "quick";
  const labels =
    surface === "quick"
      ? {
          title: "quickChat.title" as const,
          new: "quickChat.new" as const,
          closeTab: "quickChat.closeTab" as const,
          hide: "quickChat.hide" as const,
          temporary: "quickChat.temporary" as const,
          placeholder: "quickChat.placeholder" as const,
          send: "quickChat.send" as const,
        }
      : {
          title: "sideChat.title" as const,
          new: "sideChat.new" as const,
          closeTab: "sideChat.closeTab" as const,
          hide: "sideChat.hide" as const,
          temporary: "sideChat.temporary" as const,
          placeholder: "sideChat.placeholder" as const,
          send: "sideChat.send" as const,
        };
  const [tabs, setTabs] = useState<TransientChatTab[]>([]);
  const tabsRef = useRef(tabs);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const pendingCreationsRef = useRef(new Map<string, PendingCreation>());
  const handledSeedsRef = useRef(new Set<string>());
  const initialTabPendingRef = useRef(false);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const imageInputRef = useRef<HTMLInputElement | null>(null);
  const panelRef = useRef<HTMLElement | null>(null);
  const panelOffsetRef = useRef<PanelOffset>({ x: 0, y: 0 });
  const activePanelMoveRef = useRef<ActivePanelMove | null>(null);

  const clampPanelOffset = useCallback(
    (offset: PanelOffset, width: number, height: number): PanelOffset => {
      const centeredLeft = (window.innerWidth - width) / 2;
      const centeredTop = (window.innerHeight - height) / 2;
      const minX = QUICK_CHAT_VIEWPORT_INSET - centeredLeft;
      const maxX =
        window.innerWidth - width - QUICK_CHAT_VIEWPORT_INSET - centeredLeft;
      const minY = QUICK_CHAT_VIEWPORT_INSET - centeredTop;
      const maxY =
        window.innerHeight - height - QUICK_CHAT_VIEWPORT_INSET - centeredTop;
      return {
        x: Math.round(
          Math.min(
            Math.max(minX, maxX),
            Math.max(Math.min(minX, maxX), offset.x)
          )
        ),
        y: Math.round(
          Math.min(
            Math.max(minY, maxY),
            Math.max(Math.min(minY, maxY), offset.y)
          )
        ),
      };
    },
    []
  );

  const applyPanelOffset = (offset: PanelOffset) => {
    panelOffsetRef.current = offset;
    const panel = panelRef.current;
    if (!panel) return;
    panel.style.setProperty("--quick-chat-offset-x", `${offset.x}px`);
    panel.style.setProperty("--quick-chat-offset-y", `${offset.y}px`);
  };

  const finishPanelMove = (header: HTMLElement, pointerId: number) => {
    const active = activePanelMoveRef.current;
    if (!active || active.pointerId !== pointerId) return;
    activePanelMoveRef.current = null;
    document.body.classList.remove("moving-quick-chat");
    delete panelRef.current?.dataset.moving;
    if (header.hasPointerCapture(pointerId))
      header.releasePointerCapture(pointerId);
  };

  const onPanelPointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    if (!floating || event.button !== 0 || activePanelMoveRef.current) return;
    const target = event.target as Element;
    if (target.closest("button, a, input, textarea, select, [role='tab']"))
      return;
    const panel = panelRef.current;
    if (!panel) return;
    const rect = panel.getBoundingClientRect();
    event.preventDefault();
    activePanelMoveRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      startOffset: panelOffsetRef.current,
      width: rect.width,
      height: rect.height,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
    document.body.classList.add("moving-quick-chat");
    panel.dataset.moving = "";
  };

  const onPanelPointerMove = (event: ReactPointerEvent<HTMLElement>) => {
    const active = activePanelMoveRef.current;
    if (!active || active.pointerId !== event.pointerId) return;
    applyPanelOffset(
      clampPanelOffset(
        {
          x: active.startOffset.x + event.clientX - active.startX,
          y: active.startOffset.y + event.clientY - active.startY,
        },
        active.width,
        active.height
      )
    );
  };

  useEffect(() => {
    if (!floating) return;
    const keepPanelVisible = () => {
      const rect = panelRef.current?.getBoundingClientRect();
      if (!rect) return;
      applyPanelOffset(
        clampPanelOffset(panelOffsetRef.current, rect.width, rect.height)
      );
    };
    window.addEventListener("resize", keepPanelVisible);
    return () => window.removeEventListener("resize", keepPanelVisible);
  }, [applyPanelOffset, clampPanelOffset, floating]);

  useEffect(() => {
    if (!floating) return;
    return () => {
      activePanelMoveRef.current = null;
      document.body.classList.remove("moving-quick-chat");
    };
  }, [floating]);

  useEffect(() => {
    tabsRef.current = tabs;
  }, [tabs]);

  useEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;
    if (open) panel.removeAttribute("inert");
    else panel.setAttribute("inert", "");
  }, [open]);

  const providerModels = useCallback(
    (providerId: string) => {
      const advertised =
        providers.find((candidate) => candidate.id === providerId)?.models ??
        [];
      if (
        advertised.length > 0 ||
        providerId !== provider ||
        model == null ||
        model === ""
      )
        return advertised;
      return [{ id: model, name: model, description: null }];
    },
    [model, provider, providers]
  );

  const createLocalTab = useCallback(
    (draft = "", replaceExisting = false) => {
      const tab = makeTab({
        provider,
        cwd: cwd || ".",
        model,
        mode,
        sandbox,
        models: providerModels(provider),
        draft,
        idPrefix: surface === "quick" ? "quick-chat" : "side-chat",
      });
      const previous = tabsRef.current;
      const next = replaceExisting ? [tab] : [...previous, tab];
      tabsRef.current = next;
      setTabs(next);
      setActiveTabId(tab.localId);
      if (replaceExisting) {
        for (const existing of previous) {
          if (existing.sessionId != null && existing.sessionId !== "")
            void closeTransientSession(existing.sessionId);
        }
      }
      return tab.localId;
    },
    [cwd, mode, model, provider, providerModels, sandbox, surface]
  );

  useEffect(() => {
    if (!open) {
      initialTabPendingRef.current = false;
      return;
    }
    if (tabs.length > 0) {
      initialTabPendingRef.current = false;
      return;
    }
    if (seed || initialTabPendingRef.current) return;
    initialTabPendingRef.current = true;
    createLocalTab();
  }, [createLocalTab, open, seed, tabs.length]);

  useEffect(() => {
    setTabs((current) =>
      current.map((tab) =>
        tab.sessionId === null && tab.creationRequestId === null
          ? { ...tab, models: providerModels(tab.provider) }
          : tab
      )
    );
  }, [providerModels]);

  useEffect(() => {
    if (!seed || handledSeedsRef.current.has(seed.id)) return;
    handledSeedsRef.current.add(seed.id);
    const active = tabsRef.current.find((tab) => tab.localId === activeTabId);
    if (
      active &&
      active.sessionId === null &&
      active.creationRequestId === null &&
      active.turns.length === 0 &&
      active.draft.trim().length === 0
    ) {
      setTabs((current) =>
        current.map((tab) =>
          tab.localId === active.localId ? { ...tab, draft: seed.text } : tab
        )
      );
    } else {
      createLocalTab(seed.text, surface === "side");
    }
    onSeedHandled(seed.id);
  }, [activeTabId, createLocalTab, onSeedHandled, seed, surface]);

  const updateTab = useCallback(
    (tabId: string, update: (tab: TransientChatTab) => TransientChatTab) => {
      setTabs((current) =>
        current.map((tab) => (tab.localId === tabId ? update(tab) : tab))
      );
    },
    []
  );

  const failPrompt = useCallback(
    (
      tabId: string,
      requestId: string,
      message: string,
      attachments: AppshotCapture[] = []
    ) => {
      updateTab(tabId, (tab) => ({
        ...tab,
        running: false,
        creationRequestId: null,
        attachments: tab.attachments.length > 0 ? tab.attachments : attachments,
        turns: applyEvent(tab.turns, {
          event: "error",
          session: tab.sessionId,
          message,
          terminal: true,
          request_id: requestId,
        }),
      }));
    },
    [updateTab]
  );

  useEffect(() => {
    let disposed = false;
    const subscription = onEngineEvent((event) => {
      if (
        event.event === "session_created" &&
        event.request_id != null &&
        event.request_id !== ""
      ) {
        const pending = pendingCreationsRef.current.get(event.request_id);
        if (!pending) return;
        pendingCreationsRef.current.delete(event.request_id);
        const tab = tabsRef.current.find(
          (candidate) => candidate.localId === pending.tabId
        );
        if (!tab) {
          void closeTransientSession(event.session);
          return;
        }

        updateTab(pending.tabId, (current) => ({
          ...current,
          sessionId: event.session,
          creationRequestId: null,
        }));
        void (async () => {
          // Transient chat can use project memory for context, but app-lifetime conversations never
          // write durable memories of their own.
          const [memoryRead, memoryWrite] = transientMemoryPolicy(tab.provider);
          await setSessionMemoryPolicy(event.session, memoryRead, memoryWrite);
          if (
            !tabsRef.current.some(
              (candidate) => candidate.localId === pending.tabId
            )
          ) {
            await closeTransientSession(event.session);
            return;
          }
          await submitPrompt(
            event.session,
            pending.doc,
            pending.promptRequestId
          );
        })().catch((error: unknown) =>
          failPrompt(
            pending.tabId,
            pending.promptRequestId,
            String(error),
            pending.attachments
          )
        );
        return;
      }

      if (
        event.event === "error" &&
        event.session === null &&
        event.request_id != null &&
        event.request_id !== ""
      ) {
        const pending = pendingCreationsRef.current.get(event.request_id);
        if (!pending) return;
        pendingCreationsRef.current.delete(event.request_id);
        failPrompt(
          pending.tabId,
          pending.promptRequestId,
          event.message,
          pending.attachments
        );
        return;
      }

      if (event.session === null) return;
      const tab = tabsRef.current.find(
        (candidate) => candidate.sessionId === event.session
      );
      if (!tab) return;

      if (event.event === "session_title_changed") {
        updateTab(tab.localId, (current) => ({
          ...current,
          title: event.title,
        }));
        return;
      }
      if (event.event === "models") {
        updateTab(tab.localId, (current) => ({
          ...current,
          models: event.available.length > 0 ? event.available : current.models,
          currentModel: event.current || current.currentModel,
          defaultModel: current.defaultModel ?? event.current ?? null,
        }));
        return;
      }
      if (event.event === "config_options") {
        updateTab(tab.localId, (current) => ({
          ...current,
          configOptions: event.options,
        }));
        return;
      }
      if (event.event === "execution_policy_changed") {
        updateTab(tab.localId, (current) => ({ ...current, ...event.policy }));
        return;
      }
      if (!TURN_EVENTS.has(event.event)) return;

      updateTab(tab.localId, (current) => ({
        ...current,
        running:
          event.event === "turn_ended" ||
          (event.event === "error" && event.terminal)
            ? false
            : current.running,
        turns: applyEvent(current.turns, event),
      }));
    });
    return () => {
      disposed = true;
      void subscription.then((unlisten) => {
        if (disposed) unlisten();
      });
    };
  }, [failPrompt, updateTab]);

  const activeTab =
    tabs.find((tab) => tab.localId === activeTabId) ?? tabs[0] ?? null;

  useEffect(() => {
    if (!open || activeTab == null) return;
    const frame = window.requestAnimationFrame(() => {
      panelRef.current?.querySelector<HTMLTextAreaElement>("textarea")?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [activeTab?.localId, open]);

  useEffect(() => {
    if (!open || !floating) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [floating, onClose, open]);

  useLayoutEffect(() => {
    const viewport = viewportRef.current;
    if (viewport) viewport.scrollTop = viewport.scrollHeight;
  }, [activeTab?.turns]);

  const attachImages = async (files: readonly File[]) => {
    const tab = tabsRef.current.find(
      (candidate) => candidate.localId === activeTabId
    );
    const images = files.filter((file) => file.type.startsWith("image/"));
    if (!tab || tab.running || images.length === 0) return;
    updateTab(tab.localId, (current) => ({
      ...current,
      attaching: true,
      controlError: null,
    }));
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
    const attachments = results.flatMap((result) =>
      result.status === "fulfilled" ? [result.value] : []
    );
    const failure = results.find(
      (result): result is PromiseRejectedResult => result.status === "rejected"
    );
    updateTab(tab.localId, (current) => ({
      ...current,
      attaching: false,
      attachments: [...current.attachments, ...attachments],
      controlError: failure
        ? t("toast.imageAttachFailed", { error: String(failure.reason) })
        : null,
    }));
  };

  const changeMode = (nextMode: SessionMode) => {
    const tab = tabsRef.current.find(
      (candidate) => candidate.localId === activeTabId
    );
    const preset = SESSION_MODES.find((candidate) => candidate.id === nextMode);
    if (!tab || !preset || tab.running) return;
    const previous = { mode: tab.mode, sandbox: tab.sandbox };
    updateTab(tab.localId, (current) => ({
      ...current,
      mode: preset.mode,
      sandbox: preset.sandbox,
      controlError: null,
    }));
    if (tab.sessionId == null || tab.sessionId === "") return;
    const requestId = globalThis.crypto.randomUUID();
    void setExecutionPolicy(
      tab.sessionId,
      preset.mode,
      preset.sandbox,
      requestId
    ).catch((error: unknown) =>
      updateTab(tab.localId, (current) => ({
        ...current,
        ...previous,
        controlError: `Could not update execution policy: ${String(error)}`,
      }))
    );
  };

  const send = async () => {
    const tab = tabsRef.current.find(
      (candidate) => candidate.localId === activeTabId
    );
    if (!tab || tab.running || tab.attaching) return;
    const prompt = tab.draft.trim();
    if (!prompt && tab.attachments.length === 0) return;
    const doc: DocBlock[] = [
      ...(prompt ? [{ type: "text" as const, text: prompt }] : []),
      ...tab.attachments.map(privateImageBlock),
    ];
    const summary =
      prompt ||
      tab.attachments.map((attachment) => attachment.window_title).join(", ");
    const sentAttachments = tab.attachments;
    const promptRequestId = localId(
      surface === "quick" ? "quick-prompt" : "side-prompt"
    );
    updateTab(tab.localId, (current) => ({
      ...current,
      draft: "",
      attachments: [],
      running: true,
      turns: [...current.turns, newTurn(summary, promptRequestId)],
    }));

    if (tab.sessionId != null && tab.sessionId !== "") {
      try {
        await submitPrompt(tab.sessionId, doc, promptRequestId);
      } catch (error) {
        failPrompt(
          tab.localId,
          promptRequestId,
          String(error),
          sentAttachments
        );
      }
      return;
    }

    const creationRequestId = promptRequestId;
    pendingCreationsRef.current.set(creationRequestId, {
      tabId: tab.localId,
      doc,
      attachments: sentAttachments,
      promptRequestId,
    });
    updateTab(tab.localId, (current) => ({
      ...current,
      creationRequestId,
    }));
    try {
      await newSession(
        tab.provider,
        tab.cwd,
        null,
        creationRequestId,
        null,
        { mode: tab.mode, sandbox: tab.sandbox },
        tab.currentModel,
        true
      );
    } catch (error) {
      if (pendingCreationsRef.current.delete(creationRequestId)) {
        failPrompt(
          tab.localId,
          promptRequestId,
          String(error),
          sentAttachments
        );
      }
    }
  };

  const closeTab = (tabId: string) => {
    const { current } = tabsRef;
    const index = current.findIndex((tab) => tab.localId === tabId);
    if (index === -1) return;
    const closing = current[index];
    const remaining = current.filter((tab) => tab.localId !== tabId);
    tabsRef.current = remaining;
    setTabs(remaining);
    if (closing.sessionId != null && closing.sessionId !== "")
      void closeTransientSession(closing.sessionId);
    if (activeTabId === tabId) {
      setActiveTabId(
        remaining[Math.min(index, remaining.length - 1)]?.localId ?? null
      );
    }
    if (remaining.length === 0) onClose();
  };

  return (
    <section
      ref={panelRef}
      role={floating ? "dialog" : undefined}
      aria-modal={floating ? "false" : undefined}
      aria-label={t(labels.title)}
      aria-hidden={!open}
      data-open={open ? "" : undefined}
      data-chat-surface={surface}
      data-chat-count={tabs.length}
      className={cn(
        "bg-background flex min-h-0 flex-col overflow-hidden",
        floating
          ? "quick-chat-panel shadow-raised fixed z-50"
          : "side-chat-surface relative size-full",
        !open && "pointer-events-none"
      )}
      style={
        floating
          ? cssVars({
              "--quick-chat-offset-x": `${panelOffsetRef.current.x}px`,
              "--quick-chat-offset-y": `${panelOffsetRef.current.y}px`,
            })
          : undefined
      }
    >
      {floating ? (
        <header
          data-quick-chat-drag-handle=""
          data-transient-chat-tabs=""
          className={cn(
            "transient-chat-header flex shrink-0 items-center gap-1",
            floating && "cursor-move touch-none select-none"
          )}
          onPointerDown={onPanelPointerDown}
          onPointerMove={onPanelPointerMove}
          onPointerUp={(event) =>
            finishPanelMove(event.currentTarget, event.pointerId)
          }
          onPointerCancel={(event) =>
            finishPanelMove(event.currentTarget, event.pointerId)
          }
          onLostPointerCapture={(event) =>
            finishPanelMove(event.currentTarget, event.pointerId)
          }
        >
          <div
            className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"
            role="tablist"
          >
            {tabs.map((tab) => (
              <div
                key={tab.localId}
                data-selected={tab.localId === activeTab?.localId}
                className={cn(
                  "transient-chat-tab rounded-control flex max-w-48 shrink-0 items-center py-0.5 ps-1 transition-colors",
                  tab.localId === activeTab?.localId
                    ? "bg-secondary text-foreground"
                    : "text-muted-foreground hover:bg-accent/50 hover:text-foreground"
                )}
              >
                <Button
                  type="button"
                  variant="ghost"
                  size="compact"
                  role="tab"
                  aria-selected={tab.localId === activeTab?.localId}
                  className="text-body min-w-0 flex-1 truncate px-2"
                  onClick={() => setActiveTabId(tab.localId)}
                >
                  {tab.title ?? t(labels.title)}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="size-6 shrink-0"
                  aria-label={t(labels.closeTab)}
                  onClick={() => closeTab(tab.localId)}
                >
                  <X className="size-3.5" aria-hidden />
                </Button>
              </div>
            ))}
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="size-7 shrink-0"
              aria-label={t(labels.new)}
              onClick={() => createLocalTab()}
            >
              <Plus className="size-4" aria-hidden />
            </Button>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-7 shrink-0"
            title={t(labels.hide)}
            aria-label={t(labels.hide)}
            onClick={onClose}
          >
            <span aria-hidden>−</span>
          </Button>
        </header>
      ) : null}

      {activeTab != null && (
        <p
          className="text-metadata text-muted-foreground truncate px-3 py-1"
          title={activeTab.cwd}
        >
          {activeTab.provider} · {activeTab.cwd}
        </p>
      )}
      <div
        ref={viewportRef}
        className="min-h-0 flex-1 overflow-y-auto overscroll-contain"
      >
        {activeTab != null && activeTab.turns.length > 0 ? (
          <ol className="m-0 mx-auto w-full max-w-2xl list-none px-5 pt-5 pb-8">
            {activeTab.turns.map((turn) => (
              <li key={turn.transcriptStartSeq ?? turn.id}>
                <TurnCard
                  turn={turn}
                  linkActions={{ ...linkActions, workspaceRoot: activeTab.cwd }}
                />
              </li>
            ))}
          </ol>
        ) : (
          <div className="flex size-full flex-col items-center justify-center px-8 text-center">
            <span className="bg-fill-quiet text-muted-foreground mb-4 flex size-10 items-center justify-center rounded-full border">
              <MessageSquare className="size-5" aria-hidden />
            </span>
            <h2 className="text-ui font-medium">{t(labels.title)}</h2>
            <p className="text-ui text-muted-foreground mt-2 max-w-72 leading-relaxed">
              {t(labels.temporary)}
            </p>
          </div>
        )}
      </div>

      {activeTab != null && (
        <form
          className="shrink-0 p-4 pt-2"
          onSubmit={(event) => {
            event.preventDefault();
            void send();
          }}
        >
          {activeTab.controlError != null && activeTab.controlError !== "" ? (
            <p role="alert" className="text-fine text-destructive mb-2 px-1">
              {activeTab.controlError}
            </p>
          ) : null}
          <div
            data-transient-chat-composer=""
            className="rounded-module bg-card shadow-control duration-feedback ease-enter focus-within:bg-fill-hover transition-colors"
          >
            <input
              ref={imageInputRef}
              type="file"
              accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml"
              multiple
              hidden
              onChange={(event) => {
                const files = [...(event.currentTarget.files ?? [])];
                event.currentTarget.value = "";
                if (files.length > 0) void attachImages(files);
              }}
            />
            {activeTab.attachments.length > 0 ? (
              <div
                data-transient-chat-attachments=""
                className="flex gap-2 overflow-x-auto px-3 pt-3"
              >
                {activeTab.attachments.map((attachment) => (
                  <div
                    key={attachment.id}
                    className="group rounded-control bg-fill-quiet shadow-surface relative w-24 shrink-0 overflow-hidden"
                  >
                    <img
                      src={attachment.preview_data_url}
                      alt=""
                      className="aspect-5/3 w-full object-cover"
                    />
                    <p className="text-fine text-muted-foreground truncate px-2 py-1.5">
                      {attachment.window_title}
                    </p>
                    <Button
                      type="button"
                      variant="secondary"
                      size="icon"
                      className="absolute top-1 right-1 size-6 rounded-full opacity-85"
                      aria-label={t("composer.removeImage", {
                        title: attachment.window_title,
                      })}
                      onClick={() =>
                        updateTab(activeTab.localId, (tab) => ({
                          ...tab,
                          attachments: tab.attachments.filter(
                            (candidate) => candidate.id !== attachment.id
                          ),
                        }))
                      }
                    >
                      <X className="size-3.5" aria-hidden />
                    </Button>
                  </div>
                ))}
              </div>
            ) : null}
            <Textarea
              value={activeTab.draft}
              disabled={activeTab.running}
              focusRing={false}
              rows={3}
              aria-label={t(labels.placeholder)}
              placeholder={t(labels.placeholder)}
              className="max-h-48 min-h-20 resize-none bg-transparent px-4 pt-3 pb-2 shadow-none"
              onChange={(event) => {
                const draft = event.currentTarget.value;
                updateTab(activeTab.localId, (tab) => ({ ...tab, draft }));
              }}
              onKeyDown={(event) => {
                if (
                  event.key === "Enter" &&
                  !event.shiftKey &&
                  !event.nativeEvent.isComposing
                ) {
                  event.preventDefault();
                  void send();
                }
              }}
            />
            <div className="flex min-w-0 items-center gap-1.5 px-3 pt-1 pb-2">
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="size-8 shrink-0 rounded-full"
                aria-label={t("transientChat.add")}
                disabled={activeTab.running || activeTab.attaching}
                onClick={() => imageInputRef.current?.click()}
              >
                {activeTab.attaching ? (
                  <ActivityOrb state="connecting" aria-hidden="true" />
                ) : (
                  <Plus className="size-4" aria-hidden />
                )}
              </Button>
              <SessionModePicker
                mode={activeTab.mode}
                sandbox={activeTab.sandbox}
                disabled={activeTab.running}
                onMode={changeMode}
              />
              <div className="min-w-0 flex-1" />
              <div className="flex min-w-0 items-center gap-1.5 overflow-hidden">
                <ModelPicker
                  models={activeTab.models}
                  current={activeTab.currentModel}
                  defaultModel={activeTab.defaultModel}
                  provider={activeTab.provider}
                  configOptions={activeTab.configOptions}
                  hasSession={activeTab.sessionId !== null}
                  disabled={activeTab.running}
                  onModel={(nextModel) => {
                    const previousModel = activeTab.currentModel;
                    updateTab(activeTab.localId, (tab) => ({
                      ...tab,
                      currentModel: nextModel,
                      controlError: null,
                    }));
                    if (
                      activeTab.sessionId != null &&
                      activeTab.sessionId !== ""
                    ) {
                      void setModel(activeTab.sessionId, nextModel).catch(
                        (error: unknown) =>
                          updateTab(activeTab.localId, (tab) => ({
                            ...tab,
                            currentModel: previousModel,
                            controlError: t("toast.modelFailed", {
                              error: String(error),
                            }),
                          }))
                      );
                    }
                  }}
                  onConfigOption={(configId, value) => {
                    if (
                      activeTab.sessionId == null ||
                      activeTab.sessionId === ""
                    )
                      return;
                    const previousOptions = activeTab.configOptions;
                    updateTab(activeTab.localId, (tab) => ({
                      ...tab,
                      controlError: null,
                      configOptions: tab.configOptions.map((option) =>
                        option.id === configId
                          ? { ...option, current: value }
                          : option
                      ),
                    }));
                    void setConfigOption(
                      activeTab.sessionId,
                      configId,
                      value
                    ).catch((error: unknown) =>
                      updateTab(activeTab.localId, (tab) => ({
                        ...tab,
                        configOptions: previousOptions,
                        controlError: t("toast.modelFailed", {
                          error: String(error),
                        }),
                      }))
                    );
                  }}
                  showWhenUnavailable
                />
              </div>
              {voiceEnabled ? (
                <VoiceButton
                  onText={(text) =>
                    updateTab(activeTab.localId, (tab) => ({
                      ...tab,
                      draft: `${tab.draft}${tab.draft && !/\s$/u.test(tab.draft) ? " " : ""}${text}`,
                    }))
                  }
                />
              ) : null}
              {activeTab.running ? (
                activeTab.sessionId != null && activeTab.sessionId !== "" ? (
                  <Button
                    type="button"
                    variant="destructive"
                    size="icon"
                    className="size-8 shrink-0 rounded-full"
                    aria-label={t("composer.stop")}
                    onClick={() => void cancelTurn(activeTab.sessionId!)}
                  >
                    <Square className="size-3.5 fill-current" aria-hidden />
                  </Button>
                ) : (
                  <Button
                    type="button"
                    variant="secondary"
                    size="icon"
                    className="size-8 shrink-0 rounded-full"
                    aria-label={t("session.loading")}
                    disabled
                  >
                    <ActivityOrb state="connecting" aria-hidden="true" />
                  </Button>
                )
              ) : (
                <Button
                  type="submit"
                  size="icon"
                  variant={
                    activeTab.draft.trim() || activeTab.attachments.length > 0
                      ? "default"
                      : "secondary"
                  }
                  className="size-8 shrink-0 rounded-full"
                  aria-label={t(labels.send)}
                  disabled={activeTab.attaching}
                >
                  <ArrowUp className="size-4" aria-hidden />
                </Button>
              )}
            </div>
          </div>
        </form>
      )}
    </section>
  );
}

import { useEffect, useRef, useState, useCallback } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";

import { NavigationRow } from "@/components/business/navigation-row";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Activity,
  FolderTree,
  GitBranch,
  GitPullRequest,
  Globe,
  MessageSquare,
  TerminalIcon,
  X,
} from "@/components/ui/icons";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useResizeHandle } from "@/components/ui/use-resize-handle";
import { cn } from "@/lib/utils";

import { useT } from "../i18n";
import type { StringKey } from "../i18n/strings";
import { isOneOf } from "../lib/jsonValue";

export type DockSurface =
  | "trajectory"
  | "terminal"
  | "browser"
  | "side-chat"
  | "files"
  | "git"
  | "pull-request";
/** "home" is the dock open with nothing chosen yet — the surface picker. */
export type DockTab = DockSurface | "home";
export type DockContentMap = Partial<Record<DockSurface, ReactNode>>;

const DOCK_SURFACES = [
  "trajectory",
  "terminal",
  "browser",
  "side-chat",
  "files",
  "git",
  "pull-request",
] as const satisfies readonly DockSurface[];

/** Panel choices share one list order across picker and tabs. */
interface DockSurfaceDefinition {
  id: DockSurface;
  icon: typeof Globe;
  titleKey: StringKey;
  descKey: StringKey;
}

interface DockProps {
  /** Whether the dock is expanded. It stays mounted while closed so shells survive. */
  open: boolean;
  /** Shell-owned titlebar destination; null until the window strip mounts. */
  titlebarHost: HTMLElement | null;
  /** null while closed; the last surface stays rendered underneath the collapse animation. */
  tab: DockTab | null;
  onTab: (surface: DockSurface) => void;
  onClose: () => void;
  width: number;
  onWidth: (width: number) => void;
  /** Inline shell width that must remain beside the document while the dock is open. */
  reservedWidth?: number;
  autoTab?: DockSurface | null;
  /** Disabled surfaces are neither advertised nor mounted. */
  availableSurfaces?: DockSurface[];
  /** Content is inert until its matching surface is enabled and mounted by the container. */
  content?: DockContentMap;
}

const SURFACES: DockSurfaceDefinition[] = [
  {
    id: "trajectory",
    icon: Activity,
    titleKey: "trajectory.label",
    descKey: "dock.trajectoryDesc",
  },
  {
    id: "browser",
    icon: Globe,
    titleKey: "dock.browser",
    descKey: "dock.browserDesc",
  },
  {
    id: "terminal",
    icon: TerminalIcon,
    titleKey: "dock.terminal",
    descKey: "dock.terminalDesc",
  },
  {
    id: "side-chat",
    icon: MessageSquare,
    titleKey: "sideChat.title",
    descKey: "sideChat.temporary",
  },
  {
    id: "files",
    icon: FolderTree,
    titleKey: "dock.files",
    descKey: "dock.filesDesc",
  },
  { id: "git", icon: GitBranch, titleKey: "dock.git", descKey: "dock.gitDesc" },
  {
    id: "pull-request",
    icon: GitPullRequest,
    titleKey: "dock.pullRequest",
    descKey: "dock.pullRequestDesc",
  },
];

export const DOCK_MIN_WIDTH = 300;
export const DOCK_MAIN_MIN_WIDTH = 620;

export function dockMaxWidth(viewportWidth: number, reservedWidth = 0): number {
  return Math.max(
    DOCK_MIN_WIDTH,
    viewportWidth - reservedWidth - DOCK_MAIN_MIN_WIDTH
  );
}

export function shouldOverlayRailForWorkspace(
  viewportWidth: number,
  railWidth: number
): boolean {
  return viewportWidth < railWidth + DOCK_MAIN_MIN_WIDTH;
}

export function shouldOverlayRailForDock(
  viewportWidth: number,
  railWidth: number
): boolean {
  return viewportWidth < railWidth + DOCK_MIN_WIDTH + DOCK_MAIN_MIN_WIDTH;
}

/** Right-side container for navigation, sizing, animation, and caller-supplied surface content. */
export function Dock({
  open,
  titlebarHost,
  tab,
  onTab,
  onClose,
  width,
  onWidth,
  reservedWidth = 0,
  autoTab,
  availableSurfaces = [
    "trajectory",
    "browser",
    "terminal",
    "files",
    "git",
    "pull-request",
  ],
  content = {},
}: DockProps) {
  const t = useT();

  // What the panel shows: the live tab, or — while collapsing — whatever was open last, so the
  // content doesn't vanish mid-animation.
  const availableSurfaceSet = new Set(availableSurfaces);
  const visibleSurfaces = SURFACES.filter(({ id }) =>
    availableSurfaceSet.has(id)
  );
  const lastTab = useRef<DockTab>("home");
  if (tab) lastTab.current = tab;
  const requested = tab ?? lastTab.current;
  const shown =
    requested === "home" || availableSurfaceSet.has(requested)
      ? requested
      : "home";

  // `invisible` only after the collapse has finished: a zero-width element still paints its
  // module shadow as a hairline at the window edge, and hiding it any earlier would cut the
  // animation off — the exact stiffness this rework removes.
  const [gone, setGone] = useState(!open);
  useEffect(() => {
    if (open) {
      setGone(false);
      return;
    }
    const id = window.setTimeout(() => setGone(true), 340);
    return () => window.clearTimeout(id);
  }, [open]);

  // Never let the dock squeeze the document below a usable measure. Persist the preferred width,
  // but clamp only what is applied so it returns in full on a larger window.
  const maxForPlacement = useCallback(
    () => dockMaxWidth(window.innerWidth, reservedWidth),
    [reservedWidth]
  );
  const [maxSize, setMaxSize] = useState(maxForPlacement);
  useEffect(() => {
    const measure = () => setMaxSize(maxForPlacement());
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [maxForPlacement]);
  const applied = Math.min(width, maxSize);

  // A drag must track the pointer 1:1. The open/close width transition below would ease every
  // intermediate width instead, so the edge lags the cursor and then keeps travelling after the
  // mouse stops — and because the inner column is pinned to the new width immediately, the content
  // slides the *other* way inside the lagging panel. Dropping the transition class for the duration
  // of the drag is the fix; a CSS override can't do it, since Tailwind's utility layer wins over
  // anything we write in the components layer regardless of specificity.
  const [dragging, setDragging] = useState(false);

  const resizeHandle = useResizeHandle({
    axis: "x",
    direction: -1,
    value: applied,
    min: DOCK_MIN_WIDTH,
    max: maxSize,
    disabled: !open,
    onStart: () => {
      setDragging(true);
    },
    onResize: onWidth,
    onEnd: () => {
      setDragging(false);
      window.dispatchEvent(new Event("resize"));
    },
  });

  const renderSurfaceRow = ({
    id,
    icon: Icon,
    titleKey,
    descKey,
  }: DockSurfaceDefinition) => (
    <NavigationRow
      key={id}
      label={t(titleKey)}
      leading={<Icon className="size-4" />}
      tooltip={t(descKey)}
      onSelect={() => onTab(id)}
    />
  );

  const header = open ? (
    <div
      data-dock-titlebar
      data-orientation="horizontal"
      className={cn(
        "dock-panel-side group/tabs electrobun-webkit-app-region-drag h-full min-w-0 shrink-0 overflow-hidden",
        dragging && "dock-panel-dragging"
      )}
      style={{
        width: applied,
        height: "var(--ds-workspace-windowbar-height)",
      }}
    >
      <div className="electrobun-webkit-app-region-drag flex h-full min-w-0 items-center gap-1 px-2">
        {shown === "home" ? (
          <div className="flex-1" />
        ) : (
          <TabsList
            variant="toolbar"
            className="dock-tabs min-w-0 flex-1 justify-start overflow-x-auto"
          >
            {visibleSurfaces.map(({ id, icon: Icon, titleKey }) => (
              <TabsTrigger
                key={id}
                value={id}
                title={
                  autoTab === id
                    ? `${t(titleKey)} · ${t("dockFollow.auto")}`
                    : t(titleKey)
                }
              >
                <Icon className="size-3.5" />
                <span className="dock-tab-label">{t(titleKey)}</span>
                {autoTab === id && (
                  <span className="bg-primary size-1.5 animate-pulse rounded-full" />
                )}
              </TabsTrigger>
            ))}
          </TabsList>
        )}
        <Button
          variant="ghost"
          size="icon-xs"
          onClick={onClose}
          title={t("dock.close")}
          aria-label={t("dock.close")}
        >
          <X className="size-3.5" />
        </Button>
      </div>
    </div>
  ) : null;
  const titlebar = titlebarHost ? createPortal(header, titlebarHost) : null;

  return (
    <aside
      data-dock-placement="right"
      aria-hidden={!open}
      onTransitionEnd={(e) => {
        // Terminals and iframes fit themselves to their box — refit once the sweep lands.
        if (e.target === e.currentTarget && e.propertyName === "width")
          window.dispatchEvent(new Event("resize"));
      }}
      className={cn(
        "dock-panel dock-panel-side relative flex shrink-0 flex-col overflow-hidden",
        // The open/close sweep. Animating the real width moves the document column in the same
        // motion — the old mount-time slide left the layout to snap, which read as an animation
        // cut off halfway. It belongs to open/close only: while the grip is held, the width is the
        // pointer's to set directly.
        dragging && "dock-panel-dragging",
        gone && "invisible"
      )}
      style={{ width: open ? applied : 0 }}
    >
      <div
        data-dock-resize="horizontal"
        className="dock-grip"
        aria-label={t("dock.resize")}
        title={t("dock.resize")}
        {...resizeHandle}
      />

      {/* Pin the animated dimension so panel content does not reflow while it sweeps. */}
      <Card
        variant="raised"
        className="m-2 min-h-0 min-w-0 flex-1 gap-0 overflow-hidden py-0"
        style={{ width: applied - 16 }}
      >
        {shown === "home" ? (
          <>
            {titlebar}
            <div className="flex min-h-0 flex-1 flex-col overflow-y-auto p-6">
              <div className="mx-auto my-auto w-full max-w-xs shrink-0">
                <h2 className="text-body text-center font-medium">
                  {t("dock.openSurface")}
                </h2>
                <div className="dock-surface-list mt-4 flex flex-col gap-1">
                  {visibleSurfaces.map(renderSurfaceRow)}
                </div>
              </div>
            </div>
          </>
        ) : (
          <Tabs
            value={shown}
            onValueChange={(v) => {
              if (isOneOf(v, DOCK_SURFACES)) onTab(v);
            }}
            className="flex min-h-0 min-w-0 flex-1 flex-col gap-0 overflow-hidden"
          >
            {titlebar}

            {visibleSurfaces.map(({ id }) => (
              <TabsContent
                key={id}
                value={id}
                className="m-0 flex min-h-0 min-w-0 flex-1 overflow-hidden"
              >
                {content[id]}
              </TabsContent>
            ))}
          </Tabs>
        )}
      </Card>
    </aside>
  );
}

import { useEffect, useState } from "react";

import { CompositeActionRow } from "@/components/business/composite-action-row";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { cn } from "@/lib/utils";

import { AssistantWorkspace } from "../assistant/AssistantWorkspace";
import { providerLabel, sessionDiffStat } from "../bridge";
import type { SessionDiffStat, SessionInfo } from "../bridge";
import { useLanguage, useT } from "../i18n";
import { td } from "../i18n/dynamic";
import { ProviderIcon } from "../providers/ProviderIcon";
import { describeContextWindow } from "../session/contextWindow";
import type { ContextWindowBySession } from "../session/contextWindow";
// Explicit extension: Bun's directory cache is case-insensitive, and `missionControl` without an
// extension resolves against `MissionControl.tsx` (this file) when both live in one directory.
import { missionRows } from "./missionControl.ts";
import type { MissionRow, MissionState } from "./missionControl.ts";

/** The rail's color semantics, one dot per state: amber asks, red failed, primary at work. */
const DOT_CLASS: Record<MissionState, string> = {
  awaiting_input: "bg-warning animate-pulse",
  failed: "bg-destructive",
  running: "bg-primary animate-pulse",
  idle: "bg-muted-foreground/40",
};

/** A scene reference like `builtin:develop` reads better as its short name. */
function sceneLabel(reference: string): string {
  const colon = reference.lastIndexOf(":");
  return colon === -1 ? reference : reference.slice(colon + 1);
}

/**
 * Diff stats are fetched per row, not per open: the dialog is a glance surface, and a session's
 * working-tree stat is stable enough to cache for the app's lifetime once seen.
 */
const diffStatCache = new Map<string, SessionDiffStat | null>();

/** Spinner → `+a −d · n files` → em dash when the checkout is gone or not a repo. */
export function DiffStatCell({
  session,
  fetchStat = sessionDiffStat,
}: {
  session: string;
  /** Injectable for tests; defaults to the bridge call. */
  fetchStat?: (session: string) => Promise<SessionDiffStat | null>;
}) {
  const t = useT();
  const [stat, setStat] = useState<SessionDiffStat | null | undefined>(() =>
    diffStatCache.has(session) ? diffStatCache.get(session) : undefined
  );
  useEffect(() => {
    if (diffStatCache.has(session)) {
      setStat(diffStatCache.get(session));
      return;
    }
    let cancelled = false;
    void fetchStat(session).then((value) => {
      diffStatCache.set(session, value);
      if (!cancelled) setStat(value);
    });
    return () => {
      cancelled = true;
    };
  }, [session, fetchStat]);

  if (stat === undefined) {
    return <Spinner className="text-muted-foreground size-3" />;
  }
  if (stat === null) return <span className="text-muted-foreground">—</span>;
  return (
    <span className="text-callout whitespace-nowrap tabular-nums">
      <span className="text-success">+{stat.additions}</span>{" "}
      <span className="text-destructive">−{stat.deletions}</span>
      <span className="text-muted-foreground">
        {" "}
        · {t("mission.files", { n: stat.files })}
      </span>
    </span>
  );
}

/**
 * R6 (docs/roadmap.md): the cross-session overview answering "what needs me" — every session's
 * state, scene, working-tree diff, and context occupancy, with one click into review.
 */
export function MissionControlPage({
  sessions,
  runningSessions,
  contextWindows,
  sceneBySession,
  onSelect,
  onReview,
  onClose,
  fetchStat,
}: {
  sessions: SessionInfo[];
  runningSessions: ReadonlySet<string>;
  contextWindows: ContextWindowBySession;
  sceneBySession: ReadonlyMap<string, string>;
  onSelect: (id: string) => void;
  onReview: (id: string) => void;
  onClose: () => void;
  /** Injectable for tests; defaults to the bridge call. */
  fetchStat?: (session: string) => Promise<SessionDiffStat | null>;
}) {
  const t = useT();
  const { locale } = useLanguage();
  const rows = missionRows(
    sessions,
    runningSessions,
    contextWindows,
    sceneBySession
  );

  const row = (r: MissionRow) => {
    const s = r.session;
    const context = describeContextWindow(contextWindows[s.id] ?? null);
    return (
      <CompositeActionRow
        key={s.id}
        accessibilityLabel={s.title}
        onSelect={() => {
          onSelect(s.id);
          onClose();
        }}
        className="rounded-control hover:bg-accent/50 focus-within:bg-accent/50 gap-3 px-2 py-1.5 transition-colors"
        contentClassName="flex items-center gap-3"
        actions={
          <Button
            variant="secondary"
            size="sm"
            className="shrink-0"
            onClick={() => {
              onSelect(s.id);
              onReview(s.id);
              onClose();
            }}
          >
            {t("mission.review")}
          </Button>
        }
      >
        <span
          className={cn("size-2 shrink-0 rounded-full", DOT_CLASS[r.state])}
          title={td(t, `mission.state.${r.state}`)}
          aria-label={td(t, `mission.state.${r.state}`)}
        />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <span className="text-body min-w-0 truncate">{s.title}</span>
            {r.scene != null && r.scene !== "" && (
              <Badge
                variant="outline"
                className="text-metadata text-muted-foreground shrink-0"
              >
                {sceneLabel(r.scene)}
              </Badge>
            )}
          </div>
          <div className="text-callout text-muted-foreground flex items-center gap-1">
            <ProviderIcon
              provider={providerLabel(s.provider)}
              className="size-3 shrink-0 opacity-70"
            />
            <span className="min-w-0 truncate">
              {td(t, `mission.state.${r.state}`)}
            </span>
          </div>
          <p
            className="text-metadata text-muted-foreground truncate"
            title={s.project_path ?? s.cwd}
          >
            {(s.project_path ?? s.cwd)?.split("/").filter(Boolean).at(-1) ??
              "—"}{" "}
            · {new Date(s.created_at).toLocaleString(locale)}
          </p>
        </div>
        <DiffStatCell session={s.id} fetchStat={fetchStat} />
        <span
          className="text-callout text-muted-foreground w-10 shrink-0 text-right tabular-nums"
          title={`${t("mission.context")}: ${context?.exact ?? "—"}`}
          aria-label={`${t("mission.context")}: ${context?.exact ?? "—"}`}
        >
          {r.contextPct === null ? "—" : `${Math.round(r.contextPct)}%`}
        </span>
      </CompositeActionRow>
    );
  };

  return (
    <AssistantWorkspace
      onSelect={onSelect}
      onClose={onClose}
      activity={
        rows.length === 0 ? (
          <p className="text-callout text-muted-foreground px-2 py-4">
            {t("mission.empty")}
          </p>
        ) : (
          <div className="max-h-96 space-y-px overflow-y-auto">
            {rows.map(row)}
          </div>
        )
      }
    />
  );
}

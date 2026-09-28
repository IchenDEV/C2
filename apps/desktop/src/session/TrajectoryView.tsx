import { useDeferredValue, useEffect, useRef, useState } from "react";

import { SearchField } from "@/components/business/search-field";
import { Button } from "@/components/ui/button";
import { ArrowDown, ChevronDown, ChevronRight } from "@/components/ui/icons";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { TooltipButton } from "@/components/ui/tooltip";
import { useLanguage, useT } from "@/i18n";
import type { StringKey } from "@/i18n/strings";
import { cn } from "@/lib/utils";

import {
  deriveTrajectory,
  filterTrajectory,
  formatTrajectoryDuration,
} from "./trajectory";
import type {
  TrajectoryKind,
  TrajectoryLane,
  TrajectoryRecord,
} from "./trajectory";
import type { Turn } from "./turns";

const KIND_LABEL: Record<TrajectoryKind, StringKey> = {
  user: "trajectory.kind.user",
  assistant: "trajectory.kind.assistant",
  reasoning: "trajectory.kind.reasoning",
  tool: "trajectory.kind.tool",
  memory: "trajectory.kind.memory",
  error: "trajectory.kind.error",
};

const LANE_LABEL: Record<TrajectoryLane, StringKey> = {
  context: "trajectory.lane.context",
  assistant: "trajectory.lane.assistant",
  tool: "trajectory.lane.tool",
};

const KIND_TONE: Record<TrajectoryKind, string> = {
  user: "bg-primary",
  assistant: "bg-foreground",
  reasoning: "bg-muted-foreground",
  tool: "bg-warning",
  memory: "bg-success",
  error: "bg-destructive",
};

const FILTER_KINDS: (TrajectoryKind | "all")[] = [
  "all",
  "user",
  "assistant",
  "reasoning",
  "tool",
  "memory",
  "error",
];

function safeDetail(value: unknown): { text: string; object: boolean } | null {
  if (value === undefined || value === null || value === "") return null;
  const object = typeof value !== "string";
  let text: string;
  try {
    text = object ? JSON.stringify(value, null, 2) : value;
  } catch {
    text = String(value);
  }
  const limit = 30_000;
  return {
    object,
    text: text.length > limit ? `${text.slice(0, limit)}\n…` : text,
  };
}

function DetailBlock({ label, value }: { label: string; value: unknown }) {
  const detail = safeDetail(value);
  if (!detail) return null;
  return (
    <section className="flex flex-col gap-2">
      <h3 className="text-callout text-foreground font-medium">{label}</h3>
      <pre
        className={cn(
          "rounded-control bg-fill-quiet text-callout text-foreground max-h-72 overflow-auto p-3 break-words whitespace-pre-wrap",
          detail.object ? "font-mono" : "font-sans"
        )}
      >
        {detail.text}
      </pre>
    </section>
  );
}

interface PackedRecord {
  record: TrajectoryRecord;
  track: number;
}

function packLane(records: readonly TrajectoryRecord[]): {
  records: PackedRecord[];
  tracks: number;
} {
  const ends: number[] = [];
  const packed = [...records]
    .toSorted(
      (left, right) => left.startAt - right.startAt || left.index - right.index
    )
    .map((record) => {
      let track = ends.findIndex((end) => record.startAt >= end);
      if (track < 0) track = ends.length;
      ends[track] = Math.max(record.startAt, record.endAt);
      return { record, track };
    });
  return { records: packed, tracks: Math.max(1, ends.length) };
}

function Timeline({
  records,
  selectedId,
  onSelect,
}: {
  records: readonly TrajectoryRecord[];
  selectedId: string | null;
  onSelect: (record: TrajectoryRecord) => void;
}) {
  const t = useT();
  const startAt = Math.min(...records.map((record) => record.startAt));
  const rawEnd = Math.max(...records.map((record) => record.endAt));
  const endAt = Math.max(startAt + 1000, rawEnd);
  const span = endAt - startAt;
  const lanes: TrajectoryLane[] = ["context", "assistant", "tool"];

  return (
    <section
      className="bg-fill-quiet shrink-0 px-4 py-3"
      aria-labelledby="trajectory-overview-title"
    >
      <div className="mb-2 flex items-center justify-between gap-4">
        <h2 id="trajectory-overview-title" className="text-body font-medium">
          {t("trajectory.overview")}
        </h2>
        <span className="text-metadata text-muted-foreground font-mono">
          {formatTrajectoryDuration(span)}
        </span>
      </div>
      <div
        className="trajectory-overview-grid"
        role="group"
        aria-label={t("trajectory.overview")}
      >
        {lanes.map((lane) => {
          const packed = packLane(
            records.filter((record) => record.lane === lane)
          );
          const height = Math.max(24, packed.tracks * 12 + 8);
          return (
            <div key={lane} className="contents">
              <span className="text-metadata text-muted-foreground self-center pe-3">
                {t(LANE_LABEL[lane])}
              </span>
              <div className="trajectory-lane bg-background" style={{ height }}>
                {packed.records.map(({ record, track }) => {
                  const left = ((record.startAt - startAt) / span) * 100;
                  const width =
                    ((Math.max(record.endAt, record.startAt) - record.startAt) /
                      span) *
                    100;
                  const selected = record.id === selectedId;
                  return (
                    <TooltipButton
                      key={record.id}
                      type="button"
                      variant="ghost"
                      size="icon-xs"
                      focusStyle="inset"
                      className={cn(
                        "trajectory-bar",
                        KIND_TONE[record.kind],
                        selected
                          ? "ring-foreground/50 opacity-100 ring-2"
                          : "opacity-60 hover:opacity-90"
                      )}
                      style={{
                        insetInlineStart: `${left}%`,
                        width: `${width}%`,
                        insetBlockStart: track * 12 + 4,
                      }}
                      label={`${t(KIND_LABEL[record.kind])}: ${record.summary}`}
                      tooltip={`${record.title} · ${record.startAt > 0 && record.endAt >= record.startAt ? formatTrajectoryDuration(record.endAt - record.startAt) : "—"}`}
                      aria-pressed={selected}
                      onClick={() => onSelect(record)}
                    />
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}

function EventMarker({ kind }: { kind: TrajectoryKind }) {
  return (
    <span
      className={cn("rounded-control size-2 shrink-0", KIND_TONE[kind])}
      aria-hidden
    />
  );
}

function LedgerRow({
  record,
  selected,
  onSelect,
}: {
  record: TrajectoryRecord;
  selected: boolean;
  onSelect: () => void;
}) {
  const t = useT();
  return (
    <Button
      type="button"
      variant="selectable"
      size="row"
      focusStyle="inset"
      data-selected={selected ? "true" : "false"}
      aria-pressed={selected}
      className={cn(
        "trajectory-ledger-row w-full",
        selected
          ? "bg-fill-rest text-foreground"
          : "bg-background text-muted-foreground"
      )}
      style={{
        contentVisibility: "auto",
        containIntrinsicSize: "36px",
      }}
      onClick={onSelect}
    >
      <span className="text-metadata px-3 py-2 font-mono tabular-nums">
        {String(record.index).padStart(3, "0")}
      </span>
      <span className="text-callout flex min-w-0 items-center gap-2 px-3 py-2">
        <EventMarker kind={record.kind} />
        <span className="truncate">{t(KIND_LABEL[record.kind])}</span>
      </span>
      <span className="text-callout min-w-0 px-3 py-2">
        <span className="text-foreground block truncate">
          {record.summary || record.title}
        </span>
        {record.status != null && record.status !== "" ? (
          <span className="text-metadata text-muted-foreground block truncate">
            {record.title} · {record.status}
          </span>
        ) : null}
      </span>
    </Button>
  );
}

function Inspector({
  record,
  formatClock,
}: {
  record: TrajectoryRecord | null;
  formatClock: (timestamp: number) => string;
}) {
  const t = useT();
  if (!record) {
    return (
      <aside className="trajectory-inspector bg-fill-quiet text-callout text-muted-foreground flex items-center justify-center p-6 text-center">
        {t("trajectory.inspectHint")}
      </aside>
    );
  }
  return (
    <aside
      className="trajectory-inspector bg-fill-quiet min-h-0 overflow-y-auto p-4"
      aria-label={t("trajectory.inspector")}
    >
      <div className="flex items-start gap-2">
        <EventMarker kind={record.kind} />
        <div className="min-w-0 flex-1">
          <h2 className="text-body text-foreground truncate font-medium">
            {record.title}
          </h2>
          <p className="text-callout text-muted-foreground mt-1">
            {record.step > 0
              ? t("trajectory.turnStep", {
                  turn: record.turn,
                  step: record.step,
                })
              : t("trajectory.turn", { turn: record.turn })}
          </p>
        </div>
        {record.status != null && record.status !== "" ? (
          <span className="text-metadata text-muted-foreground shrink-0">
            {record.status}
          </span>
        ) : null}
      </div>

      <dl className="trajectory-timing bg-background text-callout mt-4">
        <div>
          <dt>{t("trajectory.started")}</dt>
          <dd>{formatClock(record.startAt)}</dd>
        </div>
        <div>
          <dt>{t("trajectory.ended")}</dt>
          <dd>
            {record.running
              ? t("trajectory.running")
              : formatClock(record.endAt)}
          </dd>
        </div>
        <div>
          <dt>{t("trajectory.duration")}</dt>
          <dd>
            {record.startAt > 0 && record.endAt >= record.startAt
              ? formatTrajectoryDuration(record.endAt - record.startAt)
              : "—"}
          </dd>
        </div>
      </dl>

      <div className="mt-4 flex flex-col gap-4">
        <DetailBlock label={t("trajectory.input")} value={record.input} />
        <DetailBlock label={t("trajectory.output")} value={record.output} />
      </div>
    </aside>
  );
}

export function TrajectoryView({
  turns,
  usage,
  hasEarlier,
  loadingEarlier,
  onLoadEarlier,
}: {
  turns: readonly Turn[];
  usage: { input_tokens: number; output_tokens: number } | null;
  hasEarlier: boolean;
  loadingEarlier: boolean;
  onLoadEarlier: () => void;
}) {
  const t = useT();
  const { locale } = useLanguage();
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query);
  const [kind, setKind] = useState<TrajectoryKind | "all">("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [collapsedTurns, setCollapsedTurns] = useState<Set<number>>(
    () => new Set()
  );
  const [following, setFollowing] = useState(true);
  const [now, setNow] = useState(() => Date.now());
  const ledgerRef = useRef<HTMLDivElement | null>(null);

  const running = turns.some((turn) => turn.endedAt === undefined);
  const records = deriveTrajectory(turns, now);
  const visibleRecords = filterTrajectory(records, kind, deferredQuery);
  const selected =
    visibleRecords.find((record) => record.id === selectedId) ??
    visibleRecords.at(-1)! ??
    null;
  const tailAt = records.at(-1)?.endAt ?? 0;
  const turnsWithRecords = (() => {
    const grouped = new Map<number, TrajectoryRecord[]>();
    for (const record of visibleRecords) {
      const group = grouped.get(record.turn) ?? [];
      group.push(record);
      grouped.set(record.turn, group);
    }
    return [...grouped.entries()];
  })();
  const clock = new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    fractionalSecondDigits: 3,
  });
  const formatClock = (timestamp: number) => clock.format(new Date(timestamp));

  useEffect(() => {
    setNow(Date.now());
    if (!running) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [running, turns]);

  useEffect(() => {
    if (!following) return;
    const frame = requestAnimationFrame(() => {
      const ledger = ledgerRef.current;
      if (ledger) ledger.scrollTop = ledger.scrollHeight;
    });
    return () => cancelAnimationFrame(frame);
  }, [following, tailAt]);

  useEffect(() => {
    const ledger = ledgerRef.current;
    if (!following || !ledger || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      ledger.scrollTop = ledger.scrollHeight;
    });
    observer.observe(ledger);
    return () => observer.disconnect();
  }, [following]);

  const jumpToLatest = () => {
    setFollowing(true);
    const ledger = ledgerRef.current;
    if (ledger) ledger.scrollTop = ledger.scrollHeight;
  };

  const toggleTurn = (turn: number) => {
    setCollapsedTurns((current) => {
      const next = new Set(current);
      if (next.has(turn)) next.delete(turn);
      else next.add(turn);
      return next;
    });
  };

  return (
    <section
      className="trajectory-view bg-background flex min-h-0 flex-1 flex-col"
      aria-label={t("trajectory.label")}
    >
      <header className="bg-background flex shrink-0 flex-wrap items-center gap-2 px-4 py-2">
        <SearchField
          className="min-w-48 flex-1"
          value={query}
          placeholder={t("trajectory.searchPlaceholder")}
          label={t("trajectory.search")}
          onChange={(event) => setQuery(event.currentTarget.value)}
        />
        <Select value={kind} onValueChange={(value) => value && setKind(value)}>
          <SelectTrigger size="sm" aria-label={t("trajectory.filter")}>
            <SelectValue>
              {kind === "all" ? t("trajectory.filterAll") : t(KIND_LABEL[kind])}
            </SelectValue>
          </SelectTrigger>
          <SelectContent position="popper" align="end">
            <SelectGroup>
              {FILTER_KINDS.map((value) => (
                <SelectItem key={value} value={value}>
                  {value === "all"
                    ? t("trajectory.filterAll")
                    : t(KIND_LABEL[value])}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
        <span className="text-metadata text-muted-foreground shrink-0 font-mono">
          {t("trajectory.eventCount", { count: visibleRecords.length })}
          {usage
            ? ` · ${t("trajectory.tokens", {
                input: usage.input_tokens.toLocaleString(locale),
                output: usage.output_tokens.toLocaleString(locale),
              })}`
            : ""}
        </span>
      </header>

      {visibleRecords.length > 0 ? (
        <Timeline
          records={visibleRecords}
          selectedId={selected?.id ?? null}
          onSelect={(record) => setSelectedId(record.id)}
        />
      ) : null}

      <div className="trajectory-workbench min-h-0 flex-1">
        <section
          className="bg-background relative flex min-h-0 flex-col"
          aria-label={t("trajectory.ledger")}
        >
          <div
            aria-hidden
            className="trajectory-ledger-row bg-fill-quiet text-metadata text-muted-foreground shrink-0 font-medium"
          >
            <span className="px-3 py-2">{t("trajectory.index")}</span>
            <span className="px-3 py-2">{t("trajectory.event")}</span>
            <span className="px-3 py-2">{t("trajectory.content")}</span>
          </div>
          <div
            ref={ledgerRef}
            className="min-h-0 flex-1 overflow-y-auto overscroll-contain"
            onScroll={(event) => {
              const element = event.currentTarget;
              setFollowing(
                element.scrollHeight -
                  element.scrollTop -
                  element.clientHeight <
                  40
              );
            }}
          >
            {hasEarlier ? (
              <div className="flex justify-center px-3 py-2">
                <Button
                  type="button"
                  size="compact"
                  variant="ghost"
                  disabled={loadingEarlier}
                  onClick={onLoadEarlier}
                >
                  {loadingEarlier ? <Spinner data-icon="inline-start" /> : null}
                  {loadingEarlier
                    ? t("transcript.loadingEarlier")
                    : t("transcript.loadEarlier")}
                </Button>
              </div>
            ) : null}
            {turnsWithRecords.length === 0 ? (
              <p className="text-callout text-muted-foreground p-6 text-center">
                {t("trajectory.noEvents")}
              </p>
            ) : (
              turnsWithRecords.map(([turn, group]) => {
                const collapsed = collapsedTurns.has(turn);
                return (
                  <section
                    key={turn}
                    aria-label={t("trajectory.turn", { turn })}
                  >
                    <Button
                      type="button"
                      variant="ghost"
                      size="row"
                      focusStyle="inset"
                      className="bg-fill-quiet text-metadata text-muted-foreground w-full gap-2 px-3 py-2 font-medium"
                      aria-expanded={!collapsed}
                      onClick={() => toggleTurn(turn)}
                    >
                      {collapsed ? (
                        <ChevronRight aria-hidden className="size-3" />
                      ) : (
                        <ChevronDown aria-hidden className="size-3" />
                      )}
                      <span>{t("trajectory.turn", { turn })}</span>
                      <span className="font-mono">{group.length}</span>
                    </Button>
                    {collapsed
                      ? null
                      : group.map((record) => (
                          <LedgerRow
                            key={record.id}
                            record={record}
                            selected={record.id === selected?.id}
                            onSelect={() => setSelectedId(record.id)}
                          />
                        ))}
                  </section>
                );
              })
            )}
          </div>
          {following ? null : (
            <Button
              type="button"
              size="compact"
              variant="secondary"
              className="absolute start-1/2 bottom-3 -translate-x-1/2"
              onClick={jumpToLatest}
            >
              <ArrowDown data-icon="inline-start" aria-hidden />
              {t("transcript.jumpLatest")}
            </Button>
          )}
        </section>
        <Inspector record={selected} formatClock={formatClock} />
      </div>
    </section>
  );
}

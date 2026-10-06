import { useRef } from "react";
import type { ReactNode } from "react";

import { ActivityOrb } from "@/components/ui/activity-orb";
import { Button } from "@/components/ui/button";
import { ArrowDown } from "@/components/ui/icons";
import { Spinner } from "@/components/ui/spinner";
import { useT } from "@/i18n";
import { cn } from "@/lib/utils";

import type { BuiltinLinkActions } from "./MarkdownContent";
import { SelectionActions } from "./SelectionActions";
import { TurnCard } from "./TurnCard";
import type { Turn } from "./turns";
import type { TranscriptScrollController } from "./useTranscriptScroll";
import { useTranscriptScroll } from "./useTranscriptScroll";

interface TranscriptPaneProps {
  variant: "main" | "side";
  turns: readonly Turn[];
  loading?: boolean;
  hasEarlier?: boolean;
  loadingEarlier?: boolean;
  onLoadEarlier?: (scroll: TranscriptScrollController) => void;
  /** R2 "Save as template…" in each turn's prompt menu. Absent → the menu stays hidden. */
  onSaveTemplate?: (promptText: string) => void;
  linkActions?: BuiltinLinkActions;
  /** Durable source session used for scroll restoration. */
  sessionId?: string | null;
  onForkTurn?: (turn: Turn) => void;
  /** Selection actions render only when the host supplies all three. */
  onAddSelection?: (text: string) => void;
  onExplainSelection?: (text: string) => void;
  onAskSelectionInSideChat?: (text: string) => void;
  /** Host-rendered declarative plugin actions above the transcript. */
  before?: ReactNode;
  /** Conversation mode: unfinished replies are not shown as running work. */
  quiet?: boolean;
  /** Records attached under one turn (questions, deliveries, …). */
  renderAfter?: (turn: Turn) => ReactNode;
  /** Live records after the last turn, inside the same scroll viewport. */
  after?: ReactNode;
  /** Shown instead of the list while there are no turns. */
  empty?: ReactNode;
  /** False while a hosting view is hidden; the scroll offset is restored on return. */
  active?: boolean;
  label?: string;
}

/** One transcript renderer shared by the main column and document-mode side panel. */
export function TranscriptPane({
  sessionId,
  variant,
  turns,
  loading = false,
  hasEarlier = false,
  loadingEarlier = false,
  onLoadEarlier,
  onSaveTemplate,
  linkActions,
  onForkTurn,
  onAddSelection,
  onExplainSelection,
  onAskSelectionInSideChat,
  before,
  quiet = false,
  renderAfter,
  after,
  empty,
  active = true,
  label,
}: TranscriptPaneProps) {
  const t = useT();
  const scroll = useTranscriptScroll(sessionId ?? null, turns, active);
  const Root = variant === "side" ? "aside" : "section";
  const selectionScopeRef = useRef<HTMLDivElement | null>(null);

  return (
    <Root
      aria-label={label ?? t("transcript.label")}
      className={cn(
        "relative min-h-0",
        variant === "side"
          ? "animate-slide-in-right bg-fill-quiet order-2 w-[560px] max-w-[55%] shrink-0 border-s @max-[48rem]/workspace:order-1 @max-[48rem]/workspace:h-1/2 @max-[48rem]/workspace:w-full @max-[48rem]/workspace:max-w-none"
          : "order-1 flex-1"
      )}
    >
      <div
        ref={(element) => {
          selectionScopeRef.current = element;
          scroll.viewportRef.current = element;
        }}
        aria-busy={loading}
        className="size-full overflow-y-auto overscroll-contain"
        onScroll={scroll.onScroll}
        onPointerDownCapture={scroll.onPointerDownCapture}
        onKeyDownCapture={scroll.onKeyDownCapture}
      >
        <div
          className={cn(
            "mx-auto w-full",
            variant === "side" ? "px-4 pt-4 pb-6" : "max-w-3xl px-8 pt-6 pb-8"
          )}
        >
          {before}
          {loading ? (
            <p
              role="status"
              className="text-body text-muted-foreground flex items-center justify-center gap-2 py-12"
            >
              <ActivityOrb state="connecting" aria-hidden="true" />
              {t("session.loading")}
            </p>
          ) : (
            <>
              {hasEarlier ? (
                <div className="flex justify-center pb-2">
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    disabled={loadingEarlier}
                    onClick={() => onLoadEarlier?.(scroll)}
                  >
                    {loadingEarlier ? (
                      <Spinner data-icon="inline-start" />
                    ) : null}
                    {loadingEarlier
                      ? t("transcript.loadingEarlier")
                      : t("transcript.loadEarlier")}
                  </Button>
                </div>
              ) : null}
              {turns.length === 0 ? empty : null}
              <ol className="m-0 list-none p-0">
                {turns.map((turn) => (
                  <li
                    key={turn.transcriptStartSeq ?? turn.id}
                    className="transcript-turn"
                  >
                    <TurnCard
                      turn={turn}
                      onSaveTemplate={onSaveTemplate}
                      linkActions={linkActions}
                      onFork={onForkTurn}
                      quiet={quiet}
                      after={renderAfter?.(turn)}
                    />
                  </li>
                ))}
              </ol>
              {after}
            </>
          )}
        </div>
      </div>

      {onAddSelection && onExplainSelection && onAskSelectionInSideChat ? (
        <SelectionActions
          scopeRef={selectionScopeRef}
          onAdd={onAddSelection}
          onDetails={onExplainSelection}
          onAskInSideChat={onAskSelectionInSideChat}
        />
      ) : null}

      {scroll.showJumpToLatest ? (
        <Button
          type="button"
          size="sm"
          variant="secondary"
          className="rounded-control absolute start-1/2 bottom-4 -translate-x-1/2 shadow-lg"
          onClick={scroll.jumpToLatest}
        >
          <ArrowDown data-icon="inline-start" aria-hidden />
          {t("transcript.jumpLatest")}
        </Button>
      ) : null}
    </Root>
  );
}

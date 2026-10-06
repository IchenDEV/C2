import { useMemo, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  ChevronDown,
  Clock3,
  Eye,
  Folder,
  FolderTree,
  Globe,
  Pause,
  Settings,
} from "@/components/ui/icons";
import { TooltipButton } from "@/components/ui/tooltip";

import type { DocBlock } from "../bridge";
import { DocEditor } from "../editor/Editor";
import { ComposerCard, ComposerSendButton } from "../session/Composer";
import { TranscriptPane } from "../session/TranscriptPane";
import type { Turn } from "../session/turns";
import { GLOBAL_MEMORY } from "./api";
import type { AssistantState, ConversationTurn } from "./api";
import {
  DeliveryCard,
  intakeLabel,
  MemoryProposalCard,
  NotificationCard,
  ProposalCard,
  QuestionCard,
} from "./ThreadCards";
import type { CardContext } from "./ThreadCards";

const OPEN_QUESTION = new Set(["open", "expired", "delivery_unknown"]);

function timeMs(value: string | number) {
  const ms = new Date(
    typeof value === "number" && value < 1e12 ? value * 1000 : value
  ).getTime();
  return Number.isNaN(ms) ? Date.now() : ms;
}

/**
 * A read-only display adapter: one durable record per row, in arrival order. Replies can arrive
 * independently, so grouping them under an earlier prompt would reorder the conversation.
 */
export function conversationTurns(rows: ConversationTurn[]): Turn[] {
  return rows.map((row, index) => {
    const startedAt = timeMs(row.created_at);
    const assistant = row.author === "assistant";
    return {
      id: index,
      requestId: row.id,
      transcriptStartSeq: index,
      accepted: true,
      streamBoundaryKnown: false,
      prompt: assistant ? "" : row.content,
      text: assistant ? row.content : "",
      textDeltas: [],
      observedTextDeltas: 0,
      observedThoughtDeltas: 0,
      pendingTextDeltaSkips: 0,
      pendingThoughtDeltaSkips: 0,
      thoughts: [],
      tools: [],
      content: assistant
        ? [{ kind: "text", text: row.content, transcriptSeq: index }]
        : [],
      startedAt,
      endedAt: startedAt,
    };
  });
}

/** One client id per distinct message, kept across failed attempts so Core can deduplicate. */
function newTurnId() {
  return globalThis.crypto.randomUUID();
}

export function ChiefThread({
  state,
  managed,
  active,
  ctx,
  onConfigure,
}: {
  state: AssistantState;
  managed: { path: string; name: string }[];
  active: boolean;
  ctx: CardContext;
  onConfigure: () => void;
}) {
  const { zh } = ctx;
  const [hints, setHints] = useState<string[]>([]);
  const [sending, setSending] = useState(false);
  const [notSent, setNotSent] = useState(false);
  const [unsupported, setUnsupported] = useState(false);
  const [empty, setEmpty] = useState(true);
  const [earlierOpen, setEarlierOpen] = useState(false);
  const [scopeOpen, setScopeOpen] = useState(false);
  const [selectedScope, setSelectedScope] = useState<string>("global");
  const [observeOpen, setObserveOpen] = useState(false);

  const activeProjects = (state.hierarchy?.projects ?? []).filter(
    (p) => p.status === "active"
  );
  const selectedProject =
    selectedScope === "global"
      ? null
      : (state.hierarchy?.projects.find((p) => p.id === selectedScope) ?? null);
  const currentScopeName =
    selectedScope === "global"
      ? zh
        ? "全局"
        : "Global"
      : (selectedProject?.name ?? selectedScope);
  const attempt = useRef<{ id: string; key: string } | null>(null);
  const getBlocksRef = useRef<(() => DocBlock[]) | null>(null);
  const getMarkdownRef = useRef<(() => Promise<string | null>) | null>(null);
  const sendingRef = useRef(false);
  const focusRef = useRef<(() => void) | null>(null);
  const clearRef = useRef<(() => void) | null>(null);

  const turns = state.conversation ?? [];
  const goalById = new Map(state.goals.map((g) => [g.id, g]));
  const shown = new Set<string>();
  const questionById = new Map((state.questions ?? []).map((q) => [q.id, q]));
  const proposals = state.memory_proposals ?? [];

  const refs = (turn: ConversationTurn) => (
    <>
      {turn.goal_ids.map((id) => {
        const g = goalById.get(id);
        if (!g || shown.has(`g:${id}`)) return null;
        shown.add(`g:${id}`);
        return <DeliveryCard key={`g:${id}`} goal={g} ctx={ctx} />;
      })}
      {turn.question_ids.map((id) => {
        const q = questionById.get(id);
        if (!q || !OPEN_QUESTION.has(q.state) || shown.has(`q:${id}`))
          return null;
        shown.add(`q:${id}`);
        return <QuestionCard key={`q:${id}`} question={q} ctx={ctx} />;
      })}
      {proposals
        .filter((p) => p.turn_id === turn.id)
        .map((p) => {
          shown.add(`m:${p.id}`);
          return (
            <MemoryProposalCard
              key={p.id}
              proposal={p}
              global={GLOBAL_MEMORY}
              ctx={ctx}
            />
          );
        })}
    </>
  );

  // Records and intake state hang under the user message they belong to. Every row's refs are
  // computed in original record order so a record is placed once, under its first mention.
  const afterByTurn = new Map<string, React.ReactNode>();
  const runById = new Map(state.scoped_runs?.map((r) => [r.run.id, r]));
  const runByScope = new Map(state.scoped_runs?.map((r) => [r.scope, r]));
  for (const turn of turns) {
    const mine = turn.author === "user";
    const run =
      (turn.source_run_id == null
        ? undefined
        : runById.get(turn.source_run_id)) ?? runByScope.get(`turn:${turn.id}`);
    afterByTurn.set(
      turn.id,
      <div
        className="mt-3 space-y-2 empty:hidden"
        data-conversation-id={turn.id}
        data-reply-to={turn.reply_to ?? undefined}
      >
        {turn.actor != null && turn.actor !== "" && (
          <div
            className="text-muted-foreground flex items-center gap-1.5 pb-0.5 text-xs"
            data-testid={`turn-actor-${turn.id}`}
          >
            {turn.actor === "chief" ? (
              <>
                <Globe className="size-3.5 shrink-0" aria-hidden />
                <span className="font-medium">{zh ? "全局" : "Global"}</span>
              </>
            ) : (
              <>
                <Folder className="size-3.5 shrink-0" aria-hidden />
                <span className="font-medium">
                  {state.hierarchy?.projects.find(
                    (p) =>
                      turn.actor === `project:${p.id}` || turn.actor === p.id
                  )?.name ?? turn.actor}
                </span>
              </>
            )}
          </div>
        )}
        {mine && ["recorded", "reviewing"].includes(turn.status) && (
          <TooltipButton
            label={intakeLabel(turn.status, zh)}
            variant="ghost"
            size="icon-xs"
            disabled
          >
            <Clock3 aria-hidden />
          </TooltipButton>
        )}
        {mine && ["failed", "unknown"].includes(turn.status) && (
          <p role="status" className="text-warning text-callout">
            {intakeLabel(turn.status, zh)}
          </p>
        )}
        {mine && (turn.error ?? "") !== "" && (
          <p role="status" className="text-warning text-callout break-words">
            {turn.error}
          </p>
        )}
        {mine &&
          turn.status === "unknown" &&
          run?.run.session_id != null &&
          run.run.session_id !== "" && (
            <Button
              variant="ghost"
              onClick={() => ctx.onSession(run.run.session_id!)}
            >
              {zh ? "核对处理会话" : "Check handling session"}
            </Button>
          )}
        {refs(turn)}
      </div>
    );
  }
  const renderAfter = (turn: Turn) =>
    turn.requestId == null ? null : afterByTurn.get(turn.requestId);
  const turnViews = useMemo(() => conversationTurns(turns), [turns]);

  // Live records not already placed under a message.
  const tail = {
    questions: (state.questions ?? []).filter(
      (q) => OPEN_QUESTION.has(q.state) && !shown.has(`q:${q.id}`)
    ),
    changes: (state.changes ?? []).filter(
      (c) => c.proposed && c.state === "proposed"
    ),
    memory: proposals.filter(
      (p) =>
        !shown.has(`m:${p.id}`) &&
        (p.state === "proposed" || p.state === "failed")
    ),
    deliveries: state.goals.filter(
      (g) =>
        g.assignments.at(-1)?.result?.state === "submitted" &&
        !shown.has(`g:${g.id}`)
    ),
    notes: (state.notifications ?? []).filter((n) => !n.read),
    // Failed reviews that belong to no goal and no message would otherwise vanish.
    reviews: (state.scoped_runs ?? []).filter(
      (r) =>
        (r.error ?? "") !== "" &&
        !goalById.has(r.scope) &&
        !turns.some(
          (t) => t.source_run_id === r.run.id || r.scope === `turn:${t.id}`
        )
    ),
  };
  const tailCount = Object.values(tail).reduce((n, v) => n + v.length, 0);
  const notified = new Set<string>();
  const latestNotes = tail.notes
    .toReversed()
    .filter((n) => {
      const key = n.goal_id || n.id;
      if (notified.has(key)) return false;
      notified.add(key);
      return true;
    })
    .toReversed();
  const latestIds = new Set(latestNotes.map((n) => n.id));
  const earlierNotes = tail.notes.filter((n) => !latestIds.has(n.id));

  const paths = hints.toSorted();
  const canSend = managed.length > 0 && !sending && !ctx.busy;
  const send = async () => {
    if (!canSend || sendingRef.current) return;
    sendingRef.current = true;
    setSending(true);
    setNotSent(false);
    setUnsupported(false);
    let saved = false;
    try {
      const content = await getMarkdownRef.current?.();
      if (content == null) {
        setUnsupported(true);
        return;
      }
      if (!content) {
        focusRef.current?.();
        return;
      }
      const effectiveActor =
        selectedScope === "global" ? "chief" : `project:${selectedScope}`;
      const effectivePaths =
        selectedScope === "global"
          ? paths
          : (selectedProject?.bindings
              .filter((b) => b.active)
              .map((b) => b.path) ?? paths);

      const key = JSON.stringify([content, effectivePaths, effectiveActor]);
      if (attempt.current?.key !== key)
        attempt.current = { id: newTurnId(), key };
      saved = await ctx.edit({
        kind: "say",
        turn_id: attempt.current.id,
        content,
        ...(effectivePaths.length ? { project_paths: effectivePaths } : {}),
        actor: effectiveActor,
      });
      if (saved) {
        attempt.current = null;
        // The editor exports the current draft again so text or formatting added during send stays.
        const now = await getMarkdownRef.current?.();
        if (now === content) clearRef.current?.();
        setHints((cur) =>
          JSON.stringify(cur.toSorted()) === JSON.stringify(paths) ? [] : cur
        );
      } else setNotSent(true);
    } catch {
      // A failed export/send keeps the editor and id; a post-save export failure keeps the new draft.
      setNotSent(!saved);
    } finally {
      sendingRef.current = false;
      setSending(false);
    }
  };

  const emptyState = (
    <p className="text-prose text-muted-foreground py-6">
      {managed.length === 0
        ? zh
          ? "先在设置里选择幕僚可以跟进的项目。"
          : "Choose the projects the chief of staff may follow in Settings first."
        : (state.summary ?? "") ||
          (zh
            ? "直接说想问的、想推进的事。可以一次交代多个项目。"
            : "Ask, discuss, or hand over work in plain words. One message can cover several projects.")}
    </p>
  );

  const tailView =
    tailCount > 0 || (state.attention ?? "") !== "" ? (
      <div className="space-y-3 pb-4">
        {state.attention != null && state.attention !== "" && (
          <p role="status" className="text-warning text-prose">
            {state.attention}
          </p>
        )}
        {tailCount > 0 && (
          <section className="space-y-3" aria-label={zh ? "当前" : "Now"}>
            {tail.questions.map((q) => (
              <QuestionCard key={q.id} question={q} ctx={ctx} />
            ))}
            {tail.changes.map((c) => (
              <ProposalCard key={c.id} change={c} ctx={ctx} />
            ))}
            {tail.memory.map((p) => (
              <MemoryProposalCard
                key={p.id}
                proposal={p}
                global={GLOBAL_MEMORY}
                ctx={ctx}
              />
            ))}
            {tail.deliveries.map((g) => (
              <DeliveryCard key={g.id} goal={g} ctx={ctx} />
            ))}
            {tail.reviews.map((r) => (
              <div key={r.run.id} className="text-warning space-y-2">
                <p className="text-prose break-words">{r.error}</p>
                {ctx.developer === true &&
                  r.run.session_id != null &&
                  r.run.session_id !== "" && (
                    <Button
                      variant="ghost"
                      onClick={() => ctx.onSession(r.run.session_id!)}
                    >
                      {zh ? "打开复查会话" : "Open review"}
                    </Button>
                  )}
              </div>
            ))}
            {latestNotes.map((n) => (
              <NotificationCard key={n.id} notification={n} ctx={ctx} />
            ))}
            {earlierNotes.length > 0 && (
              <>
                <Button
                  variant="ghost"
                  aria-expanded={earlierOpen}
                  onClick={() => setEarlierOpen(!earlierOpen)}
                >
                  {zh
                    ? `更早的进展（${earlierNotes.length}）`
                    : `Earlier updates (${earlierNotes.length})`}
                </Button>
                {earlierOpen &&
                  earlierNotes.map((n) => (
                    <NotificationCard key={n.id} notification={n} ctx={ctx} />
                  ))}
              </>
            )}
          </section>
        )}
      </div>
    ) : null;

  const sendLabel = zh ? "发送" : "Send";
  const pausedLabel = zh
    ? "跟进已暂停，消息会先记录。"
    : "Follow-up is paused; messages are recorded first.";
  const scopeLabel = hints.length
    ? zh
      ? `限定 ${hints.length} 个项目`
      : `${hints.length} selected projects`
    : zh
      ? "项目范围（可选）"
      : "Project scope (optional)";

  return (
    <div
      hidden={!active}
      className="flex min-h-0 flex-1 flex-col"
      aria-label={zh ? "与幕僚的对话" : "Conversation with chief of staff"}
      role="region"
    >
      <TranscriptPane
        variant="main"
        turns={turnViews}
        quiet
        active={active}
        label={zh ? "幕僚对话记录" : "Chief of staff transcript"}
        sessionId="chief-of-staff"
        renderAfter={renderAfter}
        after={tailView}
        empty={emptyState}
        before={
          managed.length === 0 ? (
            <Button variant="secondary" className="mb-4" onClick={onConfigure}>
              {zh ? "打开设置" : "Open settings"}
            </Button>
          ) : null
        }
      />
      <div
        className="order-2 shrink-0 px-6 pt-2 pb-6"
        onKeyDownCapture={(e) => {
          // Mod+Enter says the message here; it must never reach the app's global "run".
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            e.stopPropagation();
            void send();
          }
        }}
      >
        <ComposerCard
          controls={
            <>
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-8 gap-1.5 px-2 text-xs font-medium"
                      aria-label={
                        zh
                          ? `当前范围：${currentScopeName}（切换全局 / 业务项目）`
                          : `Current scope: ${currentScopeName} (switch Global / Business Project)`
                      }
                      data-testid="scope-switcher"
                    >
                      {selectedScope === "global" ? (
                        <Globe className="size-4 shrink-0" aria-hidden />
                      ) : (
                        <Folder className="size-4 shrink-0" aria-hidden />
                      )}
                      <span className="max-w-[120px] truncate">
                        {currentScopeName}
                      </span>
                      <ChevronDown
                        className="text-muted-foreground size-3 shrink-0"
                        aria-hidden
                      />
                    </Button>
                  }
                />
                <DropdownMenuContent align="start" className="w-56">
                  <DropdownMenuItem
                    onClick={() => {
                      setSelectedScope("global");
                      setHints([]);
                    }}
                    className="gap-2"
                    data-testid="scope-option-global"
                  >
                    <Globe className="size-4 shrink-0" aria-hidden />
                    <div className="flex min-w-0 flex-1 flex-col">
                      <span className="text-xs font-medium">
                        {zh ? "全局" : "Global"}
                      </span>
                      <span className="text-muted-foreground truncate text-[10px]">
                        {zh
                          ? "统筹全部项目与全局偏好"
                          : "Coordinates all projects & global preferences"}
                      </span>
                    </div>
                  </DropdownMenuItem>
                  {activeProjects.length > 0 && (
                    <>
                      <div className="bg-border my-1 h-px" role="separator" />
                      {activeProjects.map((p) => (
                        <DropdownMenuItem
                          key={p.id}
                          onClick={() => {
                            setSelectedScope(p.id);
                            const bPaths = p.bindings
                              .filter((b) => b.active)
                              .map((b) => b.path);
                            setHints(bPaths);
                          }}
                          className="gap-2"
                          data-testid={`scope-option-${p.id}`}
                        >
                          <Folder className="size-4 shrink-0" aria-hidden />
                          <div className="flex min-w-0 flex-1 flex-col">
                            <span className="truncate text-xs font-medium">
                              {p.name}
                            </span>
                            <span className="text-muted-foreground truncate text-[10px]">
                              {p.bindings
                                .filter((b) => b.active)
                                .map((b) => b.path)
                                .join(", ") ||
                                (zh ? "无关联工作区" : "No workspace")}
                            </span>
                          </div>
                        </DropdownMenuItem>
                      ))}
                    </>
                  )}
                </DropdownMenuContent>
              </DropdownMenu>

              <TooltipButton
                label={
                  zh
                    ? "按需观察（指令、记忆与持续任务）"
                    : "Observe scope (instructions, memory, tasks)"
                }
                variant={observeOpen ? "secondary" : "ghost"}
                size="icon"
                className="size-8 rounded-full"
                aria-expanded={observeOpen}
                onClick={() => setObserveOpen(!observeOpen)}
                data-testid="observe-scope-button"
              >
                <Eye aria-hidden />
              </TooltipButton>

              {managed.length > 0 && selectedScope === "global" && (
                <TooltipButton
                  label={scopeLabel}
                  variant={hints.length ? "secondary" : "ghost"}
                  size="icon"
                  className="size-8 rounded-full"
                  aria-expanded={scopeOpen}
                  onClick={() => setScopeOpen(!scopeOpen)}
                >
                  <FolderTree aria-hidden />
                </TooltipButton>
              )}
              {managed.length === 0 && (
                <TooltipButton
                  label={zh ? "打开设置" : "Open settings"}
                  variant="ghost"
                  size="icon"
                  className="size-8 rounded-full"
                  onClick={onConfigure}
                >
                  <Settings aria-hidden />
                </TooltipButton>
              )}
              <span className="text-callout text-muted-foreground flex min-w-0 flex-1 items-center px-1">
                {notSent && (
                  <span role="status" className="text-warning">
                    {zh
                      ? "未能发送，草稿已保留；重试使用同一条消息。"
                      : "Not sent. Your draft is kept; retrying resends the same message."}
                  </span>
                )}
                {unsupported && (
                  <span role="status" className="text-warning">
                    {zh
                      ? "这里只能发送文字，请移除卡片或附件。"
                      : "Only text can be sent here. Remove cards or attachments."}
                  </span>
                )}
                {!notSent &&
                  !unsupported &&
                  state.settings?.enabled !== true && (
                    <TooltipButton
                      label={pausedLabel}
                      variant="ghost"
                      size="icon"
                      aria-disabled
                      className="size-8 cursor-default rounded-full"
                    >
                      <Pause aria-hidden />
                    </TooltipButton>
                  )}
              </span>
              <ComposerSendButton
                empty={empty}
                disabled={!canSend}
                label={sendLabel}
                tooltip={sendLabel}
                hint="⌘↵"
                onClick={() => void send()}
              />
            </>
          }
        >
          {managed.length > 0 && scopeOpen && (
            <div
              className="flex flex-wrap items-center gap-2 px-4 pb-2"
              role="group"
              aria-label={zh ? "范围提示（可选）" : "Scope hint (optional)"}
            >
              {managed.map((p) => (
                <Button
                  key={p.path}
                  type="button"
                  size="compact"
                  variant={hints.includes(p.path) ? "secondary" : "ghost"}
                  aria-pressed={hints.includes(p.path)}
                  onClick={() =>
                    setHints((now) =>
                      now.includes(p.path)
                        ? now.filter((v) => v !== p.path)
                        : [...now, p.path]
                    )
                  }
                >
                  {p.name}
                </Button>
              ))}
            </div>
          )}
          {observeOpen && (
            <div
              className="rounded-module border-border bg-muted/40 mx-4 mb-2 space-y-3 border p-3 text-xs"
              role="region"
              aria-label={zh ? "范围观察详情" : "Scope observation details"}
              data-testid="observe-scope-panel"
            >
              <div className="border-border/60 flex items-center justify-between border-b pb-2">
                <span className="flex items-center gap-1.5 font-semibold">
                  {selectedScope === "global" ? (
                    <Globe className="size-3.5" aria-hidden />
                  ) : (
                    <Folder className="size-3.5" aria-hidden />
                  )}
                  {zh ? "当前范围：" : "Scope: "}
                  {currentScopeName}
                </span>
                <span className="text-muted-foreground text-[10px]">
                  {selectedScope === "global"
                    ? GLOBAL_MEMORY
                    : `codetwo://managed-project/${selectedScope}`}
                </span>
              </div>

              {/* Instructions Section */}
              <div className="space-y-1">
                <span className="text-foreground/80 font-medium">
                  {zh ? "作用域指令：" : "Scope Instructions:"}
                </span>
                {(() => {
                  const insts = (state.hierarchy?.instructions ?? []).filter(
                    (i) =>
                      selectedScope === "global"
                        ? i.scope.kind === "global"
                        : i.scope.kind === "project" &&
                          i.scope.project_id === selectedScope
                  );
                  const active = insts.at(-1);
                  if (!active) {
                    return (
                      <p className="text-muted-foreground italic">
                        {zh ? "暂无已确认指令" : "No active instructions"}
                      </p>
                    );
                  }
                  return (
                    <div className="rounded-control bg-background/80 border-border/40 border p-2 font-mono text-[11px] leading-relaxed break-words">
                      <div className="text-muted-foreground mb-1 flex items-center justify-between text-[10px]">
                        <span>rev {active.revision}</span>
                        <span>hash: {active.hash.slice(0, 10)}...</span>
                        <span>reach: {active.reach}</span>
                      </div>
                      <p className="text-foreground font-sans text-xs whitespace-pre-wrap">
                        {active.text}
                      </p>
                    </div>
                  );
                })()}
              </div>

              {/* Memory Shares Section */}
              <div className="space-y-1">
                <span className="text-foreground/80 font-medium">
                  {zh ? "共享记忆：" : "Memory Shares:"}
                </span>
                {(() => {
                  const shares = (state.hierarchy?.shares ?? []).filter(
                    (s) =>
                      s.state === "approved" &&
                      (selectedScope === "global"
                        ? s.target.kind === "global"
                        : s.target.kind === "project" &&
                          s.target.project_id === selectedScope)
                  );
                  if (shares.length === 0) {
                    return (
                      <p className="text-muted-foreground italic">
                        {zh ? "暂无共享记忆" : "No shared memory"}
                      </p>
                    );
                  }
                  return (
                    <div className="space-y-1">
                      {shares.map((s) => (
                        <div
                          key={s.id}
                          className="rounded-control bg-background/80 border-border/40 flex items-center justify-between border px-2 py-1 text-[11px]"
                        >
                          <span className="truncate">{s.source_scope}</span>
                          <span className="text-muted-foreground font-mono text-[10px]">
                            {s.content_hash.slice(0, 8)}...
                          </span>
                        </div>
                      ))}
                    </div>
                  );
                })()}
              </div>

              {/* Tasks Section */}
              <div className="space-y-1">
                <span className="text-foreground/80 font-medium">
                  {zh ? "关联持续任务：" : "Scope Tasks:"}
                </span>
                {(() => {
                  const goals = state.goals.filter((g) => {
                    if (selectedScope === "global") return true;
                    const owner = state.hierarchy?.goal_owners[g.id];
                    return owner?.project_id === selectedScope;
                  });
                  if (goals.length === 0) {
                    return (
                      <p className="text-muted-foreground italic">
                        {zh ? "无关联任务" : "No associated tasks"}
                      </p>
                    );
                  }
                  return (
                    <div className="space-y-1">
                      {goals.map((g) => (
                        <div
                          key={g.id}
                          className="rounded-control bg-background/80 border-border/40 flex items-center justify-between border px-2 py-1 text-[11px]"
                        >
                          <span className="truncate font-medium">
                            {g.title}
                          </span>
                          <span className="text-muted-foreground text-[10px]">
                            {g.status}
                          </span>
                        </div>
                      ))}
                    </div>
                  );
                })()}
              </div>
            </div>
          )}
          <DocEditor
            textOnly
            sessionId={null}
            placeholder={
              zh
                ? "问进展、聊取舍，或交办 A 和 B 的事…"
                : "Ask for progress, discuss trade-offs, or hand over work for A and B…"
            }
            inputLabel={zh ? "给幕僚的消息" : "Message to chief of staff"}
            getBlocksRef={getBlocksRef}
            getMarkdownRef={getMarkdownRef}
            focusRef={focusRef}
            clearRef={clearRef}
            onEmptyChange={setEmpty}
          />
        </ComposerCard>
      </div>
    </div>
  );
}

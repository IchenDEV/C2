import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Check,
  ChevronRight,
  ExternalLink,
  Eye,
  Square,
  UserRound,
  X,
} from "@/components/ui/icons";
import { TooltipButton } from "@/components/ui/tooltip";

import { MarkdownContent } from "../session/MarkdownContent";
import type {
  AssistantNotification,
  AssistantState,
  CoordinationQuestion,
  Goal,
  MemoryProposal,
  RequirementChange,
} from "./api";
import {
  Proposal,
  Question,
  questionReadOnly,
  status,
} from "./CoordinationPanel";

export type Edit = (action: unknown, target?: Goal) => Promise<boolean>;
export interface CardContext {
  state: AssistantState;
  busy: boolean;
  edit: Edit;
  zh: boolean;
  projectName: (path: string) => string;
  onGoal: (id: string) => void;
  onSession: (id: string) => void;
  /** Routine execution/source-session shortcuts show only for the developer inspector. */
  developer?: boolean;
}

const card = "bg-fill-quiet rounded-control space-y-3 p-4";

/** Every state here is read from the original record; nothing is stored in the card. */
function goalStateLine(g: Goal, zh: boolean) {
  const a = g.assignments.at(-1);
  const parts = [`${zh ? "要求" : "Requirements"} v${g.contract_revision}`];
  if (a) {
    parts.push(
      (a.confirmed_revision ?? 0) > 0
        ? `${zh ? "执行者已确认" : "Worker confirmed"} v${a.confirmed_revision}`
        : zh
          ? "执行者待确认"
          : "Worker confirmation pending"
    );
    if (a.stop_requested)
      parts.push(zh ? "等待停止回执" : "Waiting for stop receipt");
    if (a.taken_over) parts.push(zh ? "已由你接管" : "Taken over by you");
    if (a.result?.state === "submitted")
      parts.push(
        zh
          ? `已提交 v${a.result.contract_revision}，待你验收`
          : `Submitted v${a.result.contract_revision}; awaiting your review`
      );
  }
  if (g.status === "completed") parts.push(zh ? "已验收" : "Accepted");
  return parts.join(" · ");
}

export function GoalCard({ goal: g, ctx }: { goal: Goal; ctx: CardContext }) {
  const { zh, busy, edit } = ctx;
  const a = g.assignments.at(-1);
  const live = g.status !== "completed" && g.status !== "cancelled";
  const controllable = a?.owned === true && !a.taken_over;
  return (
    <article className={card} aria-label={g.title}>
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="text-callout text-muted-foreground">
            {ctx.projectName(g.project_path)}
          </p>
          <h3 className="text-body font-medium break-words">{g.title}</h3>
        </div>
        <Badge variant="outline">
          {
            {
              active: zh ? "推进中" : "Active",
              paused: zh ? "已暂停" : "Paused",
              needs_attention: zh ? "待决定" : "Needs a decision",
              completed: zh ? "已验收" : "Accepted",
              cancelled: zh ? "已取消" : "Cancelled",
            }[g.status]
          }
        </Badge>
      </div>
      <p className="text-callout text-muted-foreground">
        {goalStateLine(g, zh)}
      </p>
      {(g.blocker || g.next_step) && (
        <p className="text-prose break-words">{g.blocker || g.next_step}</p>
      )}
      <div className="flex flex-wrap gap-2">
        <TooltipButton
          label={zh ? "详情" : "Details"}
          size="icon"
          variant="secondary"
          onClick={() => ctx.onGoal(g.id)}
        >
          <Eye aria-hidden />
        </TooltipButton>
        {ctx.developer === true &&
          a?.session_id != null &&
          a.session_id !== "" && (
            <TooltipButton
              label={zh ? "打开执行会话" : "Open execution"}
              size="icon"
              variant="ghost"
              onClick={() => ctx.onSession(a.session_id!)}
            >
              <ExternalLink aria-hidden />
            </TooltipButton>
          )}
        {live && controllable && (
          <TooltipButton
            label={zh ? "请求停止执行" : "Request stop"}
            size="icon"
            variant="ghost"
            disabled={busy || a.stop_requested}
            onClick={() =>
              void edit({ kind: "control", goal_id: g.id, operation: "stop" })
            }
          >
            <Square aria-hidden />
          </TooltipButton>
        )}
        {live && g.assignments.length > 0 && a?.taken_over !== true && (
          <TooltipButton
            label={zh ? "接管" : "Take over"}
            size="icon"
            variant="ghost"
            disabled={busy}
            onClick={() => void edit({ kind: "takeover", id: g.id })}
          >
            <UserRound aria-hidden />
          </TooltipButton>
        )}
      </div>
    </article>
  );
}

/** A deliverable is bound to the requirement version it was produced for. */
export function DeliveryCard({
  goal: g,
  ctx,
}: {
  goal: Goal;
  ctx: CardContext;
}) {
  const result = g.assignments.at(-1)?.result;
  if (result?.state !== "submitted") return null;
  const { zh } = ctx;
  return (
    <article
      className={card}
      aria-label={zh ? `交付：${g.title}` : `Deliverable: ${g.title}`}
    >
      <h3 className="text-body font-medium">
        {zh ? "已提交，待你验收" : "Submitted; awaiting your review"} · v
        {result.contract_revision}
      </h3>
      <p className="text-callout text-muted-foreground">
        {g.title}
        {result.contract_revision !== g.contract_revision &&
          (zh
            ? ` · 当前要求已是 v${g.contract_revision}，此交付基于旧版本`
            : ` · requirements are now v${g.contract_revision}; this delivery targets an older version`)}
      </p>
      <MarkdownContent text={result.evidence} />
      {result.artifacts.map((f) => (
        <p key={f.path} className="text-caption break-all">
          {f.path} · SHA-256 {f.sha256}
        </p>
      ))}
      <TooltipButton
        label={zh ? "查看产物并验收" : "Review deliverable"}
        size="icon"
        variant="secondary"
        onClick={() => ctx.onGoal(g.id)}
      >
        <Eye aria-hidden />
      </TooltipButton>
    </article>
  );
}

export function QuestionCard({
  question: q,
  ctx,
}: {
  question: CoordinationQuestion;
  ctx: CardContext;
}) {
  const { zh, state } = ctx;
  const goal = state.goals.find((g) => g.id === q.goal_id);
  const session = goal?.assignments.find(
    (a) => a.id === q.assignment_id
  )?.session_id;
  const openOriginal =
    session != null && session !== "" ? (
      <Button variant="secondary" onClick={() => ctx.onSession(session)}>
        {zh ? "打开原会话" : "Open original session"}
      </Button>
    ) : null;
  if (q.state === "delivery_unknown" || q.state === "expired")
    return (
      <article className={card} aria-label={q.title}>
        <h3 className="text-body font-medium">{q.title}</h3>
        <p className="text-callout text-muted-foreground">
          {q.state === "delivery_unknown"
            ? zh
              ? "答复结果未知。请核对原执行会话；系统不会自动重发，任务验收保持阻塞。"
              : "Answer outcome unknown. Inspect the original execution; no automatic resend or acceptance."
            : zh
              ? "原问题已失效。需重新发起澄清或打开执行会话，旧答案不能回放。"
              : "The original input expired. Reopen clarification or inspect execution; old answers cannot be replayed."}
        </p>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="secondary"
            disabled={ctx.busy || q.state !== "expired"}
            onClick={() =>
              void ctx.edit({ kind: "retry_question", question_id: q.id })
            }
          >
            {zh ? "重新发起澄清" : "Reopen clarification"}
          </Button>
          {openOriginal}
          {q.goal_id && (
            <Button variant="ghost" onClick={() => ctx.onGoal(q.goal_id)}>
              {zh ? "查看目标" : "Open goal"}
            </Button>
          )}
        </div>
      </article>
    );
  if (q.source_input?.kind === "permission")
    return (
      <article className={card} aria-label={q.title}>
        <h3 className="text-body font-medium">{q.title}</h3>
        <p className="text-prose whitespace-pre-wrap">{q.context}</p>
        <p className="text-callout text-muted-foreground">
          {zh
            ? "这是具体的权限请求。请在原会话的审批入口处理，幕僚不会代批。"
            : "This is a specific permission request. Decide it in the original session; the chief of staff never approves it for you."}
        </p>
        {openOriginal}
      </article>
    );
  return (
    <div className="space-y-2">
      <Question
        question={q}
        busy={ctx.busy}
        edit={ctx.edit}
        zh={zh}
        readOnly={questionReadOnly(state, q)}
      />
      {q.goal_id && (
        <Button variant="ghost" onClick={() => ctx.onGoal(q.goal_id)}>
          {zh ? "查看目标" : "Open goal"}
        </Button>
      )}
    </div>
  );
}

export function ProposalCard({
  change: c,
  ctx,
}: {
  change: RequirementChange;
  ctx: CardContext;
}) {
  return (
    <div className="space-y-2">
      <Proposal change={c} busy={ctx.busy} edit={ctx.edit} zh={ctx.zh} />
      <TooltipButton
        label={ctx.zh ? "查看目标" : "Open goal"}
        size="icon"
        variant="ghost"
        onClick={() => ctx.onGoal(c.goal_id)}
      >
        <ChevronRight aria-hidden />
      </TooltipButton>
    </div>
  );
}

export function NotificationCard({
  notification: n,
  ctx,
}: {
  notification: AssistantNotification;
  ctx: CardContext;
}) {
  const { zh } = ctx;
  return (
    <article className={card} aria-label={n.title}>
      <h3 className="text-body font-medium">{n.title}</h3>
      <p className="text-prose whitespace-pre-wrap">{n.body}</p>
      <div className="flex flex-wrap gap-2">
        {n.goal_id && (
          <TooltipButton
            label={zh ? "查看目标" : "Open goal"}
            size="icon"
            variant="secondary"
            onClick={() => ctx.onGoal(n.goal_id)}
          >
            <ChevronRight aria-hidden />
          </TooltipButton>
        )}
        {ctx.developer === true &&
          n.session_id != null &&
          n.session_id !== "" && (
            <Button
              variant="ghost"
              onClick={() => ctx.onSession(n.session_id!)}
            >
              {zh ? "打开执行会话" : "Open execution"}
            </Button>
          )}
        <TooltipButton
          label={zh ? "标为已读" : "Mark read"}
          size="icon"
          variant="ghost"
          disabled={ctx.busy}
          onClick={() =>
            void ctx.edit({ kind: "acknowledge", notification_id: n.id })
          }
        >
          <Check aria-hidden />
        </TooltipButton>
      </div>
    </article>
  );
}

export function MemoryProposalCard({
  proposal: p,
  global,
  ctx,
}: {
  proposal: MemoryProposal;
  global: string;
  ctx: CardContext;
}) {
  const { zh } = ctx;
  const scope =
    p.project_path === global
      ? zh
        ? "全局记忆"
        : "Shared memory"
      : ctx.projectName(p.project_path);
  const decide = (kind: "confirm_memory" | "reject_memory") =>
    void ctx.edit({ kind, proposal_id: p.id, content_hash: p.content_hash });
  return (
    <article
      className={card}
      aria-label={zh ? "记忆建议" : "Memory suggestion"}
    >
      <h3 className="text-body font-medium">
        {zh ? "要记住这条吗？" : "Remember this?"}
      </h3>
      <p className="text-callout text-muted-foreground">
        {scope} · {p.category}
        {ctx.developer === true ? ` · #${p.content_hash.slice(0, 8)}` : ""}
      </p>
      <p className="text-prose whitespace-pre-wrap">{p.content}</p>
      {p.state === "proposed" ? (
        <div className="flex flex-wrap gap-2">
          <TooltipButton
            label={zh ? "记住" : "Remember"}
            size="icon"
            disabled={ctx.busy}
            onClick={() => decide("confirm_memory")}
          >
            <Check aria-hidden />
          </TooltipButton>
          <TooltipButton
            label={zh ? "不用记" : "Dismiss"}
            size="icon"
            variant="secondary"
            disabled={ctx.busy}
            onClick={() => decide("reject_memory")}
          >
            <X aria-hidden />
          </TooltipButton>
        </div>
      ) : (
        <p className="text-callout text-muted-foreground">
          {p.state === "confirmed"
            ? p.memory_id == null
              ? zh
                ? "已确认，等待写入"
                : "Confirmed; waiting to save"
              : zh
                ? "已写入记忆"
                : "Saved to memory"
            : p.state === "rejected"
              ? zh
                ? "已忽略"
                : "Dismissed"
              : zh
                ? "未能写入记忆"
                : "Could not save"}
          {p.error != null && p.error !== "" ? ` · ${p.error}` : ""}
        </p>
      )}
    </article>
  );
}

export const intakeLabel = (s: string, zh: boolean) =>
  ({
    recorded: zh
      ? "已记录，等待幕僚读取"
      : "Recorded; waiting for the chief of staff",
    reviewing: zh ? "幕僚正在处理" : "Chief of staff is reviewing",
    handled: zh ? "幕僚已处理" : "Handled",
    failed: zh ? "处理失败" : "Could not be handled",
    unknown: zh
      ? "结果未知，不会自动重发"
      : "Outcome unknown; it will not be resent automatically",
  })[s] ?? status(s, zh);

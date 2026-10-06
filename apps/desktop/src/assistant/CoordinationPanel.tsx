import { useState } from "react";
import type { ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";

import { answerContent, canSubmit } from "../session/elicitation";
import type {
  ElicitationValue,
  ElicitationValues,
} from "../session/elicitation";
import { sameGoalTarget } from "./api";
import type {
  AssistantState,
  CoordinationQuestion,
  Goal,
  RequirementChange,
} from "./api";
import { AssistantSelect } from "./AssistantSelect";

type Edit = (action: unknown, target?: Goal) => Promise<boolean>;

export function DraftRebase({
  baseline,
  current,
  review = false,
  adopt,
  zh,
}: {
  baseline: Goal | null | undefined;
  current: Goal | undefined;
  review?: boolean;
  adopt: () => void;
  zh: boolean;
}) {
  if (!baseline || !current || sameGoalTarget(baseline, current, review))
    return null;
  return (
    <div className="text-warning text-callout space-y-2">
      <p>
        {zh
          ? "这份草稿依据的要求或交付已变化。请核对上方最新内容。"
          : "The requirements or deliverable behind this draft changed. Review the latest content above."}
      </p>
      <Button type="button" variant="secondary" onClick={adopt}>
        {zh
          ? "已核对，按最新内容继续编辑"
          : "Reviewed; continue with latest version"}
      </Button>
    </div>
  );
}
const field = "text-callout flex min-w-0 flex-col gap-2";
const labels: Record<string, [string, string]> = {
  recorded: ["Recorded", "已记录"],
  queued: ["Waiting for delivery", "等待投递"],
  accepted: ["Engine accepted", "引擎已接收"],
  received: ["Progress received", "已收到进展"],
  delivery_unknown: [
    "Answer outcome unknown; inspect execution",
    "答复结果未知，请核对执行会话",
  ],
  unknown: ["Outcome unknown; inspect execution", "结果未知，请核对执行会话"],
  failed: ["Delivery failed", "投递失败"],
  cancelled: ["Cancelled", "已取消"],
  confirmed: ["Worker confirmed", "执行者已确认"],
  awaiting_confirmation: ["Awaiting confirmation", "等待执行者确认"],
  proposed: ["Decision needed", "待决定"],
  applied: ["Applied", "已应用"],
  rejected: ["Rejected", "已拒绝"],
  superseded: ["Superseded", "已被新版本替代"],
};
function stringValue(value: ElicitationValue | undefined) {
  return typeof value === "string" ? value : "";
}
function selectedValues(value: ElicitationValue | undefined) {
  return Array.isArray(value) ? value : [];
}
function inputValue(value: ElicitationValue | undefined) {
  return typeof value === "string" || typeof value === "number" ? value : "";
}
export function status(value: string, zh: boolean) {
  return labels[value]?.[zh ? 1 : 0] ?? value;
}

export function Question({
  question: q,
  busy,
  edit,
  zh,
  readOnly,
}: {
  question: CoordinationQuestion;
  readOnly: boolean;
  busy: boolean;
  edit: Edit;
  zh: boolean;
}) {
  const [answer, setAnswer] = useState("");
  const [values, setValues] = useState<ElicitationValues>({});
  const form = q.source_input?.form;
  const permission = q.source_input?.kind === "permission";
  const ready = form ? canSubmit(form, values) : !!answer.trim();
  const setField = (key: string, value: ElicitationValue | undefined) =>
    setValues((previous) => {
      const next = { ...previous };
      if (value === undefined) delete next[key];
      else next[key] = value;
      return next;
    });
  return (
    <article
      className="bg-fill-quiet rounded-control space-y-3 p-4"
      aria-label={q.title}
    >
      <h3 className="text-body font-medium">{q.title}</h3>
      <p className="text-prose whitespace-pre-wrap">{q.context}</p>
      <p className="text-callout text-muted-foreground">
        {q.blocking
          ? zh
            ? "等待回答，相关工作暂停"
            : "Blocking related work"
          : zh
            ? "补充问题"
            : "Non-blocking question"}
      </p>
      {q.source_input?.context && (
        <p className="text-callout text-muted-foreground">
          {Object.values(q.source_input.context)
            .filter((v) => typeof v === "string" && v.length > 0)
            .join(" · ")}
        </p>
      )}
      {readOnly ? (
        <p className="text-callout text-muted-foreground">
          {zh
            ? "此执行已不在当前控制范围。请打开原执行会话处理。"
            : "This execution is outside the current control scope. Open its original session to respond."}
        </p>
      ) : (
        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            void edit({
              kind: "answer",
              question_id: q.id,
              answer: form
                ? JSON.stringify(answerContent(form, values))
                : answer,
            });
          }}
        >
          {form ? (
            form.fields.map((f) => (
              <div key={f.key} className={field}>
                {f.title ?? f.key}
                {f.description != null && f.description !== "" && (
                  <span className="text-muted-foreground">{f.description}</span>
                )}
                {f.kind === "boolean" ? (
                  <AssistantSelect
                    label={f.title ?? f.key}
                    value={
                      values[f.key] === undefined ? "" : String(values[f.key])
                    }
                    options={[
                      ["", zh ? "请选择" : "Choose"],
                      ["true", zh ? "是" : "Yes"],
                      ["false", zh ? "否" : "No"],
                    ]}
                    onValueChange={(v) =>
                      setField(f.key, v === "" ? undefined : v === "true")
                    }
                  />
                ) : f.kind === "select" || f.kind === "multi_select" ? (
                  f.kind === "multi_select" ? (
                    <span className="grid gap-2">
                      {f.options?.map((o) => (
                        <label
                          key={o.value}
                          className="text-body flex items-center gap-2"
                        >
                          <input
                            type="checkbox"
                            checked={selectedValues(values[f.key]).includes(
                              o.value
                            )}
                            onChange={(e) => {
                              const previous = selectedValues(values[f.key]);
                              setField(
                                f.key,
                                e.target.checked
                                  ? [...previous, o.value]
                                  : previous.filter((v) => v !== o.value)
                              );
                            }}
                          />
                          {o.label}
                        </label>
                      ))}
                    </span>
                  ) : (
                    <AssistantSelect
                      label={f.title ?? f.key}
                      value={stringValue(values[f.key])}
                      options={[
                        ["", zh ? "请选择" : "Choose"],
                        ...(f.options?.map(
                          (o) => [o.value, o.label] as [string, string]
                        ) ?? []),
                      ]}
                      onValueChange={(v) => setField(f.key, v)}
                    />
                  )
                ) : (
                  <Input
                    aria-label={f.title ?? f.key}
                    type={
                      f.kind === "integer" || f.kind === "number"
                        ? "number"
                        : "text"
                    }
                    step={f.kind === "integer" ? 1 : undefined}
                    value={inputValue(values[f.key])}
                    onChange={(e) =>
                      setField(
                        f.key,
                        e.target.value === ""
                          ? undefined
                          : f.kind === "integer" || f.kind === "number"
                            ? Number(e.target.value)
                            : e.target.value
                      )
                    }
                  />
                )}
              </div>
            ))
          ) : (
            <>
              {(permission
                ? q.source_input!.options
                : q.options.map((o) => [o, o])
              ).map(([value, label]) => (
                <label key={value} className="text-body flex items-start gap-2">
                  <input
                    type="radio"
                    name={`answer-${q.id}`}
                    value={value}
                    checked={answer === value}
                    onChange={() => setAnswer(value)}
                  />
                  {label}
                </label>
              ))}
              {permission ? (
                <label className="text-body flex items-center gap-2">
                  <input
                    type="radio"
                    name={`answer-${q.id}`}
                    checked={answer === "cancel"}
                    onChange={() => setAnswer("cancel")}
                  />
                  {zh ? "拒绝并取消此请求" : "Decline this request"}
                </label>
              ) : (
                <label className={field}>
                  {zh ? "回答或补充说明" : "Answer or details"}
                  <Textarea
                    maxLength={8000}
                    value={answer}
                    onChange={(e) => setAnswer(e.target.value)}
                  />
                </label>
              )}
            </>
          )}
          <div>
            <Button type="submit" disabled={busy || !ready}>
              {zh ? "提交回答" : "Send answer"}
            </Button>
          </div>
        </form>
      )}
    </article>
  );
}

export function Proposal({
  change: c,
  busy,
  edit,
  zh,
}: {
  change: RequirementChange;
  busy: boolean;
  edit: Edit;
  zh: boolean;
}) {
  const [remember, setRemember] = useState(false);
  return (
    <article className="bg-fill-quiet rounded-control space-y-3 p-4">
      <h3 className="text-body font-medium">
        {zh ? "方向变更建议" : "Proposed change"} · v{c.from_revision} → v
        {c.to_revision}
      </h3>
      <p className="text-callout text-muted-foreground">
        {c.before_title} · {c.before_acceptance}
      </p>
      <p className="text-prose">{c.title}</p>
      <p className="text-prose">{c.acceptance}</p>
      <p className="text-body">{c.reason}</p>
      <label className="text-body flex items-center gap-2">
        <input
          type="checkbox"
          checked={remember}
          onChange={(e) => setRemember(e.target.checked)}
        />
        {zh
          ? "执行者确认后，记为项目决定"
          : "Remember as a project decision after worker confirmation"}
      </label>
      <div className="flex flex-wrap gap-2">
        <Button
          disabled={busy}
          onClick={() =>
            void edit({ kind: "apply_change", change_id: c.id, remember })
          }
        >
          {zh ? "采用变更" : "Apply change"}
        </Button>
        <Button
          variant="secondary"
          disabled={busy}
          onClick={() => void edit({ kind: "reject_change", change_id: c.id })}
        >
          {zh ? "保留原要求" : "Keep requirements"}
        </Button>
      </div>
    </article>
  );
}

/** A question can be answered here only while its goal still owns that exact assignment. */
export function questionReadOnly(
  state: AssistantState,
  q: CoordinationQuestion
) {
  return (
    !!q.goal_id &&
    !state.goals.some((g) => {
      const a = g.assignments.at(-1);
      return (
        g.id === q.goal_id &&
        g.status === "active" &&
        g.contract_revision === q.contract_revision &&
        state.settings?.projects.includes(g.project_path) === true &&
        a?.id === q.assignment_id &&
        a.owned &&
        !a.taken_over
      );
    })
  );
}

export function GoalCoordination({
  goal: g,
  state,
  busy,
  edit,
  zh,
  onSession,
  management,
}: {
  goal: Goal;
  onSession: (id: string) => void;
  management: ReactNode;
  state: AssistantState;
  busy: boolean;
  edit: Edit;
  zh: boolean;
}) {
  const [section, setSection] = useState("progress");
  const [history, setHistory] = useState(false);
  const [deliveryOptions, setDeliveryOptions] = useState(false);
  const [content, setContent] = useState("");
  const [queue, setQueue] = useState(false);
  const [evidence, setEvidence] = useState("");
  const [messageBaseline, setMessageBaseline] = useState<Goal | null>(null);
  const [reviewBaseline, setReviewBaseline] = useState<Goal | null>(null);
  const [dependencyBaseline, setDependencyBaseline] = useState<Goal | null>(
    null
  );
  const [dependencies, setDependencies] = useState(g.dependencies ?? []);
  const a = g.assignments.at(-1);
  const controllable = a?.owned === true && !a.taken_over;
  const messages = (state.messages ?? []).filter((m) => m.goal_id === g.id);
  const changes = (state.changes ?? []).filter(
    (c) => c.goal_id === g.id && !c.proposed
  );
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <Tabs value={section} onValueChange={(v) => setSection(String(v))}>
          <TabsList aria-label={zh ? "事项详情" : "Goal sections"}>
            <TabsTrigger
              value="progress"
              id="assistant-progress-tab"
              aria-controls="assistant-progress-panel"
            >
              {zh ? "进展与交付" : "Progress"}
            </TabsTrigger>
            <TabsTrigger
              value="updates"
              id="assistant-updates-tab"
              aria-controls="assistant-updates-panel"
            >
              {zh ? "沟通" : "Updates"}
            </TabsTrigger>
            <TabsTrigger
              value="manage"
              id="assistant-manage-tab"
              aria-controls="assistant-manage-panel"
            >
              {zh ? "管理" : "Manage"}
            </TabsTrigger>
          </TabsList>
        </Tabs>
        <div className="flex flex-wrap gap-2">
          {a?.session_id != null && a.session_id !== "" && (
            <Button
              variant="secondary"
              onClick={() => onSession(a.session_id!)}
            >
              {zh ? "打开执行会话" : "Open execution"}
            </Button>
          )}
          {controllable &&
            g.status !== "completed" &&
            g.status !== "cancelled" && (
              <Button
                variant="ghost"
                disabled={busy || a.stop_requested}
                onClick={() =>
                  void edit({
                    kind: "control",
                    goal_id: g.id,
                    operation: "stop",
                  })
                }
              >
                {zh ? "请求停止执行" : "Request stop"}
              </Button>
            )}
        </div>
      </div>
      <p className="text-callout text-muted-foreground">
        {zh ? "要求版本" : "Requirements"} v{g.contract_revision ?? 1} ·{" "}
        {zh ? "执行者确认" : "Worker confirmed"}{" "}
        {(a?.confirmed_revision ?? 0) > 0
          ? `v${a?.confirmed_revision}`
          : zh
            ? "待确认"
            : "Pending"}
        {a?.stop_requested === true &&
          (zh ? " · 等待停止回执" : " · Waiting for stop receipt")}
      </p>
      <Separator />
      <div
        role="tabpanel"
        id="assistant-updates-panel"
        aria-labelledby="assistant-updates-tab"
        hidden={section !== "updates"}
        className="space-y-4"
      >
        {controllable &&
          g.status !== "completed" &&
          g.status !== "cancelled" && (
            <form
              className="grid gap-3"
              onSubmit={(e) => {
                e.preventDefault();
                void edit(
                  {
                    kind: "message",
                    goal_id: g.id,
                    content,
                    queue,
                  },
                  messageBaseline ?? g
                ).then((saved) => {
                  if (saved) {
                    setContent("");
                    setMessageBaseline(null);
                  }
                });
              }}
            >
              <DraftRebase
                baseline={messageBaseline}
                current={g}
                adopt={() => setMessageBaseline(g)}
                zh={zh}
              />
              <label className={field}>
                {zh ? "补充或询问执行者" : "Message worker"}
                <Textarea
                  required
                  maxLength={16_000}
                  value={content}
                  onChange={(e) => {
                    setMessageBaseline((old) => old ?? g);
                    setContent(e.target.value);
                  }}
                />
              </label>
              <Button
                variant="ghost"
                className="justify-self-start"
                aria-expanded={deliveryOptions}
                aria-controls="assistant-delivery-options"
                onClick={() => setDeliveryOptions(!deliveryOptions)}
              >
                {zh ? "送达方式" : "Delivery options"}
              </Button>
              <div id="assistant-delivery-options" hidden={!deliveryOptions}>
                <label className="text-callout flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={queue}
                    onChange={(e) => setQueue(e.target.checked)}
                  />
                  {zh ? "等本轮结束后再送达" : "Deliver after this turn"}
                </label>{" "}
              </div>

              <p className="text-callout text-muted-foreground">
                {zh
                  ? "支持即时补充时会直接送达，否则排队。修改目标会创建新版本，并先等待旧执行停止。"
                  : "Uses live steering when supported; otherwise queues. Editing requirements creates a version and waits for the old execution to stop."}
                {state.settings?.enabled !== true &&
                  (zh
                    ? " 跟进已暂停，消息会等待启用。"
                    : " Follow-up is paused; delivery awaits enablement.")}
              </p>
              <div>
                <Button type="submit" disabled={busy || a.stop_requested}>
                  {zh ? "发送补充" : "Send message"}
                </Button>
              </div>
            </form>
          )}
        {!!(messages.length || changes.length) && (
          <section>
            <h3 className="text-section-title font-medium">
              {zh ? "沟通与变更记录" : "Communication and changes"}
            </h3>
            <div className="space-y-3 py-3">
              {changes.map((c) => (
                <article
                  key={c.id}
                  className="bg-fill-quiet rounded-control space-y-2 p-3"
                >
                  <p className="text-body">
                    v{c.from_revision} → v{c.to_revision} ·{" "}
                    {status(c.state, zh)}
                  </p>
                  <p className="text-prose">{c.reason}</p>
                  <p className="text-prose">{c.acceptance}</p>
                  {!c.remember && (
                    <Button
                      variant="ghost"
                      disabled={busy}
                      onClick={() =>
                        void edit({ kind: "remember_change", change_id: c.id })
                      }
                    >
                      {zh
                        ? "确认后记为项目决定"
                        : "Remember after confirmation"}
                    </Button>
                  )}
                  {c.memory_id != null && c.memory_id !== "" && (
                    <p className="text-caption text-muted-foreground">
                      {zh
                        ? "已记为项目决定"
                        : "Remembered as a project decision"}
                    </p>
                  )}
                </article>
              ))}
              {messages.toReversed().map((m) => (
                <article
                  key={m.id}
                  className="bg-fill-quiet rounded-control space-y-2 p-3"
                >
                  <p className="text-caption text-muted-foreground">
                    {m.author} · v{m.contract_revision} ·{" "}
                    {m.mode === "steer"
                      ? zh
                        ? "即时补充"
                        : "Live steering"
                      : m.mode === "queue"
                        ? zh
                          ? "轮后送达"
                          : "After this turn"
                        : ""}{" "}
                    ·{" "}
                    {m.confirmed
                      ? status("confirmed", zh)
                      : status(m.state, zh)}
                  </p>
                  <p className="text-prose whitespace-pre-wrap">{m.content}</p>
                  {m.outcome && (
                    <p className="text-callout text-muted-foreground">
                      {m.outcome}
                    </p>
                  )}
                </article>
              ))}
            </div>
          </section>
        )}{" "}
      </div>
      <div
        role="tabpanel"
        id="assistant-progress-panel"
        aria-labelledby="assistant-progress-tab"
        hidden={section !== "progress"}
        className="space-y-5"
      >
        <section className="space-y-2">
          <h3 className="text-section-title font-medium">
            {zh ? "当前进展" : "Current progress"}
          </h3>
          <p className="text-prose">
            {g.next_step ||
              (zh ? "等待幕僚安排下一步。" : "Waiting for the next step.")}
          </p>
          {g.blocker && <p className="text-warning text-prose">{g.blocker}</p>}
          {state.scoped_runs
            ?.filter(
              (r) =>
                r.scope === g.id &&
                (r.state === "running" || (r.error != null && r.error !== ""))
            )
            .map((r) => (
              <div key={r.run.id} className="space-y-2">
                <p className="text-callout break-words">
                  {(r.error ?? "") || (zh ? "幕僚复查中" : "Reviewing")}
                </p>
                {r.run.session_id != null && r.run.session_id !== "" && (
                  <Button
                    variant="ghost"
                    onClick={() => onSession(r.run.session_id!)}
                  >
                    {zh ? "打开复查会话" : "Open review"}
                  </Button>
                )}
              </div>
            ))}
        </section>
        <section className="space-y-2">
          <h3 className="text-section-title font-medium">
            {zh ? "完成条件" : "Acceptance criteria"}
          </h3>
          <p className="text-prose whitespace-pre-wrap">{g.acceptance}</p>
        </section>
        {g.verdict && (
          <section className="space-y-2">
            <h3 className="text-section-title font-medium">
              {zh ? "验收证据" : "Acceptance evidence"}
            </h3>
            <p className="text-prose whitespace-pre-wrap">
              {g.verdict.evidence}
            </p>
            {g.verdict.artifacts.map((f) => (
              <p key={f.path} className="text-caption break-all">
                {f.path} · SHA-256 {f.sha256}
              </p>
            ))}
          </section>
        )}
        {a?.result && (
          <div className="bg-fill-quiet rounded-control grid gap-3 p-4">
            <h4 className="text-body font-medium">
              {zh ? "交付与验收" : "Deliverable review"} · v
              {a.result.contract_revision}
            </h4>
            <p className="text-prose whitespace-pre-wrap">
              {a.result.evidence}
            </p>
            {a.result.artifacts.map((file) => (
              <p key={file.path} className="text-caption break-all">
                {file.path} · SHA-256 {file.sha256}
              </p>
            ))}
            {a.result.state === "submitted" && (
              <>
                <DraftRebase
                  baseline={reviewBaseline}
                  current={g}
                  review
                  adopt={() => setReviewBaseline(g)}
                  zh={zh}
                />
                <label className={field}>
                  {zh
                    ? "验收或返工说明"
                    : "Review evidence or rework instructions"}
                  <Textarea
                    maxLength={8000}
                    value={evidence}
                    onChange={(e) => {
                      setReviewBaseline((old) => old ?? g);
                      setEvidence(e.target.value);
                    }}
                  />
                </label>
                <div className="flex flex-wrap gap-2">
                  {[
                    ["accepted", zh ? "验收通过" : "Accept"],
                    ["rework", zh ? "要求返工" : "Request rework"],
                    ["unverifiable", zh ? "无法验证" : "Cannot verify"],
                  ].map(([verdict, label]) => (
                    <Button
                      key={verdict}
                      variant="secondary"
                      disabled={busy || !evidence.trim()}
                      onClick={() =>
                        void edit(
                          {
                            kind: "review",
                            goal_id: g.id,
                            verdict,
                            evidence,
                          },
                          reviewBaseline ?? g
                        ).then((saved) => {
                          if (saved) {
                            setEvidence("");
                            setReviewBaseline(null);
                          }
                        })
                      }
                    >
                      {label}
                    </Button>
                  ))}
                </div>
              </>
            )}
          </div>
        )}
        {(a?.prior_results?.length ?? 0) > 0 && (
          <section>
            <Button
              variant="ghost"
              aria-expanded={history}
              aria-controls="assistant-delivery-history"
              onClick={() => setHistory(!history)}
            >
              {zh ? "历史交付" : "Previous deliverables"}
            </Button>
            <div
              id="assistant-delivery-history"
              hidden={!history}
              className="space-y-3 py-3"
            >
              {a?.prior_results.map((r) => (
                <article
                  key={r.id}
                  className="bg-fill-quiet rounded-control space-y-2 p-3"
                >
                  <p className="text-body">
                    v{r.contract_revision} · {r.state}
                  </p>
                  <p className="text-prose whitespace-pre-wrap">{r.evidence}</p>
                  {r.artifacts.map((file) => (
                    <p key={file.path} className="text-caption break-all">
                      {file.path} · SHA-256 {file.sha256}
                    </p>
                  ))}
                </article>
              ))}
            </div>
          </section>
        )}
      </div>
      <div
        role="tabpanel"
        id="assistant-manage-panel"
        aria-labelledby="assistant-manage-tab"
        hidden={section !== "manage"}
        className="space-y-6"
      >
        <section className="space-y-3">
          <h3 className="text-section-title font-medium">
            {zh ? "执行与控制" : "Execution and control"}
          </h3>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="secondary"
              disabled={
                busy ||
                g.status === "cancelled" ||
                g.status === "completed" ||
                a?.stop_requested === true
              }
              onClick={() =>
                void edit({
                  kind: "status",
                  id: g.id,
                  status: g.status === "active" ? "paused" : "active",
                })
              }
            >
              {g.status === "active"
                ? zh
                  ? "暂停目标跟进"
                  : "Pause follow-up"
                : zh
                  ? "恢复目标跟进"
                  : "Resume follow-up"}
            </Button>
            {g.assignments.length > 0 && (
              <Button
                variant="ghost"
                disabled={busy}
                onClick={() => void edit({ kind: "takeover", id: g.id })}
              >
                {zh ? "接管" : "Take over"}
              </Button>
            )}
          </div>
          {controllable &&
            g.status !== "completed" &&
            g.status !== "cancelled" && (
              <>
                {" "}
                <div className="flex flex-wrap gap-2">
                  {g.status !== "active" && (
                    <Button
                      variant="secondary"
                      disabled={busy || a.stop_requested}
                      onClick={() =>
                        void edit({
                          kind: "control",
                          goal_id: g.id,
                          operation: "resume",
                        })
                      }
                    >
                      {zh ? "继续执行" : "Resume execution"}
                    </Button>
                  )}

                  <Button
                    variant="ghost"
                    disabled={busy || a.stop_requested}
                    onClick={() =>
                      void edit({
                        kind: "control",
                        goal_id: g.id,
                        operation: "cancel",
                      })
                    }
                  >
                    {zh ? "取消目标" : "Cancel goal"}
                  </Button>
                </div>
                <p className="text-callout text-muted-foreground">
                  {zh
                    ? "暂停跟进只暂停幕僚安排。停止执行会请求执行者结束当前工作。"
                    : "Pausing follow-up pauses coordination. Request stop to end the current execution."}
                </p>
              </>
            )}
        </section>
        <section className="space-y-3">
          <h3 className="text-section-title font-medium">
            {zh ? "修改目标" : "Edit goal"}
          </h3>
          {management}
        </section>
        <section>
          <h3 className="text-section-title font-medium">
            {zh ? "输入依赖" : "Input dependencies"}
          </h3>
          <div className="grid gap-2 py-3">
            <DraftRebase
              baseline={dependencyBaseline}
              current={g}
              adopt={() => setDependencyBaseline(g)}
              zh={zh}
            />
            {state.goals
              .filter((other) => other.id !== g.id)
              .map((other) => (
                <label
                  key={other.id}
                  className="text-body flex items-start gap-2"
                >
                  <input
                    type="checkbox"
                    checked={dependencies.includes(other.id)}
                    onChange={(e) => {
                      setDependencyBaseline((old) => old ?? g);
                      setDependencies(
                        e.target.checked
                          ? [...dependencies, other.id]
                          : dependencies.filter((id) => id !== other.id)
                      );
                    }}
                  />
                  {other.title}
                </label>
              ))}
            <div>
              <Button
                variant="secondary"
                disabled={busy}
                onClick={() =>
                  void edit(
                    { kind: "dependencies", goal_id: g.id, dependencies },
                    dependencyBaseline ?? g
                  ).then((saved) => {
                    if (saved) setDependencyBaseline(null);
                  })
                }
              >
                {zh ? "保存依赖" : "Save dependencies"}
              </Button>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}

import { useCallback, useEffect, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { AppIcon } from "@/components/ui/icons";
import {
  Activity,
  AlertCircle,
  AlertTriangle,
  Check,
  CircleAlert,
  Clock3 as Clock,
  Info,
  Pencil,
  Plus,
  Power,
  RotateCcw,
} from "@/components/ui/icons";
import { Input } from "@/components/ui/input";
import {
  Tooltip,
  TooltipButton,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

import {
  DEFAULT_GLOBAL_HISTORY,
  DEFAULT_SOURCE_HISTORY,
  principalKey,
} from "./api";
import type {
  AssistantApi,
  AssistantState,
  ObservationSnapshot,
  SourceBinding,
  SourceHealth,
  TargetReceipt,
} from "./api";
import { AssistantSelect } from "./AssistantSelect";

const fieldClass = "text-callout flex min-w-0 flex-col gap-2";
const RECEIPT_LIMIT = 20;
const MAX_STREAMS = 8;
const MAX_FILTER = 64;

type Icon = AppIcon;
// state -> [icon, label, label (zh), tooltip, tooltip (zh)]. Every state stays distinct.
const RECEIPT: Record<string, [Icon, string, string, string, string]> = {
  recorded: [
    Clock,
    "Queued",
    "已接收",
    "Recorded; waiting for review.",
    "已记录，等待复查。",
  ],
  reviewing: [
    Activity,
    "Reviewing",
    "复查中",
    "Its pinned run is reviewing this input.",
    "固定的复查正在处理这条输入。",
  ],
  deferred_paused: [
    Clock,
    "Deferred: paused",
    "延后：已暂停",
    "Chief of staff is paused; resumes with it.",
    "幕僚已暂停，恢复后继续。",
  ],
  deferred_attention: [
    Clock,
    "Deferred: attention",
    "延后：待关注",
    "Waiting on a global attention item.",
    "等待全局待关注项。",
  ],
  deferred_budget: [
    Clock,
    "Deferred: budget",
    "延后：额度",
    "Waiting for review allowance.",
    "等待复查额度。",
  ],
  deferred_goal_state: [
    Info,
    "Deferred: goal state",
    "延后：事项状态",
    "Goal is paused, finished or not committed; you were only notified. Nothing is reopened.",
    "事项已暂停、完成或未提交，只通知你，不会重开。",
  ],
  needs_configuration: [
    AlertTriangle,
    "Needs setup",
    "需配置",
    "Configure scope or provider first.",
    "请先配置范围或模型。",
  ],
  needs_attention: [
    AlertTriangle,
    "Needs attention",
    "待你处理",
    "Inside the binding, but it is not tied to a goal yet; it needs your association or review.",
    "在绑定内，但尚未关联到事项，需要你关联或复查。",
  ],
  invalidated: [
    AlertCircle,
    "Invalidated",
    "已失效",
    "The binding, goal or control changed; the decision was discarded.",
    "绑定、事项或控制已变化，旧决定作废。",
  ],
  unknown: [
    CircleAlert,
    "Unknown outcome",
    "结果未知",
    "Outcome unconfirmed. It will not be replayed or counted as handled.",
    "结果未确认，不会自动重放，也不算已处理。",
  ],
  handled: [
    Check,
    "Handled",
    "已处理",
    "Review finished and its outputs were committed.",
    "复查已完成，输出已提交。",
  ],
  rejected: [
    Info,
    "Rejected",
    "已拒绝",
    "Terminal; kept for dedupe.",
    "终态，保留用于去重。",
  ],
  filtered: [
    Info,
    "Filtered",
    "已过滤",
    "Outside the binding filters; no content kept.",
    "在绑定过滤之外，未保留正文。",
  ],
  superseded: [
    Info,
    "Superseded",
    "已被取代",
    "A newer version replaced it.",
    "已有更新版本取代。",
  ],
  revoked: [Info, "Revoked", "已撤销", "Binding was revoked.", "绑定已撤销。"],
};
const HEALTH: Record<string, [string, string]> = {
  ok: ["Receiving", "接收中"],
  needs_capacity: ["Needs capacity", "需提高容量"],
  needs_reset: ["Needs reset", "需重置游标"],
};

const healthChip = (
  h: SourceHealth
): [Icon, string, string, string, string] => {
  const [en, cn] = HEALTH[h.state] ?? [h.state, h.state];
  return [
    h.state === "ok" ? Check : AlertTriangle,
    en,
    cn,
    h.detail ?? en,
    h.detail ?? cn,
  ];
};

const list = (text: string) => [
  ...new Set(
    text
      .split(",")
      .map((v) => v.trim())
      .filter(Boolean)
  ),
];
const openStatus = new Set(["active", "paused", "needs_attention"]);

interface Draft {
  /** The persisted binding this draft was opened from; null when creating. */
  base: SourceBinding | null;
  connector: string;
  account: string;
  resources: string;
  actors: string;
  projects: string[];
  goals: string[];
  streams: string;
  enabled: boolean;
}
const emptyDraft: Draft = {
  base: null,
  connector: "",
  account: "",
  resources: "",
  actors: "",
  projects: [],
  goals: [],
  streams: "",
  enabled: true,
};
const toDraft = (b: SourceBinding): Draft => ({
  base: b,
  connector: principalKey(b.principal),
  account: b.account_scope,
  resources: b.resource_filter.join(", "),
  actors: b.actor_filter.join(", "),
  projects: b.project_paths,
  goals: b.goal_ids,
  streams: b.streams.join(", "),
  enabled: b.enabled,
});
interface CapDraft {
  id: string;
  version: number;
  source: string;
  global: string;
}

const MAX_CHECKPOINT = 4096;
const MAX_REASON = 256;
const bytes = (text: string) => new TextEncoder().encode(text).length;
// eslint-disable-next-line no-control-regex
const hasControl = (text: string) => /[\u0000-\u001F\u007F-\u009F]/.test(text);
/** Exact rendering: a real null and an empty string must never look alike. */
const exact = (
  v: string | null | undefined,
  tr: (en: string, cn: string) => string
) =>
  v === undefined
    ? tr("not reported", "未报告")
    : v === null
      ? tr("null (no checkpoint)", "null（无检查点）")
      : JSON.stringify(v);

interface ResetDraft {
  id: string;
  /** Binding version the user is approving against. */
  version: number;
  stream: string;
  /** Stream checkpoint the user reviewed; undefined until a stream is chosen or reported. */
  before: string | null | undefined;
  nullBaseline: boolean;
  baseline: string;
  reason: string;
}

const inRange = (text: string, min: number, max: number) => {
  const n = Number(text);
  return Number.isInteger(n) && n >= min && n <= max;
};

export function ObservationInspector({
  state,
  api,
  edit,
  busy,
  zh,
  projectName,
}: {
  state: AssistantState;
  api: Pick<AssistantApi, "observations">;
  edit: (action: unknown) => Promise<boolean>;
  busy: boolean;
  zh: boolean;
  projectName: (path: string) => string;
}) {
  const tr = (en: string, cn: string) => (zh ? cn : en);
  const [obs, setObs] = useState<ObservationSnapshot | null>(null);
  const [loadError, setLoadError] = useState("");
  const [unavailable, setUnavailable] = useState(false);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [cap, setCap] = useState<CapDraft | null>(null);
  const [reset, setReset] = useState<ResetDraft | null>(null);

  const load = useCallback(async () => {
    if (!api.observations) {
      setUnavailable(true);
      return;
    }
    try {
      setObs(await api.observations(50));
      setLoadError("");
    } catch (e) {
      setLoadError(String(e));
    }
  }, [api]);
  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(), 5000);
    return () => clearInterval(timer);
  }, [load]);

  const sources = state.sources ?? [];
  const connectors = obs?.connectors ?? [];
  const health = (key: string) => obs?.health.find((h) => h.key === key);
  /** undefined = this Core did not report it; null = reported, no checkpoint. */
  const checkpointOf = (id: string, stream: string) =>
    obs?.streams?.find((v) => v.source_id === id && v.stream_id === stream)
      ?.checkpoint;
  const receipts = obs?.receipts ?? [];
  const goalTitle = (id: string) =>
    state.goals.find((g) => g.id === id)?.title ?? id;
  const sourceLabel = (id: string) => {
    const s = sources.find((v) => v.source_id === id);
    return s ? `${s.principal.connector_id} · ${s.account_scope}` : id;
  };

  const submit = async (action: unknown) => {
    if (await edit(action)) {
      setDraft(null);
      setCap(null);
      setReset(null);
      await load();
    }
  };

  const chip = (entry: [Icon, string, string, string, string]) => {
    const [I, en, cn, tipEn, tipCn] = entry;
    return (
      <Tooltip>
        <TooltipTrigger render={<span />}>
          <Badge variant="outline">
            <I aria-hidden />
            {tr(en, cn)}
          </Badge>
        </TooltipTrigger>
        <TooltipContent>{tr(tipEn, tipCn)}</TooltipContent>
      </Tooltip>
    );
  };

  // ---- binding form
  const form = () => {
    if (!draft) return null;
    const base = draft.base;
    const current = base
      ? sources.find((s) => s.source_id === base.source_id)
      : undefined;
    const stale = base != null && current?.version !== base.version;
    const catalog = connectors.find((c) => principalKey(c) === draft.connector);
    const principal = base?.principal ?? catalog;
    const provider = base?.provider ?? catalog?.provider ?? "";
    const account = base ? base.account_scope : draft.account.trim();
    const streams = list(draft.streams);
    const resources = list(draft.resources);
    const actors = list(draft.actors);
    const scope = state.settings?.projects ?? [];
    const projects = [...new Set([...scope, ...draft.projects])];
    const goalOptions = state.goals.filter(
      (g) =>
        draft.projects.includes(g.project_path) &&
        (openStatus.has(g.status) || draft.goals.includes(g.id))
    );
    const goals = draft.goals.filter((id) =>
      goalOptions.some((g) => g.id === id)
    );
    const duplicate =
      !base &&
      principal != null &&
      sources.some(
        (s) =>
          principalKey(s.principal) === principalKey(principal) &&
          s.provider === provider &&
          s.account_scope === account
      );
    const valid =
      principal != null &&
      account !== "" &&
      account.length <= 256 &&
      draft.projects.length > 0 &&
      draft.projects.every((p) => scope.includes(p)) &&
      streams.length > 0 &&
      streams.length <= MAX_STREAMS &&
      streams.every((s) => s.length <= 256) &&
      resources.length <= MAX_FILTER &&
      actors.length <= MAX_FILTER &&
      !duplicate;
    const set = (patch: Partial<Draft>) =>
      setDraft((d) => d && { ...d, ...patch });
    const toggle = (key: "projects" | "goals", value: string, on: boolean) => {
      const next = on
        ? [...draft[key], value]
        : draft[key].filter((v) => v !== value);
      if (key === "projects") {
        set({
          projects: next,
          goals: draft.goals.filter((id) =>
            state.goals.some(
              (g) => g.id === id && next.includes(g.project_path)
            )
          ),
        });
      } else set({ goals: next });
    };
    return (
      <form
        className="bg-fill-quiet rounded-control grid max-w-2xl gap-3 p-4"
        aria-label={
          base
            ? tr("Edit event source", "编辑事件来源")
            : tr("Add event source", "添加事件来源")
        }
        onSubmit={(e) => {
          e.preventDefault();
          if (!valid || stale || principal == null) return;
          void submit({
            kind: "source",
            binding: {
              source_id: base ? base.source_id : null,
              expected_version: base ? base.version : 0,
              principal: {
                plugin: principal.plugin,
                realm: principal.realm,
                connector_id: principal.connector_id,
              },
              provider,
              account_scope: account,
              resource_filter: resources,
              actor_filter: actors,
              project_paths: draft.projects,
              goal_ids: goals,
              streams,
              enabled: draft.enabled,
            },
          });
        }}
      >
        {stale && (
          <div role="alert" className="text-callout grid gap-2">
            <p>
              {tr(
                `This source changed (v${base.version} → ${current ? `v${current.version}` : "missing"}). Your draft is kept; Save is off until you review the latest version.`,
                `这个来源已变化（v${base.version} → ${current ? `v${current.version}` : "不存在"}）。草稿已保留；核对最新版本前不能保存。`
              )}
            </p>
            {current && (
              <Button
                variant="secondary"
                className="justify-self-start"
                onClick={() => set({ base: current })}
              >
                {tr(
                  "Keep my draft on the latest version",
                  "在最新版本上保留我的草稿"
                )}
              </Button>
            )}
          </div>
        )}
        {base ? (
          <p className="text-callout">
            {tr(
              "Identity (cannot change; bind a new source instead)",
              "身份（不可修改；需要更换请新建来源）"
            )}
            {": "}
            <code>{principalKey(base.principal)}</code> · {base.provider} ·{" "}
            {base.account_scope}
          </p>
        ) : (
          <>
            <label className={fieldClass}>
              {tr("Connector", "连接器")}
              <AssistantSelect
                label={tr("Connector", "连接器")}
                value={draft.connector}
                options={[
                  ["", tr("Choose", "请选择")],
                  ...connectors.map(
                    (c) =>
                      [
                        principalKey(c),
                        `${c.label} · ${c.connector_id} (${c.plugin})`,
                      ] as [string, string]
                  ),
                ]}
                onValueChange={(v) => set({ connector: v })}
              />
            </label>
            <label className={fieldClass}>
              {tr(
                "Account (reported by the adapter; you pin it, Core cannot verify it)",
                "账户（适配器自报，由你固定，Core 无法验证）"
              )}
              <Input
                required
                maxLength={256}
                value={draft.account}
                onChange={(e) => set({ account: e.target.value })}
              />
            </label>
          </>
        )}
        <label className={fieldClass}>
          {tr(
            "Streams (comma separated, up to 8)",
            "流（逗号分隔，最多 8 个）"
          )}
          <Input
            required
            value={draft.streams}
            onChange={(e) => set({ streams: e.target.value })}
          />
        </label>
        <label className={fieldClass}>
          {tr(
            "Only these resource IDs (exact, empty = all)",
            "仅限这些资源 ID（精确匹配，留空=全部）"
          )}
          <Input
            value={draft.resources}
            onChange={(e) => set({ resources: e.target.value })}
          />
        </label>
        <label className={fieldClass}>
          {tr(
            "Only these sender IDs (exact, empty = all)",
            "仅限这些发送者 ID（精确匹配，留空=全部）"
          )}
          <Input
            value={draft.actors}
            onChange={(e) => set({ actors: e.target.value })}
          />
        </label>
        <fieldset className="grid gap-2">
          <legend className="text-callout mb-1">
            {tr("Projects", "项目")}
          </legend>
          {projects.map((p) => (
            <label key={p} className="text-callout flex items-center gap-2">
              <input
                type="checkbox"
                checked={draft.projects.includes(p)}
                onChange={(e) => toggle("projects", p, e.target.checked)}
              />
              {projectName(p)}
              {!scope.includes(p) &&
                tr(" (outside chief-of-staff scope)", "（已不在幕僚范围内）")}
            </label>
          ))}
        </fieldset>
        <fieldset className="grid gap-2">
          <legend className="text-callout mb-1">{tr("Goals", "事项")}</legend>
          {goalOptions.map((g) => (
            <label key={g.id} className="text-callout flex items-center gap-2">
              <input
                type="checkbox"
                checked={draft.goals.includes(g.id)}
                onChange={(e) => toggle("goals", g.id, e.target.checked)}
              />
              {g.title}
            </label>
          ))}
          {goals.length === 0 && (
            <p className="text-callout text-muted-foreground">
              {tr(
                "No goal selected: events are kept and need your attention. They are not routed to every goal.",
                "未选事项：事件会保留并等你处理，不会自动分给所有事项。"
              )}
            </p>
          )}
        </fieldset>
        <label className="text-callout flex items-center gap-2">
          <input
            type="checkbox"
            checked={draft.enabled}
            onChange={(e) => set({ enabled: e.target.checked })}
          />
          {tr("Receive events from this source", "接收此来源的事件")}
        </label>
        {duplicate && (
          <p role="alert" className="text-destructive text-callout">
            {tr(
              "This account is already bound; edit that source instead.",
              "该账户已绑定，请编辑现有来源。"
            )}
          </p>
        )}
        {principal && (
          <p role="note" className="text-callout text-muted-foreground">
            {tr(
              `Observe only. Connector ${principalKey(principal)}, account “${account}” (adapter-reported, pinned by you) may send events about ${draft.projects.map(projectName).join(", ") || "no project"}; goals: ${goals.length ? goals.map(goalTitle).join(", ") : "none (events wait for your attention)"}. Events can trigger review, questions and proposals. They cannot command, dispatch work, change controls, accept results, write memory or send messages.`,
              `仅观察。连接器 ${principalKey(principal)}、账户“${account}”（适配器自报，由你固定）可发送关于 ${draft.projects.map(projectName).join("、") || "无项目"} 的事件；事项：${goals.length ? goals.map(goalTitle).join("、") : "无（事件等待你处理）"}。事件只会触发复查、提问和提案，不能下达命令、派工、改控制、验收、写记忆或外发。`
            )}
          </p>
        )}
        <div className="flex gap-2">
          <Button type="submit" disabled={busy || stale || !valid}>
            {base
              ? tr("Save source", "保存来源")
              : tr("Add source", "添加来源")}
          </Button>
          <Button variant="ghost" onClick={() => setDraft(null)}>
            {tr("Cancel", "取消")}
          </Button>
        </div>
      </form>
    );
  };

  // ---- capacity form
  const capacityForm = (b: SourceBinding) => {
    if (cap?.id !== b.source_id) return null;
    const stale = cap.version !== b.version;
    const sourceOk =
      cap.source === "" || inRange(cap.source, DEFAULT_SOURCE_HISTORY, 20_000);
    const globalOk =
      cap.global === "" || inRange(cap.global, DEFAULT_GLOBAL_HISTORY, 100_000);
    const valid =
      sourceOk && globalOk && (cap.source !== "" || cap.global !== "");
    return (
      <form
        className="bg-fill-quiet rounded-control grid gap-3 p-4"
        aria-label={tr("Raise history limit", "提高历史上限")}
        onSubmit={(e) => {
          e.preventDefault();
          if (!valid || stale) return;
          void submit({
            kind: "source_capacity",
            source_id: b.source_id,
            binding_version: cap.version,
            ...(cap.source !== "" && {
              source_history_limit: Number(cap.source),
            }),
            ...(cap.global !== "" && {
              global_history_limit: Number(cap.global),
            }),
          });
        }}
      >
        {stale && (
          <div role="alert" className="text-callout grid gap-2">
            <p>
              {tr(
                `This source changed (v${cap.version} → v${b.version}). Your numbers are kept; review the latest health, then continue.`,
                `这个来源已变化（v${cap.version} → v${b.version}）。数值已保留；请核对最新状态后继续。`
              )}
            </p>
            <Button
              variant="secondary"
              className="justify-self-start"
              onClick={() => setCap({ ...cap, version: b.version })}
            >
              {tr(
                "Keep my numbers on the latest version",
                "在最新版本上保留我的数值"
              )}
            </Button>
          </div>
        )}
        <label className={fieldClass}>
          {tr(
            `This source's history limit (now ${b.history_limit ?? DEFAULT_SOURCE_HISTORY}; ${DEFAULT_SOURCE_HISTORY}–20000)`,
            `该来源历史上限（当前 ${b.history_limit ?? DEFAULT_SOURCE_HISTORY}；${DEFAULT_SOURCE_HISTORY}–20000）`
          )}
          <Input
            type="number"
            value={cap.source}
            onChange={(e) => {
              const value = e.target.value;
              setCap((c) => c && { ...c, source: value });
            }}
          />
        </label>
        <label className={fieldClass}>
          {tr(
            `Global history limit (now ${state.source_history_global ?? DEFAULT_GLOBAL_HISTORY}; ${DEFAULT_GLOBAL_HISTORY}–100000; empty keeps it)`,
            `全局历史上限（当前 ${state.source_history_global ?? DEFAULT_GLOBAL_HISTORY}；${DEFAULT_GLOBAL_HISTORY}–100000；留空不变）`
          )}
          <Input
            type="number"
            value={cap.global}
            onChange={(e) => {
              const value = e.target.value;
              setCap((c) => c && { ...c, global: value });
            }}
          />
        </label>
        <p className="text-callout text-muted-foreground">
          {tr(
            "Raising a limit keeps more dedupe keys on disk. Nothing is deleted or replayed.",
            "提高上限会在磁盘上保留更多去重键；不会删除或回放任何内容。"
          )}
        </p>
        <div className="flex gap-2">
          <Button type="submit" disabled={busy || stale || !valid}>
            {tr("Raise limit", "提高上限")}
          </Button>
          <Button variant="ghost" onClick={() => setCap(null)}>
            {tr("Cancel", "取消")}
          </Button>
        </div>
      </form>
    );
  };

  // ---- cursor reset form (observation-only: Core records an approval, nothing else happens)
  const resetForm = (b: SourceBinding) => {
    if (reset?.id !== b.source_id) return null;
    const live =
      reset.stream === "" ? undefined : checkpointOf(b.source_id, reset.stream);
    const authorityStale =
      reset.version !== b.version ||
      !b.enabled ||
      (reset.stream !== "" && !b.streams.includes(reset.stream));
    const cursorStale =
      reset.stream !== "" &&
      reset.before !== undefined &&
      live !== reset.before;
    const reason = reset.reason.trim();
    const after = reset.nullBaseline ? null : reset.baseline;
    const baselineOk = after === null || bytes(after) <= MAX_CHECKPOINT;
    const reasonOk =
      reason !== "" && bytes(reason) <= MAX_REASON && !hasControl(reason);
    const valid =
      reset.stream !== "" &&
      reset.before !== undefined &&
      baselineOk &&
      reasonOk &&
      !authorityStale &&
      !cursorStale;
    const patch = (next: Partial<ResetDraft>) =>
      setReset((r) => r && { ...r, ...next });
    return (
      <form
        className="bg-fill-quiet rounded-control grid gap-3 p-4"
        aria-label={tr("Reset stream cursor", "重置流游标")}
        onSubmit={(e) => {
          e.preventDefault();
          if (!valid || busy || reset.before === undefined) return;
          void submit({
            kind: "source_reset",
            source_id: b.source_id,
            binding_version: reset.version,
            stream_id: reset.stream,
            checkpoint_before: reset.before,
            checkpoint_after: after,
            reason,
          });
        }}
      >
        {authorityStale && (
          <div role="alert" className="text-callout grid gap-2">
            <p>
              {tr(
                `This source changed (v${reset.version} → v${b.version}${b.enabled ? "" : ", disabled"}). Your draft is kept; Approve is off until you rebase it on the latest version.`,
                `这个来源已变化（v${reset.version} → v${b.version}${b.enabled ? "" : "，已停用"}）。草稿已保留；在最新版本上重新核对前不能批准。`
              )}
            </p>
            {b.enabled &&
              (reset.stream === "" || b.streams.includes(reset.stream)) && (
                <Button
                  variant="secondary"
                  className="justify-self-start"
                  onClick={() => patch({ version: b.version })}
                >
                  {tr(
                    "Keep my draft on the latest version",
                    "在最新版本上保留我的草稿"
                  )}
                </Button>
              )}
          </div>
        )}
        {cursorStale && (
          <div role="alert" className="text-callout grid gap-2">
            <p>
              {tr(
                `The stream checkpoint changed: you reviewed ${exact(reset.before, tr)}, it is now ${exact(live, tr)}. Your baseline and reason are kept; Approve is off until you adopt the current checkpoint.`,
                `流检查点已变化：你核对的是 ${exact(reset.before, tr)}，现在是 ${exact(live, tr)}。新基线和原因已保留；采用当前检查点前不能批准。`
              )}
            </p>
            {live !== undefined && (
              <Button
                variant="secondary"
                className="justify-self-start"
                onClick={() => patch({ before: live })}
              >
                {tr("Adopt the current checkpoint", "采用当前检查点")}
              </Button>
            )}
          </div>
        )}
        <p className="text-callout">
          {tr("Identity (fixed)", "身份（固定）")}
          {": "}
          <code>{principalKey(b.principal)}</code> · {b.provider} ·{" "}
          {b.account_scope}
        </p>
        <label className={fieldClass}>
          {tr("Stream", "流")}
          <AssistantSelect
            label={tr("Stream", "流")}
            value={reset.stream}
            options={[
              ["", tr("Choose", "请选择")],
              ...b.streams.map((id) => [id, id] as [string, string]),
            ]}
            onValueChange={(id) =>
              patch({
                stream: id,
                before: id === "" ? undefined : checkpointOf(b.source_id, id),
              })
            }
          />
        </label>
        {reset.stream !== "" && (
          <p className="text-callout break-all">
            {tr("Current checkpoint (read-only)", "当前检查点（只读）")}
            {": "}
            <code>{exact(reset.before, tr)}</code>
            {reset.before === undefined &&
              ` — ${tr("this Core did not report it, so a reset cannot be pinned to it", "当前 Core 未报告，无法按它固定重置")}`}
          </p>
        )}
        <label className="text-callout flex items-center gap-2">
          <input
            type="checkbox"
            checked={reset.nullBaseline}
            onChange={(e) => patch({ nullBaseline: e.target.checked })}
          />
          {tr(
            "New baseline is null (no checkpoint), not an empty string",
            "新基线为 null（无检查点），而不是空字符串"
          )}
        </label>
        <label className={fieldClass}>
          {tr(
            "New baseline (exact text, up to 4096 bytes)",
            "新基线（原样文本，最多 4096 字节）"
          )}
          <Input
            disabled={reset.nullBaseline}
            value={reset.nullBaseline ? "" : reset.baseline}
            onChange={(e) => patch({ baseline: e.target.value })}
          />
        </label>
        {!baselineOk && (
          <p role="alert" className="text-destructive text-callout">
            {tr(
              "The new baseline is over 4096 bytes.",
              "新基线超过 4096 字节。"
            )}
          </p>
        )}
        <label className={fieldClass}>
          {tr(
            "Gap reason (what may be skipped, up to 256 bytes)",
            "缺口原因（可能跳过的内容，最多 256 字节）"
          )}
          <Input
            value={reset.reason}
            onChange={(e) => patch({ reason: e.target.value })}
          />
        </label>
        {reset.reason !== "" && !reasonOk && (
          <p role="alert" className="text-destructive text-callout">
            {tr(
              "The reason must be text of at most 256 bytes without control characters.",
              "原因必须是不含控制字符、最多 256 字节的文本。"
            )}
          </p>
        )}
        {reset.stream !== "" && reset.before !== undefined && (
          <p
            role="note"
            className="text-callout text-muted-foreground break-all"
          >
            {tr(
              `Review. Source ${b.source_id}, connector ${principalKey(b.principal)}, account “${b.account_scope}” (adapter-reported, pinned by you), scope: ${b.project_paths.map(projectName).join(", ")}; goals: ${b.goal_ids.length ? b.goal_ids.map(goalTitle).join(", ") : "none"}. Stream “${reset.stream}”: checkpoint ${exact(reset.before, tr)} → ${exact(after, tr)}. Gap: ${reason || "(not given)"}. Approving only makes Core record an approval reference. It fetches nothing, resets nothing and sends nothing; you then configure the existing adapter with that reference. Events in the gap are not approved for execution.`,
              `请核对。来源 ${b.source_id}，连接器 ${principalKey(b.principal)}，账户“${b.account_scope}”（适配器自报，由你固定），范围：${b.project_paths.map(projectName).join("、")}；事项：${b.goal_ids.length ? b.goal_ids.map(goalTitle).join("、") : "无"}。流“${reset.stream}”：检查点 ${exact(reset.before, tr)} → ${exact(after, tr)}。缺口：${reason || "（未填写）"}。批准只会让 Core 记录一个批准引用；不会拉取、重置或发送任何内容，之后需要你用该引用配置现有适配器。缺口内的事件不会因此获准执行。`
            )}
          </p>
        )}
        <div className="flex gap-2">
          <Button type="submit" disabled={busy || !valid}>
            {tr("Approve reset", "批准重置")}
          </Button>
          <Button variant="ghost" onClick={() => setReset(null)}>
            {tr("Cancel", "取消")}
          </Button>
        </div>
      </form>
    );
  };

  // Approved resets are only evidence for the user: completion needs the stream to move too.
  const approvedResets = (b: SourceBinding) =>
    (obs?.resets ?? []).filter((r) => r.source_id === b.source_id);

  const card = (b: SourceBinding) => {
    const h: SourceHealth | undefined = health(b.source_id);
    const nonzero = h
      ? (
          [
            ["Accepted", "已接收", h.accepted],
            ["Filtered", "已过滤", h.filtered],
            ["Duplicate", "重复", h.duplicate],
            ["Conflicts", "冲突", h.conflicts],
            ["Rejected", "已拒绝", h.rejected],
          ] as const
        ).filter((row) => row[2] > 0)
      : [];
    return (
      <article
        key={b.source_id}
        aria-label={`${b.principal.connector_id} ${b.account_scope}`}
        className="border-border rounded-control grid gap-2 border p-4"
      >
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="min-w-0">
            <p className="text-body font-medium break-words">
              {b.principal.connector_id} · {b.account_scope}
            </p>
            <p className="text-metadata text-muted-foreground break-all">
              {principalKey(b.principal)} · {b.provider} · v{b.version}
            </p>
          </div>
          <div className="flex items-center gap-1">
            <Badge variant="outline">
              {b.enabled ? tr("Enabled", "已启用") : tr("Disabled", "已停用")}
            </Badge>
            {h && chip(healthChip(h))}
            <TooltipButton
              label={tr(
                `Edit ${b.principal.connector_id} ${b.account_scope}`,
                `编辑 ${b.principal.connector_id} ${b.account_scope}`
              )}
              variant="ghost"
              size="icon"
              disabled={busy}
              onClick={() => {
                setCap(null);
                setReset(null);
                setDraft(toDraft(b));
              }}
            >
              <Pencil aria-hidden />
            </TooltipButton>
            {b.enabled && (
              <TooltipButton
                label={tr(
                  `Disable ${b.principal.connector_id} ${b.account_scope}`,
                  `停用 ${b.principal.connector_id} ${b.account_scope}`
                )}
                variant="ghost"
                size="icon"
                disabled={busy}
                onClick={() =>
                  void submit({
                    kind: "source",
                    binding: {
                      source_id: b.source_id,
                      expected_version: b.version,
                      principal: b.principal,
                      provider: b.provider,
                      account_scope: b.account_scope,
                      resource_filter: b.resource_filter,
                      actor_filter: b.actor_filter,
                      project_paths: b.project_paths,
                      goal_ids: b.goal_ids,
                      streams: b.streams,
                      enabled: false,
                    },
                  })
                }
              >
                <Power aria-hidden />
              </TooltipButton>
            )}
          </div>
        </div>
        <p className="text-callout text-muted-foreground">
          {tr("Projects", "项目")}:{" "}
          {b.project_paths.map(projectName).join(", ")}
          {" · "}
          {tr("Goals", "事项")}:{" "}
          {b.goal_ids.length
            ? b.goal_ids.map(goalTitle).join(", ")
            : tr("none (events need attention)", "无（事件待你处理）")}
          {" · "}
          {tr("Streams", "流")}: {b.streams.join(", ")}
        </p>
        {(b.resource_filter.length > 0 || b.actor_filter.length > 0) && (
          <p className="text-callout text-muted-foreground break-all">
            {b.resource_filter.length > 0 &&
              `${tr("Resources", "资源")}: ${b.resource_filter.join(", ")} `}
            {b.actor_filter.length > 0 &&
              `${tr("Senders", "发送者")}: ${b.actor_filter.join(", ")}`}
          </p>
        )}
        {h && (nonzero.length > 0 || h.gap_count > 0) && (
          <p className="text-metadata text-muted-foreground">
            {nonzero.map(([en, cn, n]) => `${tr(en, cn)} ${n}`).join(" · ")}
            {h.gap_count > 0 &&
              ` ${tr("Gaps", "缺口")} ${h.gap_count} (${h.gap_first ?? "?"} – ${h.gap_last ?? "?"})`}
          </p>
        )}
        {h && h.state !== "ok" && (h.detail ?? "") !== "" && (
          <p role="status" className="text-callout break-words">
            {h.detail}
          </p>
        )}
        {h?.state === "needs_capacity" && cap?.id !== b.source_id && (
          <Button
            variant="secondary"
            className="justify-self-start"
            disabled={busy}
            onClick={() => {
              setDraft(null);
              setReset(null);
              setCap({
                id: b.source_id,
                version: b.version,
                source: String(
                  Math.min(
                    (b.history_limit ?? DEFAULT_SOURCE_HISTORY) * 2,
                    20_000
                  )
                ),
                global: "",
              });
            }}
          >
            {tr("Raise history limit", "提高历史上限")}
          </Button>
        )}
        {h?.state === "needs_reset" && reset?.id !== b.source_id && (
          <TooltipButton
            label={tr(
              `Reset cursor ${b.principal.connector_id} ${b.account_scope}`,
              `重置游标 ${b.principal.connector_id} ${b.account_scope}`
            )}
            variant="secondary"
            size="icon"
            className="justify-self-start"
            disabled={busy || !b.enabled}
            onClick={() => {
              setDraft(null);
              setCap(null);
              const only = b.streams.length === 1 ? b.streams[0] : "";
              setReset({
                id: b.source_id,
                version: b.version,
                stream: only,
                before:
                  only === "" ? undefined : checkpointOf(b.source_id, only),
                nullBaseline: false,
                baseline: "",
                reason: "",
              });
            }}
          >
            <RotateCcw aria-hidden />
          </TooltipButton>
        )}
        {capacityForm(b)}
        {resetForm(b)}
        {approvedResets(b).map((r) => {
          const live = checkpointOf(r.source_id, r.stream_id);
          return (
            <div
              key={r.approval_ref}
              role="status"
              className="text-callout border-border rounded-control grid gap-1 border p-3 break-all"
              aria-label={`${tr("Approved reset", "已批准的重置")} ${r.approval_ref}`}
            >
              <p>
                {tr("Approved reset reference", "已批准的重置引用")}:{" "}
                <code>{r.approval_ref}</code> · {tr("stream", "流")}{" "}
                {r.stream_id} · v{r.binding_version}
              </p>
              <p>
                {tr("Old checkpoint", "旧检查点")}:{" "}
                <code>{exact(r.checkpoint_before, tr)}</code> →{" "}
                {tr("new baseline", "新基线")}:{" "}
                <code>{exact(r.checkpoint_after, tr)}</code>
              </p>
              <p className="text-muted-foreground">
                {tr("Gap", "缺口")}: {r.reason} ·{" "}
                {new Date(r.approved_at_ms).toISOString()}
              </p>
              <p className="text-muted-foreground">
                {live === undefined
                  ? tr(
                      "This Core did not report the stream checkpoint, so completion cannot be confirmed.",
                      "当前 Core 未报告流检查点，无法确认是否已完成。"
                    )
                  : live === r.checkpoint_before
                    ? tr(
                        "Waiting: the stream is still at the old checkpoint. Configure the existing adapter with this reference; nothing was reset yet.",
                        "等待中：流仍在旧检查点。请用此引用配置现有适配器；尚未发生任何重置。"
                      )
                    : tr(
                        "The stream checkpoint moved, but this approval is still listed, so it is not confirmed as used. Refresh before relying on it.",
                        "流检查点已变化，但这条批准仍在列表中，尚未确认已被使用。请刷新后再确认。"
                      )}
              </p>
            </div>
          );
        })}
      </article>
    );
  };

  const runOf = (r: TargetReceipt) =>
    r.run_id === null
      ? undefined
      : state.scoped_runs?.find((s) => s.run.id === r.run_id);
  const unbound = (obs?.health ?? []).filter(
    (h) => h.kind === "principal" && h.unbound > 0
  );

  return (
    <section
      className="grid gap-4"
      aria-label={tr("Event sources", "事件来源")}
    >
      <div>
        <h2 tabIndex={-1} className="text-section-title font-semibold">
          {tr("Event sources", "事件来源")}
        </h2>
        <p className="text-callout text-muted-foreground max-w-2xl">
          {tr(
            "Observe only. External events can trigger review, questions and proposals; they cannot command, dispatch work, change controls or send messages.",
            "仅观察。外部事件只能触发复查、提问和提案，不能下达命令、派工、改控制或外发。"
          )}
        </p>
      </div>
      {(unavailable || loadError) && (
        <p role="alert" className="text-destructive text-callout">
          {unavailable
            ? tr(
                "Source inspector is not available from this Core.",
                "当前 Core 不提供来源检查。"
              )
            : loadError}
        </p>
      )}
      {sources.length === 0 && !draft && (
        <p className="text-muted-foreground text-body">
          {tr("No event sources are bound.", "尚未绑定事件来源。")}
        </p>
      )}
      {sources.map(card)}
      {unbound.map((h) => (
        <p key={h.key} className="text-callout text-muted-foreground break-all">
          {tr(
            `${h.unbound} event(s) from ${h.key.replace(/^principal:/, "")} arrived without a binding; no content was kept.`,
            `${h.key.replace(/^principal:/, "")} 有 ${h.unbound} 条事件因未绑定而未保留正文。`
          )}
        </p>
      ))}
      {draft ? (
        form()
      ) : (
        <div className="grid gap-2">
          <Button
            variant="secondary"
            className="justify-self-start"
            disabled={busy || connectors.length === 0 || !state.settings}
            onClick={() => {
              setCap(null);
              setReset(null);
              setDraft(emptyDraft);
            }}
          >
            <Plus aria-hidden />
            {tr("Add source", "添加来源")}
          </Button>
          {obs && connectors.length === 0 && (
            <p className="text-callout text-muted-foreground">
              {tr(
                "No installed, trusted connector declares observations.",
                "没有已安装且受信任、并声明 observations 的连接器。"
              )}
            </p>
          )}
        </div>
      )}
      <div className="grid gap-2">
        <h3 className="text-body font-semibold">{tr("Receipts", "回执")}</h3>
        {receipts.length === 0 && (
          <p className="text-callout text-muted-foreground">
            {tr("No receipts yet.", "暂无回执。")}
          </p>
        )}
        <ul className="grid gap-2">
          {receipts.slice(0, RECEIPT_LIMIT).map((r) => {
            const run = runOf(r);
            return (
              <li
                key={`${r.observation_id}:${r.goal_id}`}
                className="border-border rounded-control flex flex-wrap items-center justify-between gap-2 border px-3 py-2"
              >
                <div className="min-w-0">
                  <p className="text-callout break-words">
                    {goalTitle(r.goal_id)}
                  </p>
                  <p className="text-metadata text-muted-foreground break-all">
                    {sourceLabel(r.source_id)} · v{r.binding_version} ·{" "}
                    {r.observation_id}
                    {r.run_id !== null &&
                      ` · ${tr("run", "运行")} ${r.run_id}${run ? ` (${run.state})` : ""}`}
                    {r.output_id !== null &&
                      ` · ${tr("output", "输出")} ${r.output_id}`}
                  </p>
                </div>
                {chip(
                  RECEIPT[r.state] ?? [Info, r.state, r.state, r.state, r.state]
                )}
              </li>
            );
          })}
        </ul>
        {receipts.length > RECEIPT_LIMIT && (
          <p className="text-metadata text-muted-foreground">
            {tr(
              `Showing ${RECEIPT_LIMIT} of ${receipts.length}.`,
              `显示 ${RECEIPT_LIMIT} / ${receipts.length}。`
            )}
          </p>
        )}
      </div>
    </section>
  );
}

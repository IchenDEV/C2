import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Activity,
  ArrowLeft,
  Brain,
  Eye,
  ListChecks,
  MoreHorizontal,
  Settings,
  X,
} from "@/components/ui/icons";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { TooltipButton } from "@/components/ui/tooltip";

import type { MemoryRecord, Project, ProviderInfo } from "../bridge";
import { useLanguage } from "../i18n";
import { assistantApi, GLOBAL_MEMORY, sameEditTarget } from "./api";
import type {
  AssistantApi,
  AssistantSettings,
  AssistantSnapshot,
  Goal,
} from "./api";
import { AssistantSelect } from "./AssistantSelect";
import { ChiefThread } from "./ChiefThread";
import { DraftRebase, GoalCoordination } from "./CoordinationPanel";
import { ObservationInspector } from "./ObservationInspector";
import { GoalCard } from "./ThreadCards";

const fieldClass = "text-callout flex min-w-0 flex-col gap-2";
const initialSettings: AssistantSettings = {
  enabled: false,
  projects: [],
  provider: "",
  model: null,
  reasoning_effort: null,
  concurrency: 1,
  turn_limit: 10,
  dispatch_limit: 5,
};

type Api = AssistantApi;
export function AssistantWorkspace({
  activity,
  onSelect,
  onClose,
  api = assistantApi,
  initialTab = "thread",
}: {
  activity: ReactNode;
  onSelect: (id: string) => void;
  onClose: () => void;
  api?: Api;
  initialTab?: string;
}) {
  const { locale } = useLanguage();
  const zh = locale.startsWith("zh");
  const tr = (en: string, cn: string) => (zh ? cn : en);
  const [snapshot, setSnapshot] = useState<AssistantSnapshot | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [tab, setTab] = useState(initialTab);
  const [project, setProject] = useState("");
  const [focusGoal, setFocusGoal] = useState("");
  const [error, setError] = useState("");
  const [loadError, setLoadError] = useState("");
  const [busy, setBusy] = useState(false);
  const [settings, setSettings] = useState<AssistantSettings | null>(null);
  const [advanced, setAdvanced] = useState(false);
  const contentRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    contentRef.current?.scrollTo?.(0, 0);
    contentRef.current?.querySelector<HTMLElement>("h2")?.focus();
  }, [tab, focusGoal]);
  const refresh = useCallback(async () => {
    try {
      setSnapshot(await api.snapshot());
      setLoadError("");
    } catch (e) {
      setLoadError(String(e));
    }
  }, [api]);
  useEffect(() => {
    void refresh();
    void api
      .catalog()
      .then(([p, v]) => {
        setProjects(p);
        setProviders(v);
      })
      .catch((e: unknown) => setError(String(e)));
    const timer = setInterval(() => void refresh(), 5000);
    return () => clearInterval(timer);
  }, [refresh, api]);
  const edit = async (action: unknown, target?: Goal) => {
    if (!snapshot) return false;
    setBusy(true);
    setError("");
    try {
      const fresh = await api.snapshot();
      const before = target
        ? {
            ...snapshot.state,
            goals: snapshot.state.goals.map((g) =>
              g.id === target.id ? target : g
            ),
          }
        : snapshot.state;
      if (!sameEditTarget(before, fresh.state, action))
        throw new Error(
          tr(
            "This target changed. Review its latest requirements and retry; your draft is retained.",
            "这个目标已变化，请核对最新要求后重试。草稿已保留。"
          )
        );
      await api.edit(fresh.state.revision, action);
      await refresh();
      return true;
    } catch (e) {
      setError(String(e));
      await refresh();
      return false;
    } finally {
      setBusy(false);
    }
  };
  const state = snapshot?.state;
  const selectedProjects = projects.filter(
    (p) => state?.settings?.projects.includes(p.path) === true
  );
  const selectedGoal = state?.goals.find((g) => g.id === focusGoal);
  const openSession = (id: string) => {
    onSelect(id);
    onClose();
  };
  const openGoal = (id: string) => {
    setFocusGoal(id);
    setTab("tasks");
  };
  const threadActive = tab === "thread" && !selectedGoal;
  const openSettings = () => {
    setFocusGoal("");
    setSettings(state?.settings ?? initialSettings);
    setTab("settings");
  };
  const statusLabel = (s: Goal["status"]) =>
    ({
      active: tr("Active", "推进中"),
      paused: tr("Paused", "已暂停"),
      needs_attention: tr("Needs a decision", "待决定"),
      completed: tr("Accepted", "已验收"),
      cancelled: tr("Cancelled", "已取消"),
    })[s];
  const projectName = (path: string) =>
    projects.find((p) => p.path === path)?.name ?? path;
  const cardContext = state
    ? {
        state,
        busy,
        edit,
        zh,
        projectName,
        onGoal: openGoal,
        onSession: openSession,
        developer: !threadActive,
      }
    : null;
  const projectPicker = (
    <AssistantSelect
      label={tr("Project", "项目")}
      value={project}
      options={[
        ["", tr("All projects", "全部项目")],
        ...selectedProjects.map((p) => [p.path, p.name] as [string, string]),
      ]}
      onValueChange={setProject}
    />
  );
  return (
    <section
      className="bg-background text-foreground fixed inset-0 z-40 flex min-w-0 flex-col"
      aria-label={tr("Chief of staff", "幕僚")}
    >
      <header className="mx-auto flex w-full max-w-5xl items-center justify-between gap-3 px-4 py-2 sm:px-6">
        <h1 className="text-body font-semibold">
          {tr("Chief of staff", "幕僚")}
        </h1>
        <nav
          className="flex items-center gap-1"
          aria-label={tr("Workspace tools", "工作区工具")}
        >
          {!threadActive && (
            <TooltipButton
              label={tr("Back to conversation", "返回对话")}
              variant="ghost"
              size="icon"
              onClick={() => {
                setFocusGoal("");
                setTab("thread");
              }}
            >
              <ArrowLeft aria-hidden />
            </TooltipButton>
          )}
          {(
            [
              ["memory", tr("Memory", "记忆"), Brain],
              ["tasks", tr("Continuous tasks", "持续任务"), ListChecks],
              ["settings", tr("Settings", "设置"), Settings],
            ] as const
          ).map(([id, label, Icon]) => {
            const current = tab === id && !selectedGoal;
            return (
              <TooltipButton
                key={id}
                label={label}
                variant={current ? "secondary" : "ghost"}
                size="icon"
                aria-current={current ? "page" : undefined}
                onClick={() => {
                  if (id === "settings") {
                    openSettings();
                    return;
                  }
                  setFocusGoal("");
                  setTab(id);
                }}
              >
                <Icon aria-hidden />
              </TooltipButton>
            );
          })}
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={tr("More tools", "更多工具")}
                >
                  <MoreHorizontal aria-hidden />
                </Button>
              }
            />
            <DropdownMenuContent align="end">
              <DropdownMenuItem
                onClick={() => {
                  setFocusGoal("");
                  setTab("activity");
                }}
              >
                <Activity aria-hidden />
                {tr("Execution diagnostics", "执行诊断")}
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={() => {
                  setFocusGoal("");
                  setTab("sources");
                }}
              >
                <Eye aria-hidden />
                {tr("Event sources", "事件来源")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <TooltipButton
            label={tr("Close", "关闭")}
            variant="ghost"
            size="icon"
            onClick={onClose}
          >
            <X aria-hidden />
          </TooltipButton>
        </nav>
      </header>
      {(error || loadError) && (
        <p
          role="alert"
          className="text-destructive text-callout mx-auto w-full max-w-5xl shrink-0 px-4 py-2 sm:px-6"
        >
          {error || loadError}
        </p>
      )}
      {!state && !loadError && (
        <p role="status" className="text-muted-foreground mx-auto px-4 py-6">
          {tr("Loading…", "正在读取…")}
        </p>
      )}
      {state && cardContext && (
        <ChiefThread
          state={state}
          managed={selectedProjects}
          active={threadActive}
          ctx={cardContext}
          onConfigure={openSettings}
        />
      )}
      <div
        ref={contentRef}
        hidden={threadActive}
        className="min-h-0 flex-1 overflow-y-auto"
      >
        <div className="mx-auto flex w-full max-w-5xl flex-col gap-6 px-4 py-6 sm:px-6">
          {tab === "settings" && settings ? (
            <form
              className="grid max-w-2xl gap-6"
              onSubmit={(e) => {
                e.preventDefault();
                void edit({ kind: "settings", settings }).then((saved) => {
                  if (saved) setTab("thread");
                });
              }}
            >
              <div>
                <h2 tabIndex={-1} className="text-section-title font-semibold">
                  {tr("Scope and follow-up", "管理范围与自动跟进")}
                </h2>
                <p className="text-callout text-muted-foreground">
                  {tr(
                    "Choose the projects the chief of staff may follow.",
                    "选择幕僚可以跟进的项目。"
                  )}
                </p>
              </div>
              <fieldset className="space-y-3">
                <legend className="text-body mb-3">
                  {tr("Projects to manage", "管理的项目")}
                </legend>
                {projects.map((p) => (
                  <label
                    key={p.path}
                    className="text-body flex items-center gap-2"
                  >
                    <input
                      type="checkbox"
                      checked={settings.projects.includes(p.path)}
                      onChange={(e) =>
                        setSettings({
                          ...settings,
                          projects: e.target.checked
                            ? [...settings.projects, p.path]
                            : settings.projects.filter((v) => v !== p.path),
                        })
                      }
                    />
                    {p.name}
                  </label>
                ))}
              </fieldset>
              <label className={fieldClass}>
                {tr("Provider", "模型提供方")}
                <AssistantSelect
                  label={tr("Provider", "模型提供方")}
                  value={settings.provider}
                  options={[
                    ["", tr("Choose", "请选择")],
                    ...providers
                      .filter((p) => p.available && p.enabled)
                      .map((p) => [p.id, p.display_name] as [string, string]),
                  ]}
                  onValueChange={(value) =>
                    setSettings({ ...settings, provider: value, model: null })
                  }
                />
              </label>
              <label className="text-body flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={settings.enabled}
                  onChange={(e) =>
                    setSettings({ ...settings, enabled: e.target.checked })
                  }
                />
                {tr(
                  "Enable automatic follow-up within this scope",
                  "允许在此范围内自动跟进"
                )}
              </label>
              <Button
                variant="ghost"
                className="justify-self-start"
                aria-expanded={advanced}
                aria-controls="assistant-advanced-settings"
                onClick={() => setAdvanced(!advanced)}
              >
                {tr("Advanced settings", "高级设置")}
              </Button>
              <div
                id="assistant-advanced-settings"
                hidden={!advanced}
                className="grid gap-4 sm:grid-cols-2"
              >
                <label className={fieldClass}>
                  {tr(
                    "Model (empty uses provider default)",
                    "模型（留空使用提供方默认值）"
                  )}
                  <Input
                    value={settings.model ?? ""}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        model: e.target.value || null,
                      })
                    }
                  />
                </label>
                <label className={fieldClass}>
                  {tr("Reasoning effort (optional)", "推理强度（可选）")}
                  <Input
                    value={settings.reasoning_effort ?? ""}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        reasoning_effort: e.target.value || null,
                      })
                    }
                  />
                </label>
                <label className={fieldClass}>
                  {tr("Concurrent goals (1–4)", "同时推进的目标数（1–4）")}
                  <Input
                    type="number"
                    min={1}
                    max={4}
                    value={settings.concurrency}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        concurrency: Number(e.target.value),
                      })
                    }
                  />
                </label>
                <div className="grid grid-cols-2 gap-3">
                  <label className={fieldClass}>
                    {tr("Review allowance", "复查额度")}
                    <Input
                      type="number"
                      min={1}
                      max={100}
                      value={settings.turn_limit}
                      onChange={(e) =>
                        setSettings({
                          ...settings,
                          turn_limit: Number(e.target.value),
                        })
                      }
                    />
                  </label>
                  <label className={fieldClass}>
                    {tr("Dispatch allowance", "派工额度")}
                    <Input
                      type="number"
                      min={1}
                      max={100}
                      value={settings.dispatch_limit}
                      onChange={(e) =>
                        setSettings({
                          ...settings,
                          dispatch_limit: Number(e.target.value),
                        })
                      }
                    />
                  </label>
                </div>
              </div>
              <p className="text-callout text-muted-foreground">
                {tr(
                  "Requires Core online. Workers retain normal approvals. Saving grants a fresh bounded allowance; pausing leaves existing workers running.",
                  "需要 Core 在线。执行者保留现有操作审批。保存会重新授予额度；暂停跟进不会停止正在执行的工作。"
                )}
              </p>
              <div className="flex gap-2">
                <Button
                  type="submit"
                  disabled={
                    busy || !settings.projects.length || !settings.provider
                  }
                >
                  {tr("Save scope", "保存范围")}
                </Button>
                <Button variant="ghost" onClick={() => setTab("thread")}>
                  {tr("Cancel", "取消")}
                </Button>
              </div>
            </form>
          ) : tab === "tasks" && !selectedGoal && state && cardContext ? (
            <section
              className="space-y-4"
              aria-label={tr("Continuous tasks", "持续任务")}
            >
              <div className="max-w-xs">{projectPicker}</div>
              {state.goals
                .filter((g) => !project || g.project_path === project)
                .toSorted((a, b) => a.priority - b.priority)
                .map((g) => (
                  <GoalCard key={g.id} goal={g} ctx={cardContext} />
                ))}
              {state.goals.length === 0 && (
                <p className="text-muted-foreground text-body">
                  {tr("No tasks", "暂无任务")}
                </p>
              )}
            </section>
          ) : tab === "activity" ? (
            activity
          ) : tab === "sources" && state ? (
            <ObservationInspector
              state={state}
              api={api}
              edit={edit}
              busy={busy}
              zh={zh}
              projectName={projectName}
            />
          ) : tab === "memory" ? (
            <>
              <div className="max-w-xs">{projectPicker}</div>
              <MemoryNotebook
                key={project || GLOBAL_MEMORY}
                records={snapshot?.memories ?? []}
                goals={
                  state?.goals.filter(
                    (g) =>
                      state.settings?.projects.includes(g.project_path) === true
                  ) ?? []
                }
                scope={project || GLOBAL_MEMORY}
                api={api}
                refresh={refresh}
                onSelect={openSession}
                edit={edit}
                zh={zh}
              />
            </>
          ) : selectedGoal && state ? (
            <>
              <Button
                variant="ghost"
                className="self-start"
                onClick={() => {
                  setFocusGoal("");
                  setTab("thread");
                }}
              >
                {tr("Back to conversation", "返回对话")}
              </Button>
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="text-callout text-muted-foreground">
                    {projectName(selectedGoal.project_path)}
                  </p>
                  <h2
                    tabIndex={-1}
                    className="text-page-title font-semibold break-words"
                  >
                    {selectedGoal.title}
                  </h2>
                </div>
                <Badge variant="outline">
                  {statusLabel(selectedGoal.status)}
                </Badge>
              </div>
              <GoalCoordination
                key={selectedGoal.id}
                goal={selectedGoal}
                state={state}
                busy={busy}
                edit={edit}
                zh={zh}
                onSession={openSession}
                management={
                  <GoalEditor
                    goal={selectedGoal}
                    busy={busy}
                    onSave={edit}
                    zh={zh}
                  />
                }
              />
            </>
          ) : null}
        </div>
      </div>
    </section>
  );
}

function GoalEditor({
  goal,
  busy,
  onSave,
  zh,
}: {
  goal: Goal;
  busy: boolean;
  onSave: (action: unknown, target?: Goal) => Promise<boolean>;
  zh: boolean;
}) {
  const [title, setTitle] = useState(goal.title);
  const [acceptance, setAcceptance] = useState(goal.acceptance);
  const [priority, setPriority] = useState(goal.priority);
  const [baseline, setBaseline] = useState<Goal>(goal);
  return (
    <form
      className="bg-fill-quiet rounded-control grid gap-3 p-4"
      onSubmit={(e) => {
        e.preventDefault();
        void onSave(
          {
            kind: "goal",
            id: goal.id,
            project_path: goal.project_path,
            title,
            acceptance,
            priority,
          },
          baseline
        ).then((saved) => {
          if (saved)
            setBaseline({
              ...goal,
              title,
              acceptance,
              priority,
              contract_revision:
                goal.contract_revision +
                (goal.title !== title || goal.acceptance !== acceptance
                  ? 1
                  : 0),
            });
        });
      }}
    >
      <DraftRebase
        baseline={baseline}
        current={goal}
        adopt={() => setBaseline(goal)}
        zh={zh}
      />
      <label className={fieldClass}>
        {zh ? "优先级" : "Priority"}
        <AssistantSelect
          label={zh ? "优先级" : "Priority"}
          value={String(priority)}
          onValueChange={(v) => setPriority(Number(v))}
          options={[0, 1, 2, 3].map((n) => [String(n), `P${n}`])}
        />
      </label>
      <label className={fieldClass}>
        {zh ? "目标" : "Goal"}
        <Input
          required
          maxLength={4000}
          value={title}
          onChange={(e) => setTitle(e.target.value)}
        />
      </label>
      <label className={fieldClass}>
        {zh ? "怎样才算完成" : "Acceptance criteria"}
        <Textarea
          required
          maxLength={8000}
          value={acceptance}
          onChange={(e) => setAcceptance(e.target.value)}
        />
      </label>
      <div>
        <Button type="submit" disabled={busy}>
          {zh ? "保存修改" : "Save changes"}
        </Button>
      </div>
    </form>
  );
}

function MemoryNotebook({
  records,
  goals,
  scope,
  api,
  refresh,
  onSelect,
  edit,
  zh,
}: {
  records: MemoryRecord[];
  goals: Goal[];
  scope: string;
  api: Api;
  refresh: () => Promise<void>;
  onSelect: (id: string) => void;
  edit: (action: unknown, target?: Goal) => Promise<boolean>;
  zh: boolean;
}) {
  const [content, setContent] = useState("");
  const [adding, setAdding] = useState(false);
  const [category, setCategory] = useState("fact");
  const [editing, setEditing] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [affected, setAffected] = useState<string[]>([]);
  const save = async (name: string, args: unknown) => {
    setBusy(true);
    setError("");
    try {
      await api.memory(name, args);
      if (name === "update" || name === "set_active") {
        setAffected(
          goals
            .filter((g) => {
              const a = g.assignments.at(-1);
              return (
                g.status === "active" &&
                (scope === GLOBAL_MEMORY || g.project_path === scope) &&
                a?.owned === true &&
                a.submitted &&
                !a.taken_over
              );
            })
            .map((g) => g.id)
        );
      }
      await refresh();
      setContent("");
      setEditing(null);
      setAdding(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const shown = records.filter(
    (r) => r.project_path === scope && r.active && r.layer === "L1"
  );
  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-section-title font-semibold">
          {scope === GLOBAL_MEMORY
            ? zh
              ? "全局记忆"
              : "Shared memory"
            : zh
              ? "项目记忆"
              : "Project memory"}
        </h2>
        <p className="text-callout text-muted-foreground">
          {zh
            ? "记录背景、约束、偏好和关键决定。遗忘后不再用于新的复查；已发送给模型的历史内容仍保留在原会话。"
            : "Keep context, constraints, preferences and decisions. Forgotten notes stop appearing in new reviews; earlier provider conversations retain what was already sent."}
        </p>
      </div>
      {error && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}
      {affected.length > 0 && (
        <div className="border-border rounded-control space-y-3 border p-4">
          <p className="text-prose">
            {zh
              ? "以下委派可能已使用旧记忆。新的回忆已更新；如需改变执行中的工作，请发送明确的纠正。"
              : "These assignments may have used the previous memory. New recall is updated; send an explicit correction to change their current work."}
          </p>
          {goals
            .filter(
              (g) =>
                affected.includes(g.id) &&
                g.status === "active" &&
                g.assignments.at(-1)?.taken_over === false
            )
            .map((g) => (
              <MemoryCorrection key={g.id} goal={g} edit={edit} zh={zh} />
            ))}
        </div>
      )}
      <Button variant="secondary" onClick={() => setAdding(true)}>
        {zh ? "添加记忆" : "Add memory"}
      </Button>
      <div hidden={!adding && editing === null}>
        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            void save(
              editing === null ? "add" : "update",
              editing === null
                ? { project_path: scope, category, content, pinned: true }
                : { id: editing, category, content }
            );
          }}
        >
          <label className={fieldClass}>
            {zh ? "类型" : "Category"}
            <AssistantSelect
              label={zh ? "类型" : "Category"}
              value={category}
              onValueChange={setCategory}
              options={[
                ["fact", zh ? "背景 / 事实" : "Context / fact"],
                ["event", zh ? "关键决定" : "Decision"],
                ["preference", zh ? "偏好" : "Preference"],
                ["constraint", zh ? "约束" : "Constraint"],
              ]}
            />
          </label>
          <label className={fieldClass}>
            {zh ? "需要记住什么" : "What should be remembered"}
            <Textarea
              required
              maxLength={16_000}
              value={content}
              onChange={(e) => setContent(e.target.value)}
            />
          </label>
          <div className="flex gap-2">
            <Button type="submit" disabled={busy}>
              {editing === null
                ? zh
                  ? "记住"
                  : "Remember"
                : zh
                  ? "保存纠正"
                  : "Save correction"}
            </Button>
            {(adding || editing !== null) && (
              <Button
                variant="ghost"
                onClick={() => {
                  setEditing(null);
                  setAdding(false);
                  setContent("");
                }}
              >
                {zh ? "取消" : "Cancel"}
              </Button>
            )}
          </div>
        </form>
      </div>
      {shown.length === 0 && (
        <p className="text-muted-foreground">
          {zh ? "这个范围还没有记忆。" : "No memories in this scope yet."}
        </p>
      )}
      {shown.map((record) => (
        <article key={record.id} className="space-y-2 pt-4">
          <p className="text-prose whitespace-pre-wrap">{record.content}</p>
          <p className="text-metadata text-muted-foreground">
            {record.category} · {record.origin} ·{" "}
            {new Date(record.updated_at).toLocaleString()}
          </p>
          <div className="flex gap-2">
            {record.session_id !== null && (
              <Button
                variant="ghost"
                onClick={() => onSelect(record.session_id!)}
              >
                {zh ? "查看来源" : "View source"}
              </Button>
            )}
            {record.editable && (
              <>
                <Button
                  variant="ghost"
                  disabled={busy}
                  onClick={() => {
                    setEditing(record.id);
                    setContent(record.content);
                    setCategory(record.category);
                  }}
                >
                  {zh ? "纠正" : "Correct"}
                </Button>
                <Button
                  variant="ghost"
                  disabled={busy}
                  onClick={() =>
                    void save("set_active", { id: record.id, value: false })
                  }
                >
                  {zh ? "遗忘" : "Forget"}
                </Button>
              </>
            )}
          </div>
        </article>
      ))}
    </div>
  );
}

function MemoryCorrection({
  goal,
  edit,
  zh,
}: {
  goal: Goal;
  edit: (action: unknown, target?: Goal) => Promise<boolean>;
  zh: boolean;
}) {
  const [content, setContent] = useState("");
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState(false);
  const [baseline, setBaseline] = useState<Goal | null>(null);
  return (
    <form
      className="space-y-2"
      onSubmit={(e) => {
        e.preventDefault();
        setBusy(true);
        void edit(
          { kind: "message", goal_id: goal.id, content, queue: false },
          baseline ?? goal
        )
          .then((saved) => {
            if (saved) {
              setContent("");
              setSent(true);
              setBaseline(null);
            }
          })
          .finally(() => setBusy(false));
      }}
    >
      <DraftRebase
        baseline={baseline}
        current={goal}
        adopt={() => setBaseline(goal)}
        zh={zh}
      />
      <label className={fieldClass}>
        {goal.title}
        <Textarea
          required
          value={content}
          maxLength={16_000}
          onChange={(e) => {
            setBaseline((old) => old ?? goal);
            setContent(e.target.value);
            setSent(false);
          }}
        />
      </label>
      <Button type="submit" disabled={busy}>
        {zh ? "发送记忆纠正" : "Send memory correction"}
      </Button>
      {sent && (
        <p className="text-callout text-muted-foreground">
          {zh
            ? "已记录纠正；目标时间线显示投递与确认状态。"
            : "Correction recorded; the goal timeline shows delivery and confirmation."}
        </p>
      )}
    </form>
  );
}

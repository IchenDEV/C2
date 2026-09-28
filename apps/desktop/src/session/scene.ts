import type { MemoryAccess } from "../bridge";
import { SESSION_MODES } from "./mode";
import type { SessionMode } from "./mode";

/**
 * Agent Scenes 1.0.0 wire shapes and pure helpers (see docs/reference/scenes.md).
 *
 * Like `mode.ts`, this file is pure logic so the binding matrix, the escalation ordering, and the
 * cycling ring can be unit-tested without a DOM or a bridge.
 */

export type SceneSource = "builtin" | "user" | "project" | "plugin";

/** One typed fill-in slot — shared vocabulary between Macro skills and scene briefs. */
export interface SceneSlotDef {
  id: string;
  label: string;
  kind: "text" | "multiline" | "select" | "file" | "artifact";
  options?: string[];
  required?: boolean;
  default?: string;
}

export interface SceneBrief {
  template: string;
  slots?: SceneSlotDef[];
  clarify?: "multi_choice" | "free_form" | "off";
}

export interface SceneArtifactDef {
  id: string;
  title: string;
  kind:
    | "document"
    | "plan"
    | "report"
    | "test_report"
    | "checklist"
    | "diff"
    | "link"
    | "custom";
  required?: boolean;
  template?: string;
  description?: string;
}

export interface SceneLocalization {
  title?: string | null;
  description?: string | null;
}

export interface SceneAuthor {
  name?: string;
  email?: string;
  url?: string;
}

export interface SceneExecution {
  providers?: string[];
  model?: string;
  reasoning_effort?: string;
  session_mode?: SessionMode;
  memory_preset?: MemoryPresetId;
  worktree?: "off" | "current" | "origin_default";
}

export interface SceneSkills {
  pinned?: string[];
  inline?: SceneInlineFragment[];
  suppress_unpinned?: boolean;
}

export interface SceneInlineFragment {
  name: string;
  text: string;
  icon?: string;
}

export type SceneExitCriterionKind =
  | "required_artifacts"
  | "checklist_complete"
  | "tests_pass"
  | "user_confirm"
  | "custom";

export interface SceneExitCriterion {
  kind: SceneExitCriterionKind;
  artifact?: string;
  description?: string;
}

export interface SceneExit {
  criteria?: SceneExitCriterion[];
  next?: SceneNextSuggestion[];
}

export type SceneHookEvent =
  | "enter"
  | "turn_end"
  | "artifact_produced"
  | "exit_criteria_met"
  | "tests_failed"
  | "schedule";

export type SceneHookActionKind =
  | "suggest_scene"
  | "suggest_next"
  | "run_macro"
  | "notify";

export interface SceneHook {
  on: SceneHookEvent;
  artifact?: string;
  schedule?: string;
  action: {
    kind: SceneHookActionKind;
    scene?: string;
    macro?: string;
    args?: Record<string, string>;
    message?: string;
  };
}

export interface SceneConstraints {
  guardrails?: string[];
  tools?: { allow?: string[]; deny?: string[] };
}

/** Lossless Agent Scenes 1.0 document used by the editor and desktop bridge. */
export interface SceneDocument {
  $schema: string;
  name: string;
  version?: string;
  title: string;
  description?: string;
  icon?: string;
  author?: SceneAuthor;
  homepage?: string;
  repository?: string;
  license?: string;
  keywords?: string[];
  localizations?: Record<string, SceneLocalization>;
  execution?: SceneExecution;
  skills?: SceneSkills;
  brief?: SceneBrief;
  artifacts?: SceneArtifactDef[];
  exit?: SceneExit;
  hooks?: SceneHook[];
  constraints?: SceneConstraints;
  extensions?: Record<string, Record<string, unknown>>;
}

/** One resolved scene as `list_scenes` reports it. */
export interface SceneInfo {
  reference: string;
  name: string;
  title: string;
  description: string;
  icon?: string | null;
  source: SceneSource;
  plugin_id?: string | null;
  keywords: string[];
  has_brief: boolean;
  localizations: Record<string, SceneLocalization>;
  execution?: SceneExecution | null;
  brief?: SceneBrief | null;
  artifacts: SceneArtifactDef[];
  skills?: SceneSkills | null;
  /** Appended for R8's completion banner: exit criteria and next-scene suggestions. */
  exit?: SceneExit | null;
}

/** One `exit.next` entry — a suggested follow-up scene with its carry set. */
export interface SceneNextSuggestion {
  scene: string;
  label?: string | null;
  carry?: string[];
}

export type MemoryPresetId =
  | "standard"
  | "read_only"
  | "private"
  | "learn_only";

/** Mirrors core `memory_preset_policy`, in the frontend's (read, write) vocabulary. */
export const MEMORY_PRESET_POLICY: Record<
  MemoryPresetId,
  { read: MemoryAccess; write: MemoryAccess }
> = {
  standard: { read: "inherit", write: "inherit" },
  read_only: { read: "allow", write: "deny" },
  private: { read: "deny", write: "deny" },
  learn_only: { read: "deny", write: "allow" },
};

/** The live per-session posture a scene's set fields are compared against. */
export interface LivePosture {
  mode: SessionMode;
  memoryRead: MemoryAccess;
  memoryWrite: MemoryAccess;
  provider: string;
  model: string | null;
}

/** Display title for the UI locale, falling back to the authored title. */
export function sceneTitle(scene: SceneInfo, locale: string): string {
  return scene.localizations[locale]?.title ?? scene.title;
}

/**
 * Has the user overridden any field this scene sets? Unset scene fields mean "inherit" and can
 * never count as customized.
 */
export function sceneCustomized(scene: SceneInfo, live: LivePosture): boolean {
  const { execution } = scene;
  if (!execution) return false;
  if (
    execution.session_mode !== undefined &&
    execution.session_mode !== live.mode
  )
    return true;
  if (execution.memory_preset !== undefined) {
    const preset = MEMORY_PRESET_POLICY[execution.memory_preset];
    if (preset.read !== live.memoryRead || preset.write !== live.memoryWrite)
      return true;
  }
  if (
    execution.providers !== undefined &&
    execution.providers.length > 0 &&
    execution.providers[0] !== live.provider
  ) {
    return true;
  }
  if (
    execution.model !== undefined &&
    live.model !== null &&
    execution.model !== live.model
  ) {
    return true;
  }
  return false;
}

/**
 * Which scene fields a soft-apply cannot honor (binding matrix, docs/reference/scenes.md §Binding):
 * providers/model/reasoning_effort bind at session creation; worktree is immutable per session.
 * When the live value already matches, the field is not pending.
 */
export function softApplyPending(
  scene: SceneInfo,
  live: LivePosture | null
): string[] {
  const { execution } = scene;
  if (!execution) return [];
  const pending: string[] = [];
  if (execution.providers !== undefined && execution.providers.length > 0) {
    if (!live || execution.providers[0] !== live.provider)
      pending.push("providers");
  }
  if (execution.model !== undefined) {
    if (!live || live.model === null || execution.model !== live.model)
      pending.push("model");
  }
  if (execution.reasoning_effort !== undefined)
    pending.push("reasoning_effort");
  if (execution.worktree !== undefined) pending.push("worktree");
  return pending;
}

/**
 * Does applying this scene loosen permissions? Ordering per `SESSION_MODES` (loosest last).
 * Loosening always needs an explicit user confirmation naming both modes — never silent.
 */
export function escalationNeeded(
  scene: SceneInfo,
  currentMode: SessionMode
): { from: SessionMode; to: SessionMode } | null {
  const target = scene.execution?.session_mode;
  if (!target) return null;
  const rank = (m: SessionMode) =>
    SESSION_MODES.findIndex((entry) => entry.id === m);
  if (rank(target) > rank(currentMode))
    return { from: currentMode, to: target };
  return null;
}

/**
 * The next scene reference for the cycle shortcut. An empty ring falls back to every resolved
 * scene in listing order. Wraps; null when there is nothing else to cycle to.
 */
export function nextSceneInRing(
  ring: readonly string[],
  scenes: readonly SceneInfo[],
  active: string | null
): string | null {
  const order = ring.length > 0 ? ring : scenes.map((s) => s.reference);
  if (order.length === 0) return null;
  const index = active === null ? -1 : order.indexOf(active);
  const next = order[(index + 1) % order.length];
  return next === active ? null : next;
}

/** The slice of a session config option the effort matcher needs (mirrors bridge's shape). */
export interface EffortOptionLike {
  id: string;
  category?: string | null;
  choices: { id: string; name?: string | null }[];
}

/**
 * Resolve a scene's `reasoning_effort` against the session's reported config options.
 * The effort config id is provider-specific and unknowable before the session reports it —
 * this is why the binding matrix defers the field; once options arrive, apply it exactly once.
 */
export function sceneEffortChoice(
  options: readonly EffortOptionLike[],
  effort: string
): { configId: string; value: string } | null {
  const option = options.find(
    (o) =>
      o.category === "thought_level" ||
      o.id === "effort" ||
      o.id === "reasoning_effort"
  );
  if (!option) return null;
  const wanted = effort.toLowerCase();
  const choice = option.choices.find(
    (c) =>
      c.id.toLowerCase() === wanted || (c.name ?? "").toLowerCase() === wanted
  );
  return choice ? { configId: option.id, value: choice.id } : null;
}

/** The slice of a skill listing the scene-aware `/` picker needs. */
export interface SkillLike {
  id: string;
}

/**
 * Order the `/` picker for the active scene: pinned skills first (in pin order), the rest after.
 * With `suppress_unpinned` and `showAll` false, unpinned skills are hidden behind a "show all"
 * affordance — hiddenCount tells the picker how many wait behind it (spec: the affordance must
 * always remain reachable).
 */
export function orderSkillsForScene<T extends SkillLike>(
  skills: readonly T[],
  scene: SceneInfo | null,
  showAll: boolean
): { items: T[]; hiddenCount: number } {
  const pinned = scene?.skills?.pinned ?? [];
  if (pinned.length === 0) return { items: [...skills], hiddenCount: 0 };
  const rank = new Map(pinned.map((id, index) => [id, index]));
  const front = [...skills]
    .filter((s) => rank.has(s.id))
    .toSorted((a, b) => (rank.get(a.id) ?? 0) - (rank.get(b.id) ?? 0));
  const rest = skills.filter((s) => !rank.has(s.id));
  if (scene?.skills?.suppress_unpinned === true && !showAll) {
    return { items: front, hiddenCount: rest.length };
  }
  return { items: [...front, ...rest], hiddenCount: 0 };
}

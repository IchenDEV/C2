import { call } from "../bridge";
import type {
  MemoryRecord,
  SessionInfo,
  Project,
  ProviderInfo,
  PendingInput,
} from "../bridge";

export interface ManagedProjectBinding {
  id: string;
  path: string;
  version: number;
  active: boolean;
  source: string;
  revoked_at?: string | null;
}

export interface ManagedProject {
  id: string;
  name: string;
  status: "active" | "paused" | "retired";
  version: number;
  created_at: string;
  bindings: ManagedProjectBinding[];
}

export type HierarchyScope =
  | { kind: "global" }
  | { kind: "project"; project_id: string };

export type Reach = "chief" | "project_manager" | "executor";

export interface InstructionRevision {
  scope: HierarchyScope;
  revision: number;
  text: string;
  hash: string;
  reach: Reach;
  proposal_id: string;
  confirmed_by: string;
  confirmed_at: string;
  receipt: string;
}

export interface InstructionProposal {
  id: string;
  scope: HierarchyScope;
  base_revision: number;
  text: string;
  hash: string;
  reach: Reach;
  proposer: string;
  created_at: string;
  state: "proposed" | "confirmed" | "rejected" | "stale";
}

export interface GoalOwner {
  project_id: string;
  binding_id: string;
  epoch: number;
}

export interface MemoryShare {
  id: string;
  memory_id: string;
  source_scope: string;
  content_hash: string;
  target: HierarchyScope;
  reach: Reach;
  state: "proposed" | "approved" | "revoked";
  proposer: string;
  created_at: string;
  decided_at?: string | null;
}

export interface ProjectHierarchy {
  schema: number;
  revision: number;
  projects: ManagedProject[];
  instructions: InstructionRevision[];
  proposals: InstructionProposal[];
  shares: MemoryShare[];
  goal_owners: Record<string, GoalOwner>;
}

export const GLOBAL_MEMORY = "codetwo://chief-of-staff";
export interface AssistantSettings {
  enabled: boolean;
  projects: string[];
  provider: string;
  model: string | null;
  reasoning_effort: string | null;
  concurrency: number;
  turn_limit: number;
  dispatch_limit: number;
}
export interface Goal {
  id: string;
  project_path: string;
  title: string;
  acceptance: string;
  priority: number;
  status: "active" | "paused" | "needs_attention" | "completed" | "cancelled";
  contract_revision: number;
  dependencies: string[];
  next_step: string;
  blocker: string;
  assignments: {
    id: string;
    session_id: string | null;
    submitted: boolean;
    taken_over: boolean;
    owned: boolean;
    contract_revision: number;
    confirmed_revision: number;
    stop_requested: boolean;
    prior_results: {
      id: string;
      contract_revision: number;
      evidence: string;
      artifacts: { path: string; sha256: string }[];
      state: string;
    }[];
    result: {
      id: string;
      contract_revision: number;
      evidence: string;
      artifacts: { path: string; sha256: string }[];
      state: string;
    } | null;
  }[];
  verdict: {
    session_id: string;
    activity_revision: number;
    evidence: string;
    artifacts: { path: string; sha256: string }[];
  } | null;
}
export interface AssistantState {
  revision: number;
  settings: AssistantSettings | null;
  goals: Goal[];
  run: { session_id: string | null } | null;
  scoped_runs?: {
    scope: string;
    run: { id: string; session_id: string | null };
    state: string;
    summary: string;
    error: string | null;
    /** Only observation reviews; legacy runs omit it. */
    observation?: {
      external: boolean;
      inputs: {
        observation_id: string;
        hash: string;
        source: { source_id: string; version: number };
      }[];
    } | null;
  }[];
  /** Absent in snapshots written before event ingress. */
  sources?: SourceBinding[];
  source_history_global?: number | null;
  summary: string;
  attention: string | null;
  messages: CoordinationMessage[];
  questions: CoordinationQuestion[];
  changes: RequirementChange[];
  notifications: AssistantNotification[];
  requests: {
    id: string;
    project_path: string;
    content: string;
    handled: boolean;
  }[];
  turns: number;
  dispatches: number;
  /** Absent in snapshots written before the conversation feature. */
  conversation?: ConversationTurn[];
  memory_proposals?: MemoryProposal[];
  hierarchy?: ProjectHierarchy | null;
}
export interface ConversationTurn {
  id: string;
  created_at: string | number;
  author: "user" | "assistant";
  content: string;
  reply_to: string | null;
  project_paths: string[];
  status: "recorded" | "reviewing" | "handled" | "failed" | "unknown";
  goal_ids: string[];
  question_ids: string[];
  error: string | null;
  source_run_id: string | null;
  actor?: string | null;
}
export interface MemoryProposal {
  id: string;
  turn_id: string;
  project_path: string;
  category: string;
  content: string;
  content_hash: string;
  state: "proposed" | "confirmed" | "rejected" | "failed";
  memory_id: string | null;
  error: string | null;
}
export interface CoordinationMessage {
  id: string;
  goal_id: string;
  assignment_id: string;
  contract_revision: number;
  mode: string;
  author: string;
  content: string;
  state: string;
  outcome: string;
  confirmed: boolean;
}
export interface CoordinationQuestion {
  id: string;
  goal_id: string;
  assignment_id: string;
  contract_revision: number;
  title: string;
  context: string;
  options: string[];
  blocking: boolean;
  state: string;
  answer: string | null;
  answered_by: string | null;
  source_input: PendingInput | null;
}
export interface RequirementChange {
  id: string;
  goal_id: string;
  from_revision: number;
  to_revision: number;
  before_title: string;
  before_acceptance: string;
  title: string;
  acceptance: string;
  reason: string;
  state: string;
  proposed: boolean;
  remember: boolean;
  memory_id: string | null;
}
export interface AssistantNotification {
  id: string;
  goal_id: string;
  kind: string;
  title: string;
  body: string;
  session_id: string | null;
  reference_id: string;
  read: boolean;
  desktop: string;
}
export interface AssistantSnapshot {
  state: AssistantState;
  sessions: SessionInfo[];
  memories: MemoryRecord[];
}
/** Host-verified identity; provider/account_scope are adapter-reported and pinned by the user. */
export interface SourcePrincipal {
  plugin: string;
  realm: string;
  connector_id: string;
}
export interface SourceBinding {
  source_id: string;
  principal: SourcePrincipal;
  provider: string;
  account_scope: string;
  resource_filter: string[];
  actor_filter: string[];
  project_paths: string[];
  goal_ids: string[];
  streams: string[];
  version: number;
  enabled: boolean;
  history_limit?: number | null;
}
export interface SourceHealth {
  key: string;
  kind: string;
  state: string;
  accepted: number;
  filtered: number;
  duplicate: number;
  conflicts: number;
  rejected: number;
  unbound: number;
  gap_count: number;
  gap_first: string | null;
  gap_last: string | null;
  detail: string | null;
}
export interface TargetReceipt {
  observation_id: string;
  goal_id: string;
  source_id: string;
  binding_version: number;
  project_path: string;
  state: string;
  run_id: string | null;
  output_id: string | null;
  seq: number;
}
/** Installed, trusted, enabled connectors whose manifest declares `observations`. */
export interface ObservationConnector extends SourcePrincipal {
  provider: string;
  label: string;
}
/** Current committed cursor of one configured stream. `null` is a real "no checkpoint". */
export interface ObservationStream {
  source_id: string;
  stream_id: string;
  checkpoint: string | null;
}
/** A Core-approved, exact cursor reset the adapter may present once; body-free. */
export interface SourceResetApproval {
  approval_ref: string;
  source_id: string;
  binding_version: number;
  stream_id: string;
  checkpoint_before: string | null;
  checkpoint_after: string | null;
  reason: string;
  approved_at_ms: number;
}
export interface ObservationSnapshot {
  health: SourceHealth[];
  receipts: TargetReceipt[];
  connectors: ObservationConnector[];
  /** Absent from Cores written before reset approval. */
  streams?: ObservationStream[];
  resets?: SourceResetApproval[];
}
/** Collision-free identity: no delimiter a plugin, realm or connector id could contain. */
export const principalKey = (p: SourcePrincipal) =>
  JSON.stringify([p.plugin, p.realm, p.connector_id]);
export const DEFAULT_SOURCE_HISTORY = 2000;
export const DEFAULT_GLOBAL_HISTORY = 10_000;

export const assistantApi = {
  /** User-only developer read; older hosts and injected test APIs may not have it. */
  observations: async (limit = 50) =>
    await call<ObservationSnapshot>("assistant.observations", { limit }, null),
  catalog: async () =>
    await Promise.all([
      call<Project[]>("projects.list", null, null),
      call<ProviderInfo[]>("providers.list", {}, null),
    ]),
  snapshot: async () =>
    await call<AssistantSnapshot>("assistant.snapshot", null, null),
  edit: async (revision: number, edit: unknown) =>
    await call<AssistantState>("assistant.edit", { revision, edit }, null),
  memory: async (name: string, args: unknown) =>
    await call(`memory.${name}`, args, null),
};

/** `observations` is optional so injected or older APIs degrade to a visible load error. */
export type AssistantApi = Omit<typeof assistantApi, "observations"> &
  Partial<Pick<typeof assistantApi, "observations">>;

export function sameGoalTarget(old: Goal, current: Goal, review = false) {
  return (
    old.contract_revision === current.contract_revision &&
    old.priority === current.priority &&
    old.status === current.status &&
    old.assignments.at(-1)?.id === current.assignments.at(-1)?.id &&
    old.assignments.at(-1)?.taken_over ===
      current.assignments.at(-1)?.taken_over &&
    (!review ||
      old.assignments.at(-1)?.result?.id ===
        current.assignments.at(-1)?.result?.id)
  );
}

/** Unrelated worker activity may advance the global revision while a user fills a form. */
export function sameEditTarget(
  before: AssistantState,
  after: AssistantState,
  action: unknown
) {
  if (action == null || typeof action !== "object" || !("kind" in action))
    return false;
  if (action.kind === "settings")
    return JSON.stringify(before.settings) === JSON.stringify(after.settings);
  if (
    action.kind === "source" ||
    action.kind === "source_capacity" ||
    action.kind === "source_reset"
  ) {
    // Bindings are versioned: the draft's exact source/version must still be current.
    const fields = "binding" in action ? action.binding : action;
    if (fields == null || typeof fields !== "object") return false;
    const id = "source_id" in fields ? fields.source_id : null;
    const version =
      "expected_version" in fields
        ? fields.expected_version
        : "binding_version" in fields
          ? fields.binding_version
          : null;
    if (id == null) return true; // create: Core derives the id and rejects a duplicate
    const stream =
      action.kind === "source_reset" && "stream_id" in fields
        ? fields.stream_id
        : null;
    return (after.sources ?? []).some(
      (s) =>
        s.source_id === id &&
        s.version === version &&
        // Core rejects a reset once the source is disabled or the stream is gone.
        (action.kind !== "source_reset" ||
          (s.enabled &&
            typeof stream === "string" &&
            s.streams.includes(stream)))
    );
  }
  if ("proposal_id" in action) {
    const old = before.memory_proposals?.find(
      (p) => p.id === action.proposal_id
    );
    const current = after.memory_proposals?.find(
      (p) => p.id === action.proposal_id
    );
    return (
      old != null &&
      current != null &&
      current.state === "proposed" &&
      old.state === current.state &&
      old.content_hash === current.content_hash &&
      (!("content_hash" in action) ||
        action.content_hash === current.content_hash)
    );
  }
  const questionId =
    "question_id" in action
      ? action.question_id
      : "answers_question" in action &&
          typeof action.answers_question === "string"
        ? action.answers_question
        : null;
  if (questionId != null) {
    const old = before.questions?.find((q) => q.id === questionId);
    const current = after.questions?.find((q) => q.id === questionId);
    return (
      old != null &&
      current != null &&
      old.state === current.state &&
      old.contract_revision === current.contract_revision &&
      old.assignment_id === current.assignment_id
    );
  }
  const id =
    "goal_id" in action ? action.goal_id : "id" in action ? action.id : null;
  if (id == null) return true;
  const old = before.goals.find((g) => g.id === id);
  const current = after.goals.find((g) => g.id === id);
  return (
    old != null &&
    current != null &&
    sameGoalTarget(old, current, action.kind === "review") &&
    (action.kind !== "goal" || old.priority === current.priority)
  );
}

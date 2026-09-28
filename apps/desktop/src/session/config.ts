import type {
  MemoryAccess,
  ProviderInfo,
  PermissionMode,
  ResolvedWorktreeBaseline,
  Sandbox,
  WorktreeBaselineKind,
  WorktreeBaselineOption,
} from "../bridge";
import type { SessionMode } from "./mode";
import type { SceneInfo } from "./scene";

/**
 * Everything configured once per session rather than once per turn.
 *
 * This is one bag of state, but deliberately *not* one panel: related Provider/model choices share
 * one picker while policy, worktree, scene, and memory settings keep their own controls.
 */
export interface SessionConfig {
  providers: ProviderInfo[];
  providersStatus: "loading" | "ready" | "error";
  provider: string;
  onProvider: (v: string) => void;
  /** A running turn or in-flight runtime replacement owns the provider boundary. */
  providerChangeDisabled?: boolean;
  providerSwitching?: boolean;
  /** A foreign Provider choice replaces the active runtime; null leaves its model unspecified. */
  onProviderModel: (provider: string, model: string | null) => void;
  onReloadProviders: () => void;
  /** The engine's two permission axes. Read here, but set only as a pair — see `onSessionMode`. */
  mode: PermissionMode;
  sandbox: Sandbox;
  /** Creation/persistence is in flight; another choice would not describe the next turn yet. */
  modeChangeDisabled: boolean;
  onSessionMode: (v: SessionMode) => void;
  worktreeBase: WorktreeBaselineKind | null;
  /** Immutable baseline recorded for the active session; null also covers a non-worktree session. */
  activeWorktreeBaseline: ResolvedWorktreeBaseline | null;
  /** A pre-provenance worktree session must be shown as unknown, never as Off. */
  activeWorktreeUnknown: boolean;
  worktreeOptions: WorktreeBaselineOption[];
  worktreeOptionsLoading: boolean;
  onWorktreeBase: (v: WorktreeBaselineKind | null) => void;
  /** Component-policy gate for the memory picker and its persistence calls. */
  memoryEnabled: boolean;
  memoryRead: MemoryAccess;
  memoryWrite: MemoryAccess;
  onMemoryPolicy: (read: MemoryAccess, write: MemoryAccess) => void;
  /** Whether a session exists yet — some controls have nothing to act on before that. */
  hasSession: boolean;
  /** Component-policy gate for every scene, banner, studio, and pipeline surface. */
  scenesEnabled: boolean;
  /** Resolved scenes (project > user > plugin > builtin) for the scene chip and pickers. */
  scenes: SceneInfo[];
  activeScene: SceneInfo | null;
  /** Agent-owned scene routing for this session. The currently selected scene remains visible. */
  autoScene: boolean;
  onAutoScene: (enabled: boolean) => void;
  onScene: (reference: string | null, strength: "soft" | "full") => void;
  /** Opens the complete scene library where scenes can be created, edited, or duplicated. */
  onManageScenes: () => void;
  /** The user overrode a field the active scene sets (chip shows "customized", scene unchanged). */
  sceneCustomized: boolean;
  /** Soft-apply deferrals (providers/model/effort/worktree) — non-empty shows the partial dot. */
  scenePendingFields: string[];
  /** New session, full-apply, in the active scene — closes the soft-apply gap. */
  onRestartInScene: () => void;
}

export type MemoryPresetId =
  | "codex_default"
  | "standard"
  | "read_only"
  | "private"
  | "learn_only";

export interface MemoryPreset {
  id: MemoryPresetId;
  read: MemoryAccess;
  write: MemoryAccess;
  isDefault?: boolean;
}

const MEMORY_PRESETS: readonly MemoryPreset[] = [
  { id: "standard", read: "inherit", write: "inherit", isDefault: true },
  { id: "read_only", read: "allow", write: "deny" },
  { id: "private", read: "deny", write: "deny" },
  { id: "learn_only", read: "deny", write: "allow" },
];

const CODEX_MEMORY_PRESETS: readonly MemoryPreset[] = [
  { id: "codex_default", read: "inherit", write: "inherit", isDefault: true },
  { id: "standard", read: "allow", write: "allow" },
  { id: "read_only", read: "allow", write: "deny" },
  { id: "private", read: "deny", write: "deny" },
  { id: "learn_only", read: "deny", write: "allow" },
];

/** Codex keeps inherited C2 recall out of its visible ACP user turn. */
export function memoryPresetsForProvider(
  provider: string
): readonly MemoryPreset[] {
  return provider === "codex" ? CODEX_MEMORY_PRESETS : MEMORY_PRESETS;
}
